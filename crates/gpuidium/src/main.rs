//! GPUIdium: GPUI shell for web apps.
//!
//! UI contract: a narrow icon sidebar plus the web content. There is no address or button
//! bar. An IRI field appears only in the "open" panel (Ctrl/Cmd+L, or the + icon) and
//! disappears once an address is entered. Panels temporarily replace the native view
//! (it is hidden, never overlaid), see spec section 4.

mod external;
mod host;
mod icon;
#[cfg(target_os = "macos")]
mod mac;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, Disableable as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, Focusable, Image, ImageFormat,
    InteractiveElement, IntoElement, KeyBinding, ParentElement, Render, StatefulInteractiveElement,
    Styled, Window, WindowOptions, actions, div, img, px,
};
use gpuidium_core::{
    AppId, AppState, Command, Event, ProfileId, RemoveMode,
    badge::{self, Badge},
    diagnostics::{log, redact_url},
    exchange,
    instance::InstanceLock,
    lifecycle, paths, profiles,
    storage::{self, Config, Recovery, Settings, WindowConfig},
    zoom::{self, ZoomChange},
};

use host::{HostEvent, Shortcut as Key, WebviewHost, WryHost};

/// Primary shortcut modifier: Cmd on macOS, Ctrl elsewhere.
macro_rules! mod_key {
    ($k:literal) => {
        if cfg!(target_os = "macos") {
            concat!("cmd-", $k)
        } else {
            concat!("ctrl-", $k)
        }
    };
}

const MOD: &str = if cfg!(target_os = "macos") {
    "Cmd"
} else {
    "Ctrl"
};

actions!(
    gpuidium,
    [
        OpenPanel,
        Reload,
        GoBack,
        GoForward,
        Manage,
        Shortcuts,
        Dismiss,
        Select1,
        Select2,
        Select3,
        Select4,
        Select5,
        Select6,
        Select7,
        Select8,
        Select9,
        Quit,
        PageCmdPeriod,
        ZoomIn,
        ZoomOut,
        ZoomReset
    ]
);

/// macOS app and Edit menus. WKWebView only receives Cut/Copy/Paste/Select All
/// through the Edit menu's native selectors (`paste:` ...): without the menu,
/// Cmd+V does nothing in web pages. The shortcuts come from the text input's own
/// bindings, so call this after `gpui_kit::init`. Undo/Redo are left out: GPUI
/// routes those items to its own actions, which would swallow Cmd+Z in pages.
#[cfg(target_os = "macos")]
fn install_menus(cx: &mut App) {
    use gpui_kit::component::input::{Copy as CopyText, Cut, Paste, SelectAll};
    use gpui_kit::{Menu, MenuItem, OsAction};
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        // Text inputs keep their own, more specific cmd-. binding.
        KeyBinding::new("cmd-.", PageCmdPeriod, None),
    ]);
    cx.set_menus(vec![
        Menu::new("GPUIdium").items([MenuItem::action("Quit GPUIdium", Quit)]),
        Menu::new("Edit").items([
            MenuItem::os_action("Cut", Cut, OsAction::Cut),
            MenuItem::os_action("Copy", CopyText, OsAction::Copy),
            MenuItem::os_action("Paste", Paste, OsAction::Paste),
            MenuItem::separator(),
            MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
        ]),
    ]);
}

/// Keyboard reference shown by Ctrl/Cmd+? (single source for the sheet).
fn shortcut_rows() -> Vec<(String, &'static str)> {
    vec![
        (
            format!("{MOD}+L"),
            "Enter an address to open a web app, or find one",
        ),
        (format!("{MOD}+1 … {MOD}+9"), "Switch to the 1st … 9th app"),
        (format!("{MOD}+R"), "Reload the current page"),
        (
            format!("{MOD}+Plus  /  {MOD}+Minus  /  {MOD}+0"),
            "Zoom the page in / out by 10 %, or back to 100 %",
        ),
        ("Alt+←  /  Alt+→".into(), "Back / forward"),
        (format!("{MOD}+,"), "Manage apps, data and settings"),
        (format!("{MOD}+?  /  F1"), "Show this sheet"),
        ("Esc".into(), "Close the open panel and return to the page"),
        ("Drag an icon".into(), "Reorder apps in the sidebar"),
    ]
}

#[derive(Default, Clone)]
struct PageInfo {
    url: String,
    title: String,
    loading: bool,
    /// Engine-reported load failure (CORE-006); cleared when a new load starts.
    error: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Panel {
    None,
    Open,
    Manage,
    Shortcuts,
}

/// Drag payload: index of the dragged sidebar icon.
struct DragApp(usize, String);

struct DragGhost(String);

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(40.))
            .h(px(40.))
            .rounded_full()
            .bg(cx.theme().primary)
            .text_color(cx.theme().primary_foreground)
            .flex()
            .items_center()
            .justify_center()
            .child(self.0.clone())
    }
}

fn initial(name: &str) -> String {
    name.chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .to_string()
}

/// Does typed text look like an address rather than a search for an app name?
fn looks_like_address(t: &str) -> bool {
    let t = t.trim();
    t.contains("://") || t.contains('.') || t.contains(':') || t.starts_with("localhost")
}

struct Shell {
    state: AppState,
    window_cfg: WindowConfig,
    settings: Settings,
    /// Per-app page zoom in percent (absent = 100 %).
    zoom: BTreeMap<AppId, u16>,
    /// Apps whose page notifications are off (default: on).
    notifications_off: BTreeSet<AppId>,
    /// Apps that notified while in the background: a dot until they are opened.
    unseen: HashSet<AppId>,
    /// Last Dock badge set (macOS), to avoid redundant updates.
    dock_label: Option<String>,
    /// Installed spell-check dictionaries (Linux; macOS uses the system's languages).
    spell_dicts: Vec<String>,
    config_dir: PathBuf,
    data_dir: PathBuf,
    host: WryHost,
    add_input: Entity<InputState>,
    pages: HashMap<AppId, PageInfo>,
    last_active: HashMap<AppId, Instant>,
    /// Page favicons (cached on disk by app id); the monogram is only the fallback.
    favicons: HashMap<AppId, Arc<Image>>,
    panel: Panel,
    want_focus: bool,
    /// A panel just opened: take keyboard focus back from the (now hidden) native view.
    want_activate: bool,
    /// Give GPUI's shell keyboard focus (startup, and when a panel closes) so its key
    /// bindings work before the page has been clicked.
    want_shell_focus: bool,
    pending_remove: Option<AppId>,
    editing: Option<AppId>,
    edit_name: Entity<InputState>,
    edit_url: Entity<InputState>,
    orphans: Vec<(ProfileId, u64)>,
    status: String,
    status_seen: String,
    status_since: Instant,
    focus: FocusHandle,
}

