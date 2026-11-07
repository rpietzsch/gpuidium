//! `WebviewHost`: the boundary between product logic and native WebViews (CORE-004).
//!
//! The domain model never sees Wry or GPUI-WebView types. Native callbacks run on
//! the UI thread and only push `HostEvent`s into a bounded channel; the shell
//! drains it and applies state changes on the GPUI side.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};

use gpui_kit::{App, AppContext as _, Entity, Window};
use gpui_webview::WebView;
use gpuidium_core::{
    AppId, ProfileId,
    diagnostics::{log, redact_url},
    policy::{NavDecision, decide_navigation, decide_new_window},
    profiles,
};
use wry::{NewWindowResponse, PageLoadEvent, WebContext, WebViewBuilder};

use crate::external;

/// App-level shortcuts. WebKit owns keyboard focus while a page is shown, so GPUI's
/// key bindings never see these; the host intercepts them natively and forwards them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    Open,
    Reload,
    Manage,
    Keys,
    Select(usize),
    Back,
    Forward,
}

#[cfg(target_os = "linux")]
fn shortcut_for(ev: &gtk::gdk::EventKey) -> Option<Shortcut> {
    use gtk::gdk::{ModifierType as M, keys::constants as k};
    let st = ev.state();
    let key = ev.keyval().to_lower();
    if key == k::F1 {
        return Some(Shortcut::Keys);
    }
    if st.contains(M::MOD1_MASK) && !st.contains(M::CONTROL_MASK) {
        return match key {
            k::Left => Some(Shortcut::Back),
            k::Right => Some(Shortcut::Forward),
            _ => None,
        };
    }
    if !st.contains(M::CONTROL_MASK) || st.contains(M::MOD1_MASK) {
        return None;
    }
    match key {
        k::l | k::k => Some(Shortcut::Open),
        k::r => Some(Shortcut::Reload),
        k::comma => Some(Shortcut::Manage),
        k::question | k::slash if st.contains(M::SHIFT_MASK) || key == k::question => {
            Some(Shortcut::Keys)
        }
        _ => {
            let n = key.to_unicode()?.to_digit(10)? as usize;
            (1..=9).contains(&n).then(|| Shortcut::Select(n - 1))
        }
    }
}

/// Make WebKit treat the page as focused so the text caret is drawn.
///
/// `lb-wry` wraps GPUI's own X11 window in a GTK window, but that window's focus events go
/// to GPUI's connection, so GTK never marks it active and WebKit never marks the page
/// focused (`document.hasFocus()` stays false, the caret stays hidden). We hand GTK the
/// focus-in event it would otherwise miss, then focus the web view widget.
#[cfg(target_os = "linux")]
fn activate_page(view: &wry::WebView) {
    use gtk::glib::translate::ToGlibPtr;
    use gtk::prelude::*;
    use wry::WebViewExtUnix as _;

    let webview = view.webview();
    if let Some(top) = webview.toplevel()
        && let Some(window) = top.window()
    {
        let event = gtk::gdk::Event::new(gtk::gdk::EventType::FocusChange);
        // SAFETY: `event` owns a freshly allocated GdkEventFocus; we only fill its plain
        // fields (the window reference is transferred to the event, which unrefs it on drop).
        unsafe {
            let raw: *mut gtk::gdk::ffi::GdkEventFocus =
                ToGlibPtr::<*mut gtk::gdk::ffi::GdkEvent>::to_glib_none(&event).0 as *mut _;
            (*raw).window = window.to_glib_full();
            (*raw).send_event = 1;
            (*raw).in_ = 1;
        }
        // WebKit listens for `focus-in-event` on the toplevel window; deliver it directly.
        top.event(&event);
    }
    let _ = view.focus();
}

#[cfg(not(target_os = "linux"))]
fn activate_page(view: &wry::WebView) {
    let _ = view.focus();
}

#[derive(Debug, Clone)]
pub enum HostEvent {
    Shortcut(Shortcut),
    /// The page's favicon as PNG bytes (bounded size).
    Favicon(AppId, Vec<u8>),
    LoadStarted(AppId, String),
    LoadFinished(AppId, String),
    /// The engine reported a load failure (URL redacted, short reason).
    LoadFailed(AppId, String, String),
    /// A page asked for a desktop notification and the user opted in (UX-013).
    Notify(AppId, String, String),
    Title(AppId, String),
    /// A navigation was refused by policy (URL already redacted).
    Blocked(String),
    /// A popup/new-window request was sent to the system browser.
    OpenedExternally(String),
}

