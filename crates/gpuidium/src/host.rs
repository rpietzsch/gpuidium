//! `WebviewHost`: the boundary between product logic and native WebViews (CORE-004).
//!
//! The domain model never sees Wry or GPUI-WebView types. Native callbacks run on
//! the UI thread and only push `HostEvent`s into a bounded channel; the shell
//! drains it and applies state changes on the GPUI side.

use async_channel::{Receiver, Sender};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui_kit::{App, AppContext as _, Entity, Window};
use gpui_webview::WebView;
use gpuidium_core::{
    AppId, ProfileId,
    diagnostics::{log, redact_url},
    policy::{NavDecision, decide_navigation, decide_new_window},
    profiles,
    zoom::ZoomChange,
};
use wry::{NewWindowResponse, PageLoadEvent, WebContext, WebViewBuilder};

use crate::external;

/// App-level shortcuts. WebKit owns keyboard focus while a page is shown, so GPUI's
/// key bindings never see these; the host intercepts them natively and forwards them.
// Only the Linux backend emits shortcuts so far (macOS/Windows interception: backlog).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    Open,
    Reload,
    Manage,
    Keys,
    Select(usize),
    Back,
    Forward,
    Zoom(ZoomChange),
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
    // Only exact Ctrl+key combinations: pages use the Shift variants (e.g. Mattermost
    // Ctrl+Shift+K/L/A/M), and Ctrl+K is left to pages (quick switchers).
    let shift = st.contains(M::SHIFT_MASK);
    match key {
        // Zoom: "+" needs Shift on US layouts, is its own key on German ones.
        k::plus | k::equal | k::KP_Add => Some(Shortcut::Zoom(ZoomChange::In)),
        k::minus | k::KP_Subtract => Some(Shortcut::Zoom(ZoomChange::Out)),
        k::_0 | k::KP_0 => Some(Shortcut::Zoom(ZoomChange::Reset)),
        k::question => Some(Shortcut::Keys),
        k::slash if shift => Some(Shortcut::Keys),
        _ if shift => None,
        k::l => Some(Shortcut::Open),
        k::r => Some(Shortcut::Reload),
        k::comma => Some(Shortcut::Manage),
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

// Shortcut/LoadFailed come from WebKitGTK signals only; Favicon and Notify also from macOS.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug, Clone)]
pub enum HostEvent {
    Shortcut(Shortcut),
    /// The page's favicon as PNG bytes (bounded size).
    Favicon(AppId, Vec<u8>),
    LoadStarted(AppId, String),
    LoadFinished(AppId, String),
    /// The engine reported a load failure (URL redacted, short reason).
    LoadFailed(AppId, String, String),
    /// A page showed a notification and the app's notifications are on: app, the page's
    /// notification id (macOS, for click routing; 0 on Linux), title, body.
    Notify(AppId, u64, String, String),
    Title(AppId, String),
    /// A navigation was refused by policy (URL already redacted).
    Blocked(String),
    /// A popup/new-window request was sent to the system browser.
    OpenedExternally(String),
}

/// macOS: WKWebView's default user agent omits Safari's `Version/x Safari/y` tokens, so
/// sites that sniff the browser (e.g. WhatsApp Web) treat it as an outdated Safari.
/// Send what the installed Safari sends; it runs on the same system WebKit.
/// `None` (keep WebKit's default) if Safari's version cannot be read.
#[cfg(target_os = "macos")]
fn safari_user_agent() -> Option<&'static str> {
    static UA: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    UA.get_or_init(|| {
        use objc2_foundation::{NSBundle, NSString};
        let bundle = NSBundle::bundleWithPath(&NSString::from_str("/Applications/Safari.app"))?;
        let version = bundle
            .objectForInfoDictionaryKey(&NSString::from_str("CFBundleShortVersionString"))?
            .downcast::<NSString>()
            .ok()?
            .to_string();
        // The platform and WebKit tokens are frozen by Apple; only Version/ varies.
        Some(format!(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 \
             (KHTML, like Gecko) Version/{version} Safari/605.1.15"
        ))
    })
    .as_deref()
}