impl Shell {
    #[allow(clippy::too_many_arguments)]
    fn new(
        state: AppState,
        window_cfg: WindowConfig,
        settings: Settings,
        zoom: BTreeMap<AppId, u16>,
        notifications_off: BTreeSet<AppId>,
        config_dir: PathBuf,
        data_dir: PathBuf,
        status: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let add_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Web address (https://…) or app name")
        });
        cx.subscribe_in(
            &add_input,
            window,
            |this, _, ev: &InputEvent, window, cx| match ev {
                InputEvent::PressEnter { .. } => this.submit_palette(window, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            },
        )
        .detach();

        let edit_name = cx.new(|cx| InputState::new(window, cx).placeholder("Display name"));
        let edit_url = cx.new(|cx| InputState::new(window, cx).placeholder("https://…"));
        for input in [&edit_name, &edit_url] {
            cx.subscribe_in(input, window, |this, _, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::PressEnter { .. }) {
                    this.save_edit(window, cx);
                }
            })
            .detach();
        }

        let mut host = WryHost::new(data_dir.clone());
        for &app in &notifications_off {
            host.set_app_notifications(cx, app, false);
        }
        host.set_spellcheck(cx, settings.spellcheck, &settings.spell_languages);

        // macOS: a click on a system notification switches to its app and runs the page's
        // own click handler (e.g. opens the chat).
        #[cfg(target_os = "macos")]
        {
            let (tx, clicks) = async_channel::unbounded();
            mac::init_notifications(tx);
            cx.spawn_in(window, async move |this, cx| {
                while let Ok((app, note)) = clicks.recv().await {
                    let r = this.update_in(cx, |this, window, cx| {
                        this.open_from_notification(app, note, window, cx)
                    });
                    if r.is_err() {
                        break;
                    }
                }
            })
            .detach();
        }

        // Native events (shortcuts, loads, titles, favicons) wake the UI the moment they are
        // sent; no polling, so a shortcut is handled right after the GTK callback returns.
        let events = host.events();
        cx.spawn_in(window, async move |this, cx| {
            while let Ok(first) = events.recv().await {
                let mut batch = vec![first];
                while let Ok(more) = events.try_recv() {
                    batch.push(more);
                }
                if this
                    .update_in(cx, |this, window, cx| {
                        this.handle_host_events(batch, window, cx)
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        // Slow housekeeping once a second: expire the transient status line, unload idle apps.
        cx.spawn_in(window, async move |this, cx| {
            let mut ticks: u32 = 0;
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                ticks = ticks.wrapping_add(1);
                if this
                    .update_in(cx, |this, _window, cx| this.tick(ticks, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let first_run = state.apps().is_empty();
        let mut shell = Self {
            state,
            window_cfg,
            settings,
            zoom,
            notifications_off,
            unseen: HashSet::new(),
            dock_label: None,
            spell_dicts: if cfg!(target_os = "linux") {
                gpuidium_core::spell::installed_dictionaries(
                    &gpuidium_core::spell::dictionary_dirs(),
                )
            } else {
                Vec::new()
            },
            config_dir,
            data_dir,
            host,
            add_input,
            pages: HashMap::new(),
            last_active: HashMap::new(),
            favicons: HashMap::new(),
            panel: if first_run { Panel::Open } else { Panel::None },
            want_focus: first_run,
            want_activate: false,
            want_shell_focus: true,
            pending_remove: None,
            editing: None,
            edit_name,
            edit_url,
            orphans: Vec::new(),
            status_seen: status.clone(),
            status,
            status_since: Instant::now(),
            focus: cx.focus_handle(),
        };
        shell.load_cached_favicons();
        log("shell constructed");
        // Tested and rejected: deferring this until after the first paint shows the sidebar
        // ~130 ms sooner but delays the page itself by ~75 ms.
        shell.activate_current(window, cx);
        log("first view created");
        shell
    }

    /// Show cached favicons immediately at startup, before any page has loaded.
    fn load_cached_favicons(&mut self) {
        for app in self.state.apps() {
            let file = paths::favicon_file(&self.data_dir, app.id);
            if let Ok(bytes) = std::fs::read(file)
                && bytes.len() <= 256 * 1024
            {
                self.favicons
                    .insert(app.id, Arc::new(Image::from_bytes(ImageFormat::Png, bytes)));
            }
        }
    }

    fn store_favicon(&mut self, id: AppId, png: Vec<u8>) {
        if self.state.get(id).is_none() {
            return;
        }
        let file = paths::favicon_file(&self.data_dir, id);
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&file, &png);
        self.favicons
            .insert(id, Arc::new(Image::from_bytes(ImageFormat::Png, png)));
    }

    fn save(&mut self) {
        let mut cfg = Config::from_state(&self.state, self.window_cfg);
        cfg.settings = self.settings.clone();
        cfg.notifications_off = self
            .notifications_off
            .iter()
            .filter(|id| self.state.get(**id).is_some())
            .copied()
            .collect();
        // Only apps that still exist, and only non-default levels.
        cfg.zoom = self
            .zoom
            .iter()
            .filter(|(id, z)| **z != zoom::DEFAULT && self.state.get(**id).is_some())
            .map(|(id, z)| (*id, zoom::sanitize(*z)))
            .collect();
        if let Err(e) = storage::save(&self.config_dir, &cfg) {
            self.status = format!("Could not save config: {e}");
        }
    }

    fn active_error(&self) -> Option<&str> {
        self.state
            .active()
            .and_then(|id| self.pages.get(&id))
            .and_then(|p| p.error.as_deref())
    }

    /// The native view is only visible when nothing else needs the content area.
    fn sync_views(&mut self, cx: &mut Context<Self>) {
        let visible = self.panel == Panel::None && self.active_error().is_none();
        let target = if visible { self.state.active() } else { None };
        self.host.show_only(cx, target);
    }

    /// Ensure the active app has a native view, and show only that one.
    fn activate_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.state.active() {
            self.last_active.insert(id, Instant::now());
            // Opening an app counts as having seen its notifications.
            if self.unseen.remove(&id) {
                self.update_dock_badge();
            }
            if !self.host.contains(id)
                && let Some(app) = self.state.get(id).cloned()
            {
                match self
                    .host
                    .create(window, cx, id, app.profile_id, &app.start_url)
                {
                    Ok(()) => {
                        if let Some(&z) = self.zoom.get(&id) {
                            self.host.set_zoom(cx, id, zoom::sanitize(z));
                        }
                        self.pages.insert(
                            id,
                            PageInfo {
                                url: app.start_url.clone(),
                                loading: true,
                                ..Default::default()
                            },
                        );
                    }
                    Err(e) => self.status = format!("Cannot open {}: {e}", app.name),
                }
            }
        }
        self.sync_views(cx);
        cx.notify();
    }

    fn set_panel(&mut self, panel: Panel, cx: &mut Context<Self>) {
        self.panel = panel;
        self.want_activate = panel != Panel::None;
        self.want_shell_focus = panel == Panel::None;
        self.pending_remove = None;
        self.editing = None;
        match panel {
            Panel::Open => {
                self.want_focus = true;
            }
            Panel::Manage => self.refresh_orphans(),
            _ => {}
        }
        self.sync_views(cx);
        cx.notify();
    }

    fn open_panel(&mut self, cx: &mut Context<Self>) {
        // Re-opening an empty palette is cheap; keep any text the user already typed.
        self.set_panel(Panel::Open, cx);
    }

    /// Esc: close the panel. With no apps the open panel is the only thing to show.
    fn dismiss(&mut self, cx: &mut Context<Self>) {
        if self.panel != Panel::None && !(self.panel == Panel::Open && self.state.apps().is_empty())
        {
            self.set_panel(Panel::None, cx);
        }
    }

    fn tick(&mut self, ticks: u32, cx: &mut Context<Self>) {
        // macOS: collect page notifications queued by the in-page shim.
        #[cfg(target_os = "macos")]
        self.host.poll_notifications(cx);
        // Status line is transient: it vanishes five seconds after it last changed.
        if self.status != self.status_seen {
            self.status_seen = self.status.clone();
            self.status_since = Instant::now();
            cx.notify();
        } else if !self.status.is_empty() && self.status_since.elapsed() > Duration::from_secs(5) {
            self.status.clear();
            self.status_seen.clear();
            cx.notify();
        }
        // LIFE-010: opt-in unloading of long-inactive apps (checked every 5 s).
        // `GPUIDIUM_UNLOAD_SECS` overrides the limit in seconds; it exists only so the
        // behaviour can be tested without waiting minutes.
        if ticks.is_multiple_of(5) {
            let test_secs = std::env::var("GPUIDIUM_UNLOAD_SECS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok());
            let limit = test_secs.map(Duration::from_secs).or_else(|| {
                self.settings
                    .unload_inactive_minutes
                    .map(|m| Duration::from_secs(u64::from(m) * 60))
            });
            if let Some(limit) = limit {
                let idle = self
                    .host
                    .ids()
                    .into_iter()
                    .map(|id| (id, self.last_active.get(&id).map(|t| t.elapsed())));
                for id in lifecycle::due_for_unload(idle, self.state.active(), limit) {
                    self.host.close(id);
                    self.pages.remove(&id);
                    log(&format!("unloaded inactive app {id}"));
                }
            }
        }
    }

    fn handle_host_events(
        &mut self,
        events: Vec<HostEvent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for ev in events {
            match ev {
                HostEvent::Shortcut(sc) => match sc {
                    Key::Open => self.open_panel(cx),
                    Key::Reload => self.reload_active(cx),
                    Key::Zoom(change) => self.zoom_active(change, cx),
                    Key::Manage => self.set_panel(Panel::Manage, cx),
                    Key::Keys => self.set_panel(Panel::Shortcuts, cx),
                    Key::Select(n) => self.select_nth(n, window, cx),
                    Key::Back => {
                        if let Some(id) = self.state.active() {
                            self.host.back(cx, id);
                        }
                    }
                    Key::Forward => {
                        if let Some(id) = self.state.active() {
                            self.host.forward(cx, id);
                        }
                    }
                },
                HostEvent::Favicon(id, png) => self.store_favicon(id, png),
                HostEvent::LoadStarted(id, url) => {
                    let p = self.pages.entry(id).or_default();
                    p.url = url;
                    p.loading = true;
                    p.error = None;
                }
                HostEvent::LoadFinished(id, url) => {
                    log("page load finished");
                    let p = self.pages.entry(id).or_default();
                    p.url = url;
                    p.loading = false;
                    #[cfg(target_os = "macos")]
                    self.host.refresh_favicon(cx, id);
                }
                HostEvent::LoadFailed(id, url, msg) => {
                    // Policy-blocked or superseded navigations surface as "cancelled".
                    if !msg.to_lowercase().contains("cancel") {
                        let p = self.pages.entry(id).or_default();
                        p.loading = false;
                        p.error = Some(msg);
                        self.want_activate = true;
                        log(&format!("load failed: {}", redact_url(&url)));
                    }
                }
                HostEvent::Title(id, t) => {
                    // Single-page apps (e.g. an unread count) change the icon with the title.
                    #[cfg(target_os = "macos")]
                    self.host.refresh_favicon(cx, id);
                    self.pages.entry(id).or_default().title = t;
                }
                HostEvent::Blocked(u) => self.status = format!("Blocked navigation to {u}"),
                HostEvent::OpenedExternally(u) => {
                    self.status = format!("Opened in system browser: {u}")
                }
                HostEvent::Notify(id, note, title, body) => {
                    if self.notifications_off.contains(&id) {
                        continue;
                    }
                    let name = self
                        .state
                        .get(id)
                        .map(|a| a.name.clone())
                        .unwrap_or_default();
                    #[cfg(target_os = "macos")]
                    let shown = mac::notify(id, &name, note, &title, &body);
                    #[cfg(not(target_os = "macos"))]
                    let shown = {
                        let _ = note;
                        external::notify(&name, &title, &body)
                    };
                    if !shown {
                        self.status = "Notification could not be shown".into();
                    }
                    // A dot until the app is opened, unless it is what the user looks at.
                    let in_view = self.state.active() == Some(id)
                        && self.panel == Panel::None
                        && window.is_window_active();
                    if !in_view {
                        self.unseen.insert(id);
                    }
                }
            }
        }
        self.update_dock_badge();
        self.sync_views(cx);
        cx.notify();
    }

    fn apply(&mut self, cmd: Command, window: &mut Window, cx: &mut Context<Self>) {
        match self.state.apply(cmd) {
            Ok(events) => {
                for ev in events {
                    if let Event::Removed {
                        id,
                        profile_id,
                        mode,
                    } = ev
                    {
                        self.host.close(id);
                        self.pages.remove(&id);
                        self.last_active.remove(&id);
                        self.favicons.remove(&id);
                        let _ = std::fs::remove_file(paths::favicon_file(&self.data_dir, id));
                        if mode == RemoveMode::DeleteData {
                            // The native view is closed above; only this profile is touched.
                            self.status = match host::delete_profile(&self.data_dir, profile_id) {
                                Ok(()) => "App removed and its browsing data deleted".into(),
                                Err(e) => format!("App removed, but deleting its data failed: {e}"),
                            };
                            self.recheck_deleted_profile(profile_id, cx);
                        } else {
                            self.status = "App removed; its browsing data was kept on disk".into();
                        }
                    }
                }
                self.save();
                self.activate_current(window, cx);
                if self.state.apps().is_empty() {
                    self.set_panel(Panel::Open, cx);
                } else if self.panel == Panel::Manage {
                    self.refresh_orphans();
                }
            }
            Err(e) => self.status = e.to_string(),
        }
        cx.notify();
    }

    /// Enter in the open panel: add an address, or switch to the best matching app.
    fn submit_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.add_input.read(cx).value().to_string();
        let text = input.trim();
        if text.is_empty() {
            return;
        }
        if !looks_like_address(text)
            && let Some(id) = self.matches(text).first().map(|a| a.id)
        {
            self.choose(id, window, cx);
            return;
        }
        let before = self.state.apps().len();
        self.apply(
            Command::Add {
                name: String::new(),
                input,
            },
            window,
            cx,
        );
        if self.state.apps().len() > before {
            if let Some(last) = self.state.apps().last().map(|a| a.id) {
                self.apply(Command::Select(last), window, cx);
            }
            self.add_input
                .update(cx, |s, cx| s.set_value("", window, cx));
            self.status.clear();
            self.set_panel(Panel::None, cx);
        }
    }

    fn matches(&self, text: &str) -> Vec<&gpuidium_core::AppEntry> {
        let t = text.to_lowercase();
        self.state
            .apps()
            .iter()
            .filter(|a| {
                t.is_empty()
                    || a.name.to_lowercase().contains(&t)
                    || a.start_url.to_lowercase().contains(&t)
            })
            .collect()
    }

    /// Select an app and return to the page.
    fn choose(&mut self, id: AppId, window: &mut Window, cx: &mut Context<Self>) {
        self.panel = Panel::None;
        self.pending_remove = None;
        self.editing = None;
        self.apply(Command::Select(id), window, cx);
        self.add_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        self.sync_views(cx);
    }

    fn select_nth(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.state.apps().get(n).map(|a| a.id) {
            self.choose(id, window, cx);
        }
    }

    /// UX-010: drop icon `from` onto position `to`, using the stable move commands.
    fn reorder(&mut self, from: usize, to: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.state.apps().get(from).map(|a| a.id) else {
            return;
        };
        for _ in 0..from.abs_diff(to) {
            let cmd = if to > from {
                Command::MoveDown(id)
            } else {
                Command::MoveUp(id)
            };
            self.apply(cmd, window, cx);
        }
    }

    fn reload_active(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.state.active() {
            if let Some(p) = self.pages.get_mut(&id) {
                p.error = None;
            }
            self.host.reload(cx, id);
            self.sync_views(cx);
        }
    }

    /// Ctrl/Cmd + `+` / `-` / `0`: change the active app's zoom in 10 % steps (persisted).
    fn zoom_active(&mut self, change: ZoomChange, cx: &mut Context<Self>) {
        let Some(id) = self.state.active() else {
            return;
        };
        let current = self.zoom.get(&id).copied().unwrap_or(zoom::DEFAULT);
        let next = zoom::apply(current, change);
        self.zoom.insert(id, next);
        self.host.set_zoom(cx, id, next);
        self.status = format!("Zoom {next} %");
        self.save();
        cx.notify();
    }

    fn retry_active(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.state.active() else {
            return;
        };
        let url = self
            .pages
            .get(&id)
            .map(|p| p.url.clone())
            .filter(|u| !u.is_empty())
            .or_else(|| self.state.get(id).map(|a| a.start_url.clone()));
        if let Some(p) = self.pages.get_mut(&id) {
            p.error = None;
            p.loading = true;
        }
        if let Some(url) = url
            && let Err(e) = self.host.navigate(cx, id, &url)
        {
            self.status = e.to_string();
        }
        self.sync_views(cx);
        cx.notify();
    }

    // --- manage panel helpers -------------------------------------------------------

    fn refresh_orphans(&mut self) {
        let known: Vec<_> = self.state.apps().iter().map(|a| a.profile_id).collect();
        self.orphans = profiles::orphans(&self.data_dir, &known)
            .unwrap_or_default()
            .into_iter()
            .map(|id| (id, profiles::disk_usage(&self.data_dir, id).unwrap_or(0)))
            .collect();
    }

    /// WebKit's helper processes can still flush files for a moment after the view closes
    /// and recreate part of the profile directory. Delete again after a short delay (and
    /// once more later), so "delete data" really ends with nothing left behind.
    fn recheck_deleted_profile(&self, id: ProfileId, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            for secs in [2u64, 6] {
                cx.background_executor()
                    .timer(Duration::from_secs(secs))
                    .await;
                let ok = this
                    .update(cx, |this, cx| {
                        // Ids are never reused, but never touch a profile an app owns.
                        if this.state.apps().iter().any(|a| a.profile_id == id) {
                            return true;
                        }
                        let res = host::delete_profile(&this.data_dir, id);
                        if let Err(e) = &res {
                            this.status = format!("Deleting leftover data failed: {e}");
                            cx.notify();
                        }
                        res.is_ok()
                    })
                    .unwrap_or(true);
                if !ok {
                    break;
                }
            }
        })
        .detach();
    }

    /// Delete one retained profile. Only ids not used by any app are accepted.
    fn purge_orphan(&mut self, id: ProfileId, cx: &mut Context<Self>) {
        if self.state.apps().iter().any(|a| a.profile_id == id) {
            return;
        }
        self.status = match host::delete_profile(&self.data_dir, id) {
            Ok(()) => "Deleted retained browsing data".into(),
            Err(e) => format!("Could not delete retained data: {e}"),
        };
        self.refresh_orphans();
        cx.notify();
    }

    /// Delete every retained (orphaned) profile. Apps' own profiles are never in the list.
    fn purge_all_orphans(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<ProfileId> = self.orphans.iter().map(|(id, _)| *id).collect();
        let mut failed = 0;
        for id in &ids {
            if self.state.apps().iter().any(|a| a.profile_id == *id)
                || host::delete_profile(&self.data_dir, *id).is_err()
            {
                failed += 1;
            }
        }
        self.status = if failed == 0 {
            format!("Deleted {} retained profile(s)", ids.len())
        } else {
            format!(
                "Deleted {} retained profile(s), {failed} failed",
                ids.len() - failed
            )
        };
        self.refresh_orphans();
        cx.notify();
    }

    fn export_path(&self) -> PathBuf {
        self.config_dir.join("apps-export.json")
    }

    fn import_path(&self) -> PathBuf {
        self.config_dir.join("apps-import.json")
    }

    fn export_apps(&mut self, cx: &mut Context<Self>) {
        let json = exchange::export(self.state.apps());
        self.status = match std::fs::write(self.export_path(), json) {
            Ok(()) => format!(
                "Exported app list (no sessions) to {}",
                self.export_path().display()
            ),
            Err(e) => format!("Export failed: {e}"),
        };
        cx.notify();
    }

    /// Import adds fresh apps, each with a brand-new profile; nothing is attached.
    fn import_apps(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let bytes = match std::fs::read(self.import_path()) {
            Ok(b) => b,
            Err(e) => {
                self.status = format!("Place the file at {}: {e}", self.import_path().display());
                cx.notify();
                return;
            }
        };
        match exchange::import(&bytes) {
            Ok((items, skipped)) => {
                let n = items.len();
                for it in items {
                    self.apply(
                        Command::Add {
                            name: it.name,
                            input: it.url,
                        },
                        window,
                        cx,
                    );
                }
                self.status = format!("Imported {n} app(s), skipped {skipped}");
            }
            Err(e) => self.status = format!("Import failed: {e}"),
        }
        cx.notify();
    }

    fn begin_edit(&mut self, id: AppId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(app) = self.state.get(id).cloned() else {
            return;
        };
        self.pending_remove = None;
        self.editing = Some(id);
        self.edit_name
            .update(cx, |s, cx| s.set_value(app.name, window, cx));
        self.edit_url
            .update(cx, |s, cx| s.set_value(app.start_url, window, cx));
        cx.notify();
    }

    /// Apply the edit form. The URL is re-validated by the core; the profile never changes.
    fn save_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.editing else { return };
        let name = self.edit_name.read(cx).value().to_string();
        let input = self.edit_url.read(cx).value().to_string();
        let Some(old) = self.state.get(id).cloned() else {
            self.editing = None;
            return;
        };
        if name.trim() != old.name {
            self.apply(Command::Rename { id, name }, window, cx);
            if self.state.get(id).is_some_and(|a| a.name == old.name) {
                return; // rejected: keep the form open, status explains why
            }
        }
        if input.trim() != old.start_url {
            self.apply(Command::SetUrl { id, input }, window, cx);
            match self.state.get(id).map(|a| a.start_url.clone()) {
                Some(u) if u != old.start_url => {
                    if let Err(e) = self.host.navigate(cx, id, &u) {
                        self.status = e.to_string();
                    }
                }
                _ => return, // rejected: keep the form open, status explains why
            }
        }
        self.editing = None;
        self.status.clear();
        cx.notify();
    }

    fn cycle_unload(&mut self, cx: &mut Context<Self>) {
        self.settings.unload_inactive_minutes = match self.settings.unload_inactive_minutes {
            None => Some(10),
            Some(10) => Some(30),
            Some(30) => Some(60),
            _ => None,
        };
        self.save();
        cx.notify();
    }

    fn toggle_app_notifications(&mut self, id: AppId, cx: &mut Context<Self>) {
        let on = self.notifications_off.remove(&id);
        if !on {
            self.notifications_off.insert(id);
            self.unseen.remove(&id);
        }
        self.host.set_app_notifications(cx, id, on);
        self.save();
        self.update_dock_badge();
        cx.notify();
    }

    fn toggle_spellcheck(&mut self, cx: &mut Context<Self>) {
        self.settings.spellcheck = !self.settings.spellcheck;
        self.host
            .set_spellcheck(cx, self.settings.spellcheck, &self.settings.spell_languages);
        self.save();
        cx.notify();
    }

    /// Linux: add or remove one dictionary; all chosen ones are used at the same time.
    fn toggle_spell_language(&mut self, lang: String, cx: &mut Context<Self>) {
        let langs = &mut self.settings.spell_languages;
        match langs.iter().position(|l| *l == lang) {
            Some(i) => {
                langs.remove(i);
            }
            None => langs.push(lang),
        }
        langs.sort();
        self.host
            .set_spellcheck(cx, self.settings.spellcheck, &self.settings.spell_languages);
        self.save();
        cx.notify();
    }

    /// What the sidebar shows on an app's icon.
    fn badge_for(&self, id: AppId) -> Badge {
        let title = self
            .pages
            .get(&id)
            .map(|p| badge::from_title(&p.title))
            .unwrap_or_default();
        badge::combine(title, self.unseen.contains(&id))
    }

    /// macOS: total unread on the Dock icon. Only changes reach AppKit.
    fn update_dock_badge(&mut self) {
        let label = badge::total_label(self.state.apps().iter().map(|a| self.badge_for(a.id)));
        if label != self.dock_label {
            #[cfg(target_os = "macos")]
            mac::set_dock_badge(label.as_deref());
            self.dock_label = label;
        }
    }

    /// A click on a system notification: bring the window forward, show the app, and let
    /// the page handle the click.
    #[cfg(target_os = "macos")]
    fn open_from_notification(
        &mut self,
        app: AppId,
        note: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.get(app).is_none() {
            return;
        }
        cx.activate(true);
        window.activate_window();
        self.choose(app, window, cx);
        self.host.click_notification(cx, app, note);
    }

    /// Spell-check languages. Linux: every installed dictionary as a toggle, all chosen ones
    /// at once. macOS: the system spell checker picks the language per word among the
    /// languages set in System Settings.
    fn spell_languages_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let note = |t: String| {
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(t)
        };
        if cfg!(target_os = "macos") {
            return div()
                .flex()
                .items_center()
                .gap_2()
                .child(Button::new("spell-langs").label("Spelling languages…").ghost().on_click(
                    cx.listener(|_, _, _, _| {
                        #[cfg(target_os = "macos")]
                        mac::open_spelling_settings();
                    }),
                ))
                .child(note(
                    "macOS checks each word against all spelling languages set in System Settings › Keyboard › Text Input."
                        .into(),
                ))
                .into_any_element();
        }
        if self.spell_dicts.is_empty() {
            return note(
                "No hunspell dictionaries found; install e.g. hunspell-en-us and hunspell-de-de."
                    .into(),
            )
            .into_any_element();
        }
        let chosen = &self.settings.spell_languages;
        let mut row =
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_1()
                .child(note(if chosen.is_empty() {
                    "Languages (none chosen: system language):".into()
                } else {
                    "Languages:".into()
                }));
        for (i, lang) in self.spell_dicts.iter().enumerate() {
            let on = chosen.contains(lang);
            let l = lang.clone();
            let b = Button::new(("lang", i)).label(lang.clone()).on_click(
                cx.listener(move |this, _, _, cx| this.toggle_spell_language(l.clone(), cx)),
            );
            row = row.child(if on { b.primary() } else { b.ghost() });
        }
        row.into_any_element()
    }

    fn current_origin(&self) -> String {
        let url = self
            .state
            .active()
            .and_then(|id| self.pages.get(&id))
            .map(|p| p.url.as_str());
        match url.and_then(|u| url::Url::parse(u).ok()) {
            Some(u) => u.origin().ascii_serialization(),
            None => String::new(),
        }
    }

    // --- views ------------------------------------------------------------------------

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // Tiny tiles: a page favicon (16 px) in a 28 px tile, so the rail stays ~40 px wide.
        let mut icons = div()
            .id("icons")
            .flex()
            .flex_col()
            .items_center()
            .gap(px(6.))
            .flex_1()
            .overflow_y_scroll();
        for (i, app) in self.state.apps().iter().enumerate() {
            let id = app.id;
            let selected = self.state.active() == Some(id) && self.panel == Panel::None;
            let letter = initial(&app.name);
            let failed = self.pages.get(&id).is_some_and(|p| p.error.is_some());
            let face: AnyElement = match self.favicons.get(&id) {
                Some(fav) => img(fav.clone()).size(px(16.)).into_any_element(),
                // Fallback only: monogram until the page provides a favicon.
                None => div()
                    .size(px(20.))
                    .rounded_full()
                    .bg(cx.theme().primary)
                    .text_color(cx.theme().primary_foreground)
                    .text_xs()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(letter.clone())
                    .into_any_element(),
            };
            let label = match self
                .pages
                .get(&id)
                .map(|p| p.title.clone())
                .filter(|t| !t.is_empty())
            {
                Some(t) => format!("{} - {t}", app.name),
                None => app.name.clone(),
            };
            // The badge sits inside the 28 px tile, over the favicon's top-right corner: the
            // scrolling icon column clips anything that sticks out of the tile.
            let badge = match self.badge_for(id) {
                Badge::None => None,
                Badge::Dot => Some(
                    div()
                        .absolute()
                        .top(px(3.))
                        .right(px(3.))
                        .size(px(7.))
                        .rounded_full()
                        .bg(cx.theme().danger)
                        .into_any_element(),
                ),
                b @ Badge::Count(_) => Some(
                    div()
                        .absolute()
                        .top(px(1.))
                        .right(px(1.))
                        .h(px(11.))
                        .min_w(px(11.))
                        .px(px(2.))
                        .rounded_full()
                        .bg(cx.theme().danger)
                        .text_color(cx.theme().danger_foreground)
                        .text_size(px(8.))
                        .line_height(px(11.))
                        .font_weight(gpui_kit::FontWeight::BOLD)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(b.label().unwrap_or_default())
                        .into_any_element(),
                ),
            };
            let icon = div()
                .id(("app", i))
                .relative()
                .size(px(28.))
                .flex_none()
                .rounded_md()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .border_1()
                .border_color(if failed {
                    cx.theme().danger
                } else if selected {
                    cx.theme().ring
                } else {
                    cx.theme().sidebar
                })
                .bg(if selected {
                    cx.theme().secondary
                } else {
                    cx.theme().sidebar
                })
                .hover(|s| s.bg(cx.theme().secondary))
                .on_click(cx.listener(move |this, _, window, cx| this.choose(id, window, cx)))
                .on_drag(DragApp(i, letter.clone()), |d, _, _, cx| {
                    cx.new(|_| DragGhost(d.1.clone()))
                })
                .on_drop(cx.listener(move |this, d: &DragApp, window, cx| {
                    this.reorder(d.0, i, window, cx)
                }))
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(label.clone()).build(window, cx)
                })
                .child(face)
                .children(badge);
            icons = icons.child(icon);
        }
        let small = |id: &'static str, glyph: &'static str| {
            div()
                .id(id)
                .size(px(28.))
                .flex_none()
                .rounded_md()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_sm()
                .hover(|s| s.bg(cx.theme().secondary))
                .child(glyph)
        };
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(6.))
            .py_2()
            .w(px(40.))
            .h_full()
            .flex_none()
            .bg(cx.theme().sidebar)
            .border_r_1()
            .border_color(cx.theme().border)
            .child(icons)
            .child(div().w(px(24.)).h(px(1.)).bg(cx.theme().border))
            .child(small("open", "+").on_click(cx.listener(|this, _, _, cx| this.open_panel(cx))))
            .child(
                small("manage", "⚙")
                    .on_click(cx.listener(|this, _, _, cx| this.set_panel(Panel::Manage, cx))),
            )
            .child(
                small("keys", "?")
                    .on_click(cx.listener(|this, _, _, cx| this.set_panel(Panel::Shortcuts, cx))),
            )
    }

    /// The IRI field: visible only here. Typing also filters existing apps.
    fn open_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let text = self.add_input.read(cx).value().to_string();
        let mut col = div()
            .w(px(560.))
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_xl().child("Open a web app"))
            .child(Input::new(&self.add_input))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("Enter a web address (https://… or a bare host) to add it, or type a name to switch. Esc cancels."),
            );
        let rows: Vec<(AppId, String, String)> = self
            .matches(&text)
            .into_iter()
            .map(|a| (a.id, a.name.clone(), a.start_url.clone()))
            .collect();
        for (i, (id, name, url)) in rows.into_iter().enumerate() {
            col = col.child(
                div()
                    .id(("pal", i))
                    .flex()
                    .items_center()
                    .gap_2()
                    .p_2()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|s| s.bg(cx.theme().secondary))
                    .on_click(cx.listener(move |this, _, w, cx| this.choose(id, w, cx)))
                    .child(
                        div()
                            .w(px(28.))
                            .h(px(28.))
                            .rounded_full()
                            .bg(cx.theme().primary)
                            .text_color(cx.theme().primary_foreground)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(initial(&name)),
                    )
                    .child(div().flex_1().child(name))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(url),
                    ),
            );
        }
        div()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .pt(px(72.))
            .child(col)
            .into_any_element()
    }

    fn shortcuts_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut col = div()
            .w(px(560.))
            .flex()
            .flex_col()
            .gap_2()
            .child(div().text_xl().child("Keyboard shortcuts"));
        for (keys, what) in shortcut_rows() {
            col = col.child(
                div()
                    .flex()
                    .gap_3()
                    .py_1()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(div().w(px(190.)).flex_none().child(keys))
                    .child(div().flex_1().child(what)),
            );
        }
        col = col.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child("Press Esc to close."),
        );
        div()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .pt(px(72.))
            .child(col)
            .into_any_element()
    }

    fn error_view(&self, msg: &str, cx: &mut Context<Self>) -> AnyElement {
        let url = self
            .state
            .active()
            .and_then(|id| self.pages.get(&id))
            .map(|p| p.url.clone())
            .unwrap_or_default();
        div()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .child(div().text_xl().child("This page could not be loaded"))
            .child(div().child(self.current_origin()))
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(msg.to_owned()),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("retry")
                            .label("Retry")
                            .primary()
                            .on_click(cx.listener(|this, _, _, cx| this.retry_active(cx))),
                    )
                    .child(
                        Button::new("ext")
                            .label("Open in browser")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !external::open(&url) {
                                    this.status = "Cannot open this address externally".into();
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("err-manage")
                            .label("Edit address…")
                            .ghost()
                            .on_click(cx.listener(|this, _, w, cx| {
                                this.set_panel(Panel::Manage, cx);
                                if let Some(id) = this.state.active() {
                                    this.begin_edit(id, w, cx);
                                }
                            })),
                    ),
            )
            .into_any_element()
    }

    fn manage_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let count = self.state.apps().len();
        let mut col = div()
            .w(px(640.))
            .flex()
            .flex_col()
            .gap_2()
            .child(div().text_xl().child("Apps"));
        for (i, app) in self.state.apps().iter().enumerate() {
            let id = app.id;
            let confirming = self.pending_remove == Some(id);
            let editing = self.editing == Some(id);
            let mut row = div()
                .flex()
                .flex_col()
                .gap_1()
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(cx.theme().border)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .w(px(28.))
                                .h(px(28.))
                                .rounded_full()
                                .bg(cx.theme().primary)
                                .text_color(cx.theme().primary_foreground)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(initial(&app.name)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .child(app.name.clone())
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(app.start_url.clone()),
                                ),
                        ),
                );
            if editing {
                row =
                    row.child(Input::new(&self.edit_name))
                        .child(Input::new(&self.edit_url))
                        .child(
                            div()
                                .flex()
                                .gap_1()
                                .child(
                                    Button::new(("save", i)).label("Save").primary().on_click(
                                        cx.listener(|this, _, w, cx| this.save_edit(w, cx)),
                                    ),
                                )
                                .child(
                                    Button::new(("ecancel", i))
                                        .label("Cancel")
                                        .ghost()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.editing = None;
                                            cx.notify();
                                        })),
                                ),
                        );
            } else if confirming {
                row =
                    row.child(div().text_xs().child("Remove this app?")).child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_1()
                            .child(Button::new(("keep", i)).label("Keep data").on_click(
                                cx.listener(move |this, _, w, cx| {
                                    this.pending_remove = None;
                                    this.apply(
                                        Command::Remove {
                                            id,
                                            mode: RemoveMode::RetainData,
                                        },
                                        w,
                                        cx,
                                    );
                                }),
                            ))
                            .child(
                                Button::new(("del", i))
                                    .label("Delete data")
                                    .primary()
                                    .on_click(cx.listener(move |this, _, w, cx| {
                                        this.pending_remove = None;
                                        this.apply(
                                            Command::Remove {
                                                id,
                                                mode: RemoveMode::DeleteData,
                                            },
                                            w,
                                            cx,
                                        );
                                    })),
                            )
                            .child(Button::new(("cancel", i)).label("Cancel").ghost().on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.pending_remove = None;
                                    cx.notify();
                                }),
                            )),
                    );
            } else {
                row = row.child(
                    div()
                        .flex()
                        .gap_1()
                        .child(
                            Button::new(("up", i))
                                .label("↑")
                                .ghost()
                                .disabled(i == 0)
                                .on_click(cx.listener(move |this, _, w, cx| {
                                    this.apply(Command::MoveUp(id), w, cx)
                                })),
                        )
                        .child(
                            Button::new(("down", i))
                                .label("↓")
                                .ghost()
                                .disabled(i + 1 == count)
                                .on_click(cx.listener(move |this, _, w, cx| {
                                    this.apply(Command::MoveDown(id), w, cx)
                                })),
                        )
                        .child(
                            Button::new(("notif", i))
                                .label(if self.notifications_off.contains(&id) {
                                    "Notifications: Off"
                                } else {
                                    "Notifications: On"
                                })
                                .ghost()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_app_notifications(id, cx)
                                })),
                        )
                        .child(Button::new(("edit", i)).label("Edit").ghost().on_click(
                            cx.listener(move |this, _, w, cx| this.begin_edit(id, w, cx)),
                        ))
                        .child(Button::new(("rm", i)).label("Remove").ghost().on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.pending_remove = Some(id);
                                cx.notify();
                            }),
                        )),
                );
            }
            col = col.child(row);
        }

        // Settings (all opt-in, off by default).
        let unload = match self.settings.unload_inactive_minutes {
            None => "Off".to_string(),
            Some(m) => format!("after {m} min"),
        };
        col = col
            .child(div().text_xl().mt_4().child("Settings"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(Button::new("unload").label(format!("Unload inactive apps: {unload}")).on_click(
                        cx.listener(|this, _, _, cx| this.cycle_unload(cx)),
                    ))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("Frees memory; logins in persistent cookies are kept, page state is lost."),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Button::new("spell")
                            .label(format!(
                                "Spell checking: {}",
                                if self.settings.spellcheck { "On" } else { "Off" }
                            ))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_spellcheck(cx))),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("Underlines misspelled words in page text fields; right-click for suggestions."),
                    ),
            )
            .child(self.spell_languages_row(cx))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("Page notifications are switched per app (Notifications button next to each app)."),
            );

        // Retained data and list exchange.
        col = col.child(div().text_xl().mt_4().child("Saved data of removed apps"));
        if self.orphans.is_empty() {
            col = col.child(div().text_xs().child("None."));
        } else {
            col = col.child(
                div().flex().child(
                    Button::new("purge-all")
                        .label("Delete all")
                        .on_click(cx.listener(|this, _, _, cx| this.purge_all_orphans(cx))),
                ),
            );
        }
        for (i, (id, bytes)) in self.orphans.iter().enumerate() {
            let id = *id;
            col =
                col.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().flex_1().text_xs().child(format!(
                            "{}… · {:.1} MB",
                            &id.to_string()[..8],
                            *bytes as f64 / 1_048_576.0
                        )))
                        .child(Button::new(("purge", i)).label("Delete").ghost().on_click(
                            cx.listener(move |this, _, _, cx| this.purge_orphan(id, cx)),
                        )),
                );
        }
        col = col
            .child(div().text_xl().mt_4().child("App list"))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(Button::new("export").label("Export").on_click(
                        cx.listener(|this, _, _, cx| this.export_apps(cx)),
                    ))
                    .child(Button::new("import").label("Import").on_click(
                        cx.listener(|this, _, w, cx| this.import_apps(w, cx)),
                    )),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "Names and addresses only, never sessions. Export file: {}. Import reads: {}.",
                        self.export_path().display(),
                        self.import_path().display()
                    )),
            );

        div()
            .id("manage")
            .flex_1()
            .h_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .items_center()
            .py_6()
            .child(col)
            .into_any_element()
    }
}