#[derive(Debug, thiserror::Error)]
pub enum HostError {
    #[error("unknown app")]
    Unknown,
    #[error("profile storage: {0}")]
    Profile(#[from] std::io::Error),
    #[error("webview: {0}")]
    Native(String),
}

/// Operations the shell needs from a host. GPUI requires `window`/`cx` for
/// native attachment, so (unlike the spec's conceptual sketch) they are passed in.
pub trait WebviewHost {
    fn create(
        &mut self,
        window: &mut Window,
        cx: &mut App,
        app: AppId,
        profile: ProfileId,
        url: &str,
    ) -> Result<(), HostError>;
    fn contains(&self, app: AppId) -> bool;
    fn show_only(&mut self, cx: &mut App, app: Option<AppId>);
    fn navigate(&mut self, cx: &mut App, app: AppId, url: &str) -> Result<(), HostError>;
    fn back(&mut self, cx: &mut App, app: AppId);
    fn forward(&mut self, cx: &mut App, app: AppId);
    fn reload(&mut self, cx: &mut App, app: AppId);
    /// Close the native view and release its profile store.
    fn close(&mut self, app: AppId);
    fn drain_events(&mut self) -> Vec<HostEvent>;
}

struct Hosted {
    view: Entity<WebView>,
    // Keeps the per-profile engine context alive as long as the view.
    _context: WebContext,
}

pub struct WryHost {
    data_dir: std::path::PathBuf,
    /// Global opt-in for page notifications (default off: requests are denied).
    notifications: Arc<AtomicBool>,
    views: HashMap<AppId, Hosted>,
    /// Last visibility we applied to each native view. `show_only` runs on every UI event,
    /// so only real changes may reach the native view (repeated show/hide/focus calls make
    /// pages repaint and steal focus).
    applied: HashMap<AppId, bool>,
    tx: SyncSender<HostEvent>,
    rx: Receiver<HostEvent>,
}

impl WryHost {
    pub fn new(data_dir: std::path::PathBuf) -> Self {
        let (tx, rx) = sync_channel(256);
        Self {
            data_dir,
            notifications: Arc::new(AtomicBool::new(false)),
            views: HashMap::new(),
            applied: HashMap::new(),
            tx,
            rx,
        }
    }

    pub fn set_notifications(&self, on: bool) {
        self.notifications.store(on, Ordering::Relaxed);
    }

    pub fn ids(&self) -> Vec<AppId> {
        self.views.keys().copied().collect()
    }

    pub fn view(&self, app: AppId) -> Option<Entity<WebView>> {
        self.views.get(&app).map(|h| h.view.clone())
    }
}

impl WebviewHost for WryHost {
    fn create(
        &mut self,
        window: &mut Window,
        cx: &mut App,
        app: AppId,
        profile: ProfileId,
        url: &str,
    ) -> Result<(), HostError> {
        if self.views.contains_key(&app) {
            return Ok(());
        }
        // Every app owns a distinct, persistent engine store (never shared).
        let dir = profiles::ensure(&self.data_dir, profile)?;
        let mut context = WebContext::new(Some(dir));

        let (t_nav, t_win, t_load, t_title) = (
            self.tx.clone(),
            self.tx.clone(),
            self.tx.clone(),
            self.tx.clone(),
        );
        // No `with_ipc_handler`: pages have no channel to native code.
        let builder = WebViewBuilder::new_with_web_context(&mut context)
            .with_url(url)
            .with_navigation_handler(move |target| {
                if decide_navigation(&target) == NavDecision::Allow {
                    true
                } else {
                    let _ = t_nav.try_send(HostEvent::Blocked(redact_url(&target)));
                    false
                }
            })
            .with_new_window_req_handler(move |target, _| {
                let redacted = redact_url(&target);
                if decide_new_window(&target) == NavDecision::OpenExternal
                    && external::open(&target)
                {
                    let _ = t_win.try_send(HostEvent::OpenedExternally(redacted));
                } else {
                    let _ = t_win.try_send(HostEvent::Blocked(redacted));
                }
                NewWindowResponse::Deny
            })
            .with_on_page_load_handler(move |event, target| {
                let ev = match event {
                    PageLoadEvent::Started => HostEvent::LoadStarted(app, target),
                    PageLoadEvent::Finished => HostEvent::LoadFinished(app, target),
                };
                let _ = t_load.try_send(ev);
            })
            .with_document_title_changed_handler(move |title| {
                let _ = t_title.try_send(HostEvent::Title(app, title));
            });

        let webview =
            WebView::build(builder, window, cx).map_err(|e| HostError::Native(e.to_string()))?;
        #[cfg(target_os = "linux")]
        {
            use webkit2gtk::WebViewExt as _;
            use wry::WebViewExtUnix as _;
            let t_fail = self.tx.clone();
            webview
                .raw()
                .webview()
                .connect_load_failed(move |_, _, uri, err| {
                    let _ = t_fail.try_send(HostEvent::LoadFailed(
                        app,
                        uri.to_string(),
                        err.message().to_string(),
                    ));
                    true // handled: no WebKit error page, the shell renders its own error state
                });
        }
        #[cfg(target_os = "linux")]
        {
            use gtk::glib::ObjectExt as _;
            use gtk::prelude::WidgetExt as _;
            use webkit2gtk::{PermissionRequestExt as _, WebViewExt as _};
            use wry::WebViewExtUnix as _;
            let gtk_view = webview.raw().webview();
            let t_icon = self.tx.clone();
            gtk_view.connect_favicon_notify(move |v| {
                let Some(surface) = v.favicon() else {
                    log("favicon notify: none");
                    return;
                };
                match gtk::cairo::ImageSurface::try_from(surface) {
                    Ok(img) => {
                        let mut png = Vec::new();
                        match img.write_to_png(&mut png) {
                            Ok(()) if png.len() <= 256 * 1024 => {
                                log(&format!("favicon: {} bytes", png.len()));
                                let _ = t_icon.try_send(HostEvent::Favicon(app, png));
                            }
                            Ok(()) => log("favicon too large, ignored"),
                            Err(e) => log(&format!("favicon png encode failed: {e}")),
                        }
                    }
                    Err(_) => log("favicon is not an image surface"),
                }
            });
            let t_key = self.tx.clone();
            gtk_view.connect_key_press_event(move |_, ev| match shortcut_for(ev) {
                Some(sc) => {
                    let _ = t_key.try_send(HostEvent::Shortcut(sc));
                    gtk::glib::Propagation::Stop
                }
                None => gtk::glib::Propagation::Proceed,
            });
            // Every permission (camera, location, ...) is denied; only notifications
            // may be allowed, and only while the user's global opt-in is on.
            let flag = self.notifications.clone();
            gtk_view.connect_permission_request(move |_, req| {
                let is_notification = req.is::<webkit2gtk::NotificationPermissionRequest>();
                log(&format!(
                    "permission request {}: notification={is_notification} opted_in={}",
                    req.type_().name(),
                    flag.load(Ordering::Relaxed)
                ));
                if is_notification && flag.load(Ordering::Relaxed) {
                    req.allow();
                } else {
                    req.deny();
                }
                true
            });
            let (flag, t_note) = (self.notifications.clone(), self.tx.clone());
            gtk_view.connect_show_notification(move |_, n| {
                if flag.load(Ordering::Relaxed) {
                    use webkit2gtk::NotificationExt as _;
                    let clip = |s: Option<webkit2gtk::glib::GString>| {
                        s.map(|g| g.chars().take(200).collect::<String>())
                            .unwrap_or_default()
                    };
                    let _ =
                        t_note.try_send(HostEvent::Notify(app, clip(n.title()), clip(n.body())));
                }
                true
            });
        }
        log(&format!("created webview for {app}"));
        let view = cx.new(|_| webview);
        self.views.insert(
            app,
            Hosted {
                view,
                _context: context,
            },
        );
        Ok(())
    }