/// Bind `builder` to the profile's own persistent engine store (P0 isolation).
/// Linux/Windows: the `WebContext` data directory already is that store.
/// macOS: WKWebView ignores the directory, so each profile uses the `WKWebsiteDataStore`
/// whose identifier is the profile UUID. Needs macOS 14; older systems fail closed
/// instead of sharing the default store (`third_party/lb-wry/PATCHES.md` #5).
/// It also sets the Safari-compatible user agent there (`safari_user_agent`).
pub fn bind_profile_store(builder: WebViewBuilder<'_>, profile: ProfileId) -> WebViewBuilder<'_> {
    #[cfg(target_os = "macos")]
    {
        use wry::WebViewBuilderExtDarwin as _;
        let builder = builder.with_data_store_identifier(profile.as_uuid().into_bytes());
        match safari_user_agent() {
            Some(ua) => builder.with_user_agent(ua),
            None => builder,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = profile;
        builder
    }
}

/// Delete one profile's browsing data: its directory, and on macOS also its
/// `WKWebsiteDataStore` (asynchronous, best effort; failures are logged).
/// The caller must have closed the profile's WebView first.
pub fn delete_profile(data_dir: &std::path::Path, profile: ProfileId) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use wry::WebViewExtDarwin as _;
        wry::WebView::remove_data_store(&profile.as_uuid().into_bytes(), move |res| {
            if let Err(e) = res {
                log(&format!(
                    "removing WebKit data store of {profile} failed: {e}"
                ));
            }
        });
    }
    profiles::delete(data_dir, profile)
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
    /// Page zoom in percent (100 = normal).
    fn set_zoom(&mut self, cx: &mut App, app: AppId, percent: u16);
    /// Close the native view and release its profile store.
    fn close(&mut self, app: AppId);
    /// Stream of native events. Awaiting it wakes the UI as soon as a callback sends, instead
    /// of polling on a timer.
    fn events(&self) -> Receiver<HostEvent>;
}

struct Hosted {
    view: Entity<WebView>,
    // Keeps the per-profile engine context alive as long as the view.
    _context: WebContext,
}

pub struct WryHost {
    data_dir: std::path::PathBuf,
    /// Per-app notification switch (absent = on). Shared with the native callbacks.
    notify_flags: HashMap<AppId, Arc<AtomicBool>>,
    /// Spell checking on/off and languages (Linux; macOS uses the system's languages).
    spell_on: bool,
    spell_langs: Vec<String>,
    /// All pages muted (the sidebar's sound switch); applied to new views too.
    muted: bool,
    views: HashMap<AppId, Hosted>,
    /// Last visibility we applied to each native view. `show_only` runs on every UI event,
    /// so only real changes may reach the native view (repeated show/hide/focus calls make
    /// pages repaint and steal focus).
    applied: HashMap<AppId, bool>,
    tx: Sender<HostEvent>,
    rx: Receiver<HostEvent>,
}

impl WryHost {
    pub fn new(data_dir: std::path::PathBuf) -> Self {
        let (tx, rx) = async_channel::bounded(256);
        Self {
            data_dir,
            notify_flags: HashMap::new(),
            spell_on: true,
            spell_langs: Vec::new(),
            muted: false,
            views: HashMap::new(),
            applied: HashMap::new(),
            tx,
            rx,
        }
    }

    fn notify_flag(&mut self, app: AppId) -> Arc<AtomicBool> {
        self.notify_flags
            .entry(app)
            .or_insert_with(|| Arc::new(AtomicBool::new(true)))
            .clone()
    }

    /// Switch one app's page notifications. Takes effect at once: Linux drops them in the
    /// signal handler, macOS also tells the page (its `Notification.permission`).
    pub fn set_app_notifications(&mut self, cx: &mut App, app: AppId, on: bool) {
        self.notify_flag(app).store(on, Ordering::Relaxed);
        #[cfg(target_os = "macos")]
        if let Some(h) = self.views.get(&app) {
            let _ = h.view.read(cx).raw().evaluate_script(&format!(
                "window.__gpuidiumNotify && window.__gpuidiumNotify.setPermission({on})"
            ));
        }
        #[cfg(not(target_os = "macos"))]
        let _ = cx;
    }