impl Focusable for Shell {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        {
            static FIRST: std::sync::Once = std::sync::Once::new();
            FIRST.call_once(|| log("first render"));
        }
        if self.want_activate && (self.panel != Panel::None || self.active_error().is_some()) {
            window.activate_window();
            if self.panel != Panel::Open {
                self.focus.focus(window, cx);
            }
            self.want_activate = false;
        }
        if self.want_shell_focus && self.panel == Panel::None {
            self.focus.focus(window, cx);
            self.want_shell_focus = false;
        }
        if self.want_focus && self.panel == Panel::Open {
            self.add_input.focus_handle(cx).focus(window, cx);
            self.want_focus = false;
        }
        let page: AnyElement = match self.panel {
            Panel::Open => self.open_view(cx),
            Panel::Manage => self.manage_view(cx),
            Panel::Shortcuts => self.shortcuts_view(cx),
            Panel::None => {
                if let Some(msg) = self.active_error().map(str::to_owned) {
                    self.error_view(&msg, cx)
                } else if let Some(view) = self.state.active().and_then(|id| self.host.view(id)) {
                    div().flex_1().h_full().child(view).into_any_element()
                } else {
                    div().flex_1().into_any_element()
                }
            }
        };
        let mut content = div().flex_1().h_full().flex().flex_col().child(page);
        if !self.status.is_empty() {
            // Transient (5 s); the only chrome that may appear below the page.
            content = content.child(
                div()
                    .h(px(24.))
                    .flex_none()
                    .px_2()
                    .flex()
                    .items_center()
                    .text_xs()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(self.status.clone()),
            );
        }
        let root = div()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .id("shell")
            .key_context("Shell")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .on_action(cx.listener(|this, _: &OpenPanel, _, cx| this.open_panel(cx)))
            .on_action(cx.listener(|this, _: &Manage, _, cx| this.set_panel(Panel::Manage, cx)))
            .on_action(
                cx.listener(|this, _: &Shortcuts, _, cx| this.set_panel(Panel::Shortcuts, cx)),
            )
            .on_action(cx.listener(|this, _: &Dismiss, _, cx| this.dismiss(cx)))
            .on_action(cx.listener(|this, _: &Reload, _, cx| this.reload_active(cx)))
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.zoom_active(ZoomChange::In, cx)))
            .on_action(
                cx.listener(|this, _: &ZoomOut, _, cx| this.zoom_active(ZoomChange::Out, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ZoomReset, _, cx| this.zoom_active(ZoomChange::Reset, cx)),
            )
            .on_action(cx.listener(|this, _: &PageCmdPeriod, _, cx| {
                #[cfg(target_os = "macos")]
                if this.panel == Panel::None
                    && let Some(id) = this.state.active()
                {
                    this.host.forward_current_key(cx, id);
                }
                #[cfg(not(target_os = "macos"))]
                let _ = (this, cx);
            }))
            .on_action(cx.listener(|this, _: &GoBack, _, cx| {
                if let Some(id) = this.state.active() {
                    this.host.back(cx, id);
                }
            }))
            .on_action(cx.listener(|this, _: &GoForward, _, cx| {
                if let Some(id) = this.state.active() {
                    this.host.forward(cx, id);
                }
            }))
            .child(self.sidebar(cx))
            .child(content);
        macro_rules! select_actions {
            ($root:ident, $($n:literal => $a:ident),*) => {
                $root$(.on_action(cx.listener(|this, _: &$a, w, cx| this.select_nth($n, w, cx))))*
            };
        }
        select_actions!(
            root, 0 => Select1, 1 => Select2, 2 => Select3, 3 => Select4, 4 => Select5,
            5 => Select6, 6 => Select7, 7 => Select8, 8 => Select9
        )
    }
}