    fn contains(&self, app: AppId) -> bool {
        self.views.contains_key(&app)
    }

    /// Show only the selected native view (or none). Applies changes only, in a safe order:
    /// hide the views that must go, then show the target and give it keyboard focus. Without
    /// the focus call WebKit treats the page as inactive and never draws the text caret.
    fn show_only(&mut self, cx: &mut App, app: Option<AppId>) {
        let ids: Vec<AppId> = self.views.keys().copied().collect();
        // Hide first, so no stale view ever covers chrome or the new page.
        for id in &ids {
            if Some(*id) != app && self.applied.get(id) != Some(&false) {
                if let Some(h) = self.views.get(id) {
                    h.view.update(cx, |w, _| {
                        // A hidden WebKit view must not keep keyboard focus, or GPUI never
                        // sees Esc or typing in the Open panel.
                        let _ = w.raw().focus_parent();
                        w.hide();
                    });
                }
                self.applied.insert(*id, false);
            }
        }
        if let Some(id) = app
            && self.applied.get(&id) != Some(&true)
            && let Some(h) = self.views.get(&id)
        {
            h.view.update(cx, |w, _| {
                w.show();
                activate_page(w.raw());
            });
            self.applied.insert(id, true);
        }
    }

    fn navigate(&mut self, cx: &mut App, app: AppId, url: &str) -> Result<(), HostError> {
        let h = self.views.get(&app).ok_or(HostError::Unknown)?;
        h.view.update(cx, |w, _| w.load_url(url));
        Ok(())
    }

    fn back(&mut self, cx: &mut App, app: AppId) {
        if let Some(h) = self.views.get(&app) {
            h.view.update(cx, |w, _| {
                let _ = w.back();
            });
        }
    }

    fn forward(&mut self, cx: &mut App, app: AppId) {
        if let Some(h) = self.views.get(&app) {
            h.view.update(cx, |w, _| {
                let _ = w.forward();
            });
        }
    }

    fn reload(&mut self, cx: &mut App, app: AppId) {
        if let Some(h) = self.views.get(&app) {
            h.view.update(cx, |w, _| {
                let _ = w.raw().reload();
            });
        }
    }

    fn close(&mut self, app: AppId) {
        // Dropping the entity and context releases the native view and store.
        self.views.remove(&app);
        self.applied.remove(&app);
    }

    fn drain_events(&mut self) -> Vec<HostEvent> {
        self.rx.try_iter().collect()
    }
}
