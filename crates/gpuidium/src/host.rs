//! `WebviewHost`: the boundary between product logic and native WebViews (CORE-004).
//!
//! The domain model never sees Wry or GPUI-WebView types. Native callbacks run on
//! the UI thread and only push `HostEvent`s into a bounded channel; the shell
//! drains it and applies state changes on the GPUI side.

use std::collections::HashMap;
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

#[derive(Debug, Clone)]
pub enum HostEvent {
    LoadStarted(AppId, String),
    LoadFinished(AppId, String),
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
    views: HashMap<AppId, Hosted>,
    tx: SyncSender<HostEvent>,
    rx: Receiver<HostEvent>,
}

impl WryHost {
    pub fn new(data_dir: std::path::PathBuf) -> Self {
        let (tx, rx) = sync_channel(256);
        Self {
            data_dir,
            views: HashMap::new(),
            tx,
            rx,
        }
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

    /// Hide every native view, then show the selected one: no stale view may
    /// ever cover GPUI chrome or the active page.
    fn show_only(&mut self, cx: &mut App, app: Option<AppId>) {
        for (id, h) in &self.views {
            let visible = Some(*id) == app;
            h.view
                .update(cx, |w, _| if visible { w.show() } else { w.hide() });
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
    }

    fn drain_events(&mut self) -> Vec<HostEvent> {
        self.rx.try_iter().collect()
    }
}