    /// Mute or unmute the audio of every page, now and for views created later.
    pub fn set_muted(&mut self, cx: &mut App, muted: bool) {
        self.muted = muted;
        for h in self.views.values() {
            apply_muted(h.view.read(cx), muted);
        }
    }

    /// Spell checking for all views, now and for views created later.
    pub fn set_spellcheck(&mut self, cx: &mut App, on: bool, langs: &[String]) {
        self.spell_on = on;
        self.spell_langs = langs.to_vec();
        #[cfg(target_os = "linux")]
        for h in self.views.values() {
            use webkit2gtk::WebViewExt as _;
            use wry::WebViewExtUnix as _;
            if let Some(ctx) = h.view.read(cx).raw().webview().context() {
                apply_spellcheck(&ctx, on, langs);
            }
        }
        #[cfg(target_os = "macos")]
        {
            // WebKit keeps one process-wide state; flipping it through any view updates all.
            use wry::WebViewExtMacOS as _;
            if crate::mac::spellcheck_default() != on {
                match self.views.values().next() {
                    Some(h) => crate::mac::toggle_spellcheck(&h.view.read(cx).raw().webview()),
                    None => crate::mac::set_spellcheck_default(on),
                }
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let _ = cx;
    }

    pub fn ids(&self) -> Vec<AppId> {
        self.views.keys().copied().collect()
    }

    pub fn view(&self, app: AppId) -> Option<Entity<WebView>> {
        self.views.get(&app).map(|h| h.view.clone())
    }
}

#[cfg(target_os = "macos")]
impl WryHost {
    /// Ask the page for its favicon (macOS has no WebKit favicon API, see
    /// `js/favicon_start.js`) and poll for the result a few times. A found icon
    /// arrives as `HostEvent::Favicon`, like the WebKitGTK signal on Linux.
    pub fn refresh_favicon(&self, cx: &mut App, app: AppId) {
        let Some(h) = self.views.get(&app) else {
            return;
        };
        let _ = h
            .view
            .read(cx)
            .raw()
            .evaluate_script(include_str!("js/favicon_start.js"));
        let view = h.view.downgrade();
        let tx = self.tx.clone();
        let done = Arc::new(AtomicBool::new(false));
        cx.spawn(async move |cx| {
            for ms in [600u64, 1200, 2500, 5000] {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(ms))
                    .await;
                if done.load(Ordering::Relaxed) {
                    return;
                }
                let (tx, done) = (tx.clone(), done.clone());
                let alive = view.read_with(cx, |w, _| {
                    let _ = w.raw().evaluate_script_with_callback(
                        include_str!("js/favicon_read.js"),
                        move |res| {
                            if let Some(png) = favicon_png_from_hex(&res) {
                                done.store(true, Ordering::Relaxed);
                                let _ = tx.try_send(HostEvent::Favicon(app, png));
                            }
                        },
                    );
                });
                if alive.is_err() {
                    return;
                }
            }
        })
        .detach();
    }
}

#[cfg(target_os = "macos")]
impl WryHost {
    /// Give the key event being handled right now to `app`'s page, if that page is shown.
    /// For Cmd+.: an unhandled Cmd+. becomes AppKit's `cancelOperation:` and never reaches
    /// WebKit, so pages could not use it (Mattermost: toggle the right sidebar).
    pub fn forward_current_key(&self, cx: &App, app: AppId) {
        use objc2_app_kit::NSApplication;
        use wry::WebViewExtMacOS as _;
        if self.applied.get(&app) != Some(&true) {
            return;
        }
        let (Some(h), Some(mtm)) = (self.views.get(&app), objc2::MainThreadMarker::new()) else {
            return;
        };
        if let Some(event) = NSApplication::sharedApplication(mtm).currentEvent() {
            h.view.read(cx).raw().webview().keyDown(&event);
        }
    }
}

#[cfg(target_os = "macos")]
impl WryHost {
    /// Collect queued page notifications (`js/notify_shim.js`) from every view whose app
    /// has notifications on. Called from the shell's 1 s tick.
    pub fn poll_notifications(&self, cx: &App) {
        for (&app, h) in &self.views {
            let on = self
                .notify_flags
                .get(&app)
                .is_none_or(|f| f.load(Ordering::Relaxed));
            if !on {
                continue;
            }
            let tx = self.tx.clone();
            let _ = h.view.read(cx).raw().evaluate_script_with_callback(
                "window.__gpuidiumNotify ? window.__gpuidiumNotify.take() : '[]'",
                move |res| {
                    for (id, title, body) in parse_notifications(&res) {
                        let _ = tx.try_send(HostEvent::Notify(app, id, title, body));
                    }
                },
            );
        }
    }