fn main() {
    gpuidium_core::diagnostics::init_clock();
    let (Some(config_dir), Some(data_dir)) = (paths::config_dir(), paths::data_dir()) else {
        eprintln!("cannot determine per-user config/data directories");
        std::process::exit(1);
    };
    let _lock = match InstanceLock::acquire(&config_dir) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    let loaded = match storage::load(&config_dir) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("cannot load config: {e}");
            std::process::exit(1);
        }
    };
    let status = match &loaded.recovery {
        Recovery::None => String::new(),
        Recovery::RestoredBackup { .. } => {
            "Config was damaged; restored the last good backup".into()
        }
        Recovery::StartedEmpty { .. } => {
            "Config was damaged and no backup was usable; the original was preserved".into()
        }
    };
    log(&format!(
        "loaded {} apps from {}",
        loaded.config.apps.len(),
        redact_url("file:///config")
    ));

    #[cfg(target_os = "linux")]
    {
        // GTK must be up before any per-profile WebContext exists (see ADR-001).
        gtk::gdk::set_allowed_backends("x11");
        gtk::init().expect("gtk init (is DISPLAY set / Xwayland running?)");
    }

    let window_cfg = loaded.config.window;
    let settings = loaded.config.settings.clone();
    let zoom = loaded.config.zoom.clone();
    let notifications_off = loaded.config.notifications_off.clone();
    // WebKit reads its spell-checking switch when the first view starts.
    #[cfg(target_os = "macos")]
    mac::set_spellcheck_default(settings.spellcheck);
    let state = loaded.config.to_state();

    #[cfg(target_os = "linux")]
    let app = gpui_kit::platform::linux(gpui_kit::WindowingModes::X11);
    #[cfg(not(target_os = "linux"))]
    let app = gpui_kit::application();

    app.run(move |cx| {
        gpui_kit::init(cx);
        cx.bind_keys([
            KeyBinding::new(mod_key!("l"), OpenPanel, None),
            KeyBinding::new(mod_key!("r"), Reload, None),
            // "+" is Shift+= on US layouts and its own key on German ones.
            KeyBinding::new(mod_key!("="), ZoomIn, None),
            KeyBinding::new(mod_key!("+"), ZoomIn, None),
            KeyBinding::new(mod_key!("shift-="), ZoomIn, None),
            KeyBinding::new(mod_key!("-"), ZoomOut, None),
            KeyBinding::new(mod_key!("0"), ZoomReset, None),
            KeyBinding::new(mod_key!(","), Manage, None),
            KeyBinding::new(mod_key!("?"), Shortcuts, None),
            KeyBinding::new(mod_key!("shift-/"), Shortcuts, None),
            KeyBinding::new("f1", Shortcuts, None),
            KeyBinding::new("escape", Dismiss, None),
            KeyBinding::new(mod_key!("1"), Select1, None),
            KeyBinding::new(mod_key!("2"), Select2, None),
            KeyBinding::new(mod_key!("3"), Select3, None),
            KeyBinding::new(mod_key!("4"), Select4, None),
            KeyBinding::new(mod_key!("5"), Select5, None),
            KeyBinding::new(mod_key!("6"), Select6, None),
            KeyBinding::new(mod_key!("7"), Select7, None),
            KeyBinding::new(mod_key!("8"), Select8, None),
            KeyBinding::new(mod_key!("9"), Select9, None),
            KeyBinding::new("alt-left", GoBack, None),
            KeyBinding::new("alt-right", GoForward, None),
        ]);
        #[cfg(target_os = "macos")]
        install_menus(cx);
        #[cfg(target_os = "macos")]
        icon::set_dock_icon();
        let opts = WindowOptions {
            icon: icon::window_icon(),
            ..WindowOptions::default()
        };
        let (_, shell) = gpui_kit::open_window(opts, cx, move |window, cx| {
            cx.new(|cx| {
                Shell::new(
                    state,
                    window_cfg,
                    settings,
                    zoom,
                    notifications_off,
                    config_dir,
                    data_dir,
                    status,
                    window,
                    cx,
                )
            })
        })
        .expect("open window");
        let _ = shell;
        cx.activate(true);
    });
}