    /// The user clicked a system notification: fire the page's own click handler.
    pub fn click_notification(&self, cx: &App, app: AppId, id: u64) {
        if let Some(h) = self.views.get(&app) {
            let _ = h.view.read(cx).raw().evaluate_script(&format!(
                "window.__gpuidiumNotify && window.__gpuidiumNotify.click({id})"
            ));
        }
    }
}

/// Parse the shim's queue. `evaluate_script` returns the JS string JSON-encoded, so the
/// value is decoded twice. Malformed input yields nothing; texts are clipped again here.
#[cfg(target_os = "macos")]
fn parse_notifications(res: &str) -> Vec<(u64, String, String)> {
    let inner: String = match serde_json::from_str(res) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str(&inner) else {
        return Vec::new();
    };
    let clip = |v: Option<&serde_json::Value>, n: usize| -> String {
        v.and_then(|v| v.as_str())
            .unwrap_or("")
            .chars()
            .take(n)
            .collect()
    };
    items
        .iter()
        .take(50)
        .filter_map(|it| {
            let id = it.get("id")?.as_u64()?;
            Some((id, clip(it.get("title"), 200), clip(it.get("body"), 500)))
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn apply_spellcheck(ctx: &webkit2gtk::WebContext, on: bool, langs: &[String]) {
    use webkit2gtk::WebContextExt as _;
    ctx.set_spell_checking_enabled(on);
    // No choice: the user's locale (WebKitGTK would otherwise keep the previous list).
    let langs: Vec<String> = if langs.is_empty() {
        std::env::var("LANG")
            .ok()
            .and_then(|l| gpuidium_core::spell::locale_language(&l))
            .into_iter()
            .collect()
    } else {
        langs.to_vec()
    };
    if !langs.is_empty() {
        let refs: Vec<&str> = langs.iter().map(String::as_str).collect();
        ctx.set_spell_checking_languages(&refs);
    }
}

/// Decode the page's hex-encoded PNG and accept it only if it really is a small PNG.
#[cfg(target_os = "macos")]
fn favicon_png_from_hex(res: &str) -> Option<Vec<u8>> {
    let hex: Vec<u8> = res.bytes().filter(u8::is_ascii_hexdigit).collect();
    if hex.is_empty() || !hex.len().is_multiple_of(2) || hex.len() > 2 * 256 * 1024 {
        return None;
    }
    let nib = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    let png: Vec<u8> = hex
        .chunks(2)
        .map(|p| Some(nib(p[0])? << 4 | nib(p[1])?))
        .collect::<Option<_>>()?;
    image::load_from_memory_with_format(&png, image::ImageFormat::Png).ok()?;
    Some(png)
}

/// Engine-level mute of one page: WebKitGTK's public switch, WKWebView's page-mute SPI.
fn apply_muted(view: &WebView, muted: bool) {
    #[cfg(target_os = "linux")]
    {
        use webkit2gtk::WebViewExt as _;
        use wry::WebViewExtUnix as _;
        view.raw().webview().set_is_muted(muted);
    }
    #[cfg(target_os = "macos")]
    {
        use wry::WebViewExtMacOS as _;
        crate::mac::set_page_muted(&view.raw().webview(), muted);
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let _ = (view, muted);
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
        let builder =
            bind_profile_store(WebViewBuilder::new_with_web_context(&mut context), profile)
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
        #[cfg(target_os = "macos")]
        let builder = {
            let on = self.notify_flag(app).load(Ordering::Relaxed);
            let shim = include_str!("js/notify_shim.js").replace(
                "__GPUIDIUM_PERMISSION__",
                if on { "granted" } else { "denied" },
            );
            builder.with_initialization_script_for_main_only(shim, true)
        };

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
            if let Some(ctx) = gtk_view.context() {
                apply_spellcheck(&ctx, self.spell_on, &self.spell_langs);
            }
            // Every permission (camera, location, ...) is denied; only notifications may be
            // allowed, and only while this app's notifications are on.
            let flag = self.notify_flag(app);
            gtk_view.connect_permission_request(move |_, req| {
                let is_notification = req.is::<webkit2gtk::NotificationPermissionRequest>();
                log(&format!(
                    "permission request {}: notification={is_notification} app_on={}",
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
            let (flag, t_note) = (self.notify_flag(app), self.tx.clone());
            gtk_view.connect_show_notification(move |_, n| {
                if flag.load(Ordering::Relaxed) {
                    use webkit2gtk::NotificationExt as _;
                    let clip = |s: Option<webkit2gtk::glib::GString>| {
                        s.map(|g| g.chars().take(200).collect::<String>())
                            .unwrap_or_default()
                    };
                    let _ =
                        t_note.try_send(HostEvent::Notify(app, 0, clip(n.title()), clip(n.body())));
                }
                true
            });
        }
        if self.muted {
            apply_muted(&webview, true);
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

    fn set_zoom(&mut self, cx: &mut App, app: AppId, percent: u16) {
        if let Some(h) = self.views.get(&app) {
            h.view.update(cx, |w, _| {
                let _ = w.raw().zoom(f64::from(percent) / 100.0);
            });
        }
    }

    fn close(&mut self, app: AppId) {
        // Dropping the entity and context releases the native view and store.
        self.views.remove(&app);
        self.applied.remove(&app);
    }

    fn events(&self) -> Receiver<HostEvent> {
        self.rx.clone()
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::{favicon_png_from_hex, parse_notifications};

    #[test]
    fn parses_the_shim_queue_and_clips() {
        // As delivered by evaluate_script: the JS string, JSON-encoded once more.
        let queue =
            r#"[{"id":1,"title":"Anna","body":"Hi"},{"id":2,"title":"B"},{"title":"no id"}]"#;
        let res = serde_json::to_string(queue).unwrap();
        assert_eq!(
            parse_notifications(&res),
            vec![
                (1, "Anna".into(), "Hi".into()),
                (2, "B".into(), String::new())
            ]
        );
        let long = format!(r#"[{{"id":3,"title":"{}","body":""}}]"#, "x".repeat(500));
        assert_eq!(
            parse_notifications(&serde_json::to_string(&long).unwrap())[0]
                .1
                .len(),
            200
        );
        assert!(parse_notifications("garbage").is_empty());
        assert!(parse_notifications(r#""{\"not\":\"array\"}""#).is_empty());
    }

    fn tiny_png() -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::RgbaImage::new(2, 2)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn accepts_hex_png_as_returned_by_the_page() {
        let png = tiny_png();
        let hex: String = png.iter().map(|b| format!("{b:02x}")).collect();
        // evaluate_script results arrive JSON-quoted.
        assert_eq!(favicon_png_from_hex(&format!("\"{hex}\"")), Some(png));
    }

    #[test]
    fn rejects_empty_odd_non_png_and_oversized() {
        assert_eq!(favicon_png_from_hex("\"\""), None);
        assert_eq!(favicon_png_from_hex("abc"), None);
        assert_eq!(favicon_png_from_hex("deadbeef"), None);
        assert_eq!(favicon_png_from_hex(&"00".repeat(256 * 1024 + 1)), None);
    }
}
