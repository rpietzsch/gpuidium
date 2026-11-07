//! GPUIdium: GPUI shell for web apps.

mod external;
mod host;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, Disableable as _};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement, IntoElement,
    KeyBinding, ParentElement, Render, StatefulInteractiveElement, Styled, Window, WindowOptions,
    actions, div, px,
};
use gpuidium_core::{
    AppId, AppState, Command, Event, RemoveMode,
    diagnostics::{log, redact_url},
    instance::InstanceLock,
    paths, profiles,
    storage::{self, Config, Recovery, WindowConfig},
};

use host::{HostEvent, WebviewHost, WryHost};

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

actions!(
    gpuidium,
    [
        FocusUrl, Reload, GoBack, GoForward, Select1, Select2, Select3, Select4, Select5, Select6,
        Select7, Select8, Select9
    ]
);

#[derive(Default, Clone)]
struct PageInfo {
    url: String,
    title: String,
    loading: bool,
}

struct Shell {
    state: AppState,
    window_cfg: WindowConfig,
    config_dir: PathBuf,
    data_dir: PathBuf,
    host: WryHost,
    add_input: Entity<InputState>,
    pages: HashMap<AppId, PageInfo>,
    pending_remove: Option<AppId>,
    status: String,
    focus: FocusHandle,
}

impl Shell {
    fn new(
        state: AppState,
        window_cfg: WindowConfig,
        config_dir: PathBuf,
        data_dir: PathBuf,
        status: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let add_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Add a web app: https://…"));
        cx.subscribe_in(
            &add_input,
            window,
            |this, _, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::PressEnter { .. }) {
                    this.add_from_input(window, cx);
                }
            },
        )
        .detach();

        // Drain native events on the UI thread.
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                if this
                    .update(cx, |this, cx| this.drain_host_events(cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        let mut shell = Self {
            state,
            window_cfg,
            config_dir,
            data_dir: data_dir.clone(),
            host: WryHost::new(data_dir),
            add_input,
            pages: HashMap::new(),
            pending_remove: None,
            status,
            focus: cx.focus_handle(),
        };
        shell.activate_current(window, cx);
        shell
    }

    fn save(&mut self) {
        let cfg = Config::from_state(&self.state, self.window_cfg);
        if let Err(e) = storage::save(&self.config_dir, &cfg) {
            self.status = format!("Could not save config: {e}");
        }
    }

    /// Ensure the active app has a native view, and show only that one.
    fn activate_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.state.active()
            && !self.host.contains(id)
            && let Some(app) = self.state.get(id).cloned()
        {
            match self
                .host
                .create(window, cx, id, app.profile_id, &app.start_url)
            {
                Ok(()) => {
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
        self.host.show_only(cx, self.state.active());
        cx.notify();
    }

    fn drain_host_events(&mut self, cx: &mut Context<Self>) {
        let events = self.host.drain_events();
        if events.is_empty() {
            return;
        }
        for ev in events {
            match ev {
                HostEvent::LoadStarted(id, url) => {
                    let p = self.pages.entry(id).or_default();
                    p.url = url;
                    p.loading = true;
                }
                HostEvent::LoadFinished(id, url) => {
                    let p = self.pages.entry(id).or_default();
                    p.url = url;
                    p.loading = false;
                }
                HostEvent::Title(id, t) => self.pages.entry(id).or_default().title = t,
                HostEvent::Blocked(u) => self.status = format!("Blocked navigation to {u}"),
                HostEvent::OpenedExternally(u) => {
                    self.status = format!("Opened in system browser: {u}")
                }
            }
        }
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
                        if mode == RemoveMode::DeleteData {
                            // The native view is closed above; only this profile is touched.
                            self.status = match profiles::delete(&self.data_dir, profile_id) {
                                Ok(()) => "App removed and its browsing data deleted".into(),
                                Err(e) => format!("App removed, but deleting its data failed: {e}"),
                            };
                        } else {
                            self.status = "App removed; its browsing data was kept on disk".into();
                        }
                    }
                }
                self.save();
                self.activate_current(window, cx);
            }
            Err(e) => self.status = e.to_string(),
        }
        cx.notify();
    }

    fn add_from_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.add_input.read(cx).value().to_string();
        if input.trim().is_empty() {
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
            // Make the newly added app the active one and clear the field.
            if let Some(last) = self.state.apps().last().map(|a| a.id) {
                self.apply(Command::Select(last), window, cx);
            }
            self.add_input
                .update(cx, |s, cx| s.set_value("", window, cx));
            self.status.clear();
        }
    }

    fn select_nth(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.state.apps().get(n).map(|a| a.id) {
            self.pending_remove = None;
            self.apply(Command::Select(id), window, cx);
        }
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

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.state.active();
        let loading = active
            .and_then(|id| self.pages.get(&id))
            .is_some_and(|p| p.loading);
        let url = active
            .and_then(|id| self.pages.get(&id))
            .map(|p| p.url.clone())
            .unwrap_or_default();
        let btn = |id: &'static str, label: &'static str| Button::new(id).label(label).ghost();
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .h(px(40.))
            .border_b_1()
            .border_color(cx.theme().border)
            .child(btn("back", "◀").on_click(cx.listener(|this, _, _, cx| {
                if let Some(id) = this.state.active() {
                    this.host.back(cx, id);
                }
            })))
            .child(btn("fwd", "▶").on_click(cx.listener(|this, _, _, cx| {
                if let Some(id) = this.state.active() {
                    this.host.forward(cx, id);
                }
            })))
            .child(btn("home", "⌂").on_click(cx.listener(|this, _, _, cx| {
                if let Some(id) = this.state.active()
                    && let Some(start) = this.state.get(id).map(|a| a.start_url.clone())
                    && let Err(e) = this.host.navigate(cx, id, &start)
                {
                    this.status = e.to_string();
                }
            })))
            .child(btn("reload", "⟳").on_click(cx.listener(|this, _, _, cx| {
                if let Some(id) = this.state.active() {
                    this.host.reload(cx, id);
                }
            })))
            .child(div().flex_1().child(format!(
                "{}{}",
                self.current_origin(),
                if loading { "  (loading…)" } else { "" }
            )))
            .child(
                btn("ext", "Open in browser").on_click(cx.listener(move |this, _, _, cx| {
                    if !external::open(&url) {
                        this.status = "Cannot open this address externally".into();
                    }
                    cx.notify();
                })),
            )
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = div()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .flex_1()
            .overflow_hidden();
        let count = self.state.apps().len();
        for (i, app) in self.state.apps().iter().enumerate() {
            let id = app.id;
            let selected = self.state.active() == Some(id);
            let confirming = self.pending_remove == Some(id);
            let initial = app
                .name
                .chars()
                .next()
                .unwrap_or('?')
                .to_uppercase()
                .to_string();
            let title = self
                .pages
                .get(&id)
                .map(|p| p.title.clone())
                .filter(|t| !t.is_empty());
            let mut row = div()
                .id(("app", i))
                .flex()
                .flex_col()
                .gap_1()
                .p_2()
                .rounded_md()
                .cursor_pointer()
                .when_selected(selected, cx.theme().accent, cx.theme().accent_foreground)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.pending_remove = None;
                    this.apply(Command::Select(id), window, cx);
                }))
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
                                .child(initial),
                        )
                        .child(div().flex_1().child(app.name.clone())),
                );
            if let Some(t) = title.filter(|_| selected) {
                row = row.child(div().text_xs().child(t));
            }
            if selected && !confirming {
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
                        .child(Button::new(("rm", i)).label("Remove").ghost().on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.pending_remove = Some(id);
                                cx.notify();
                            }),
                        )),
                );
            }
            if confirming {
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
            }
            list = list.child(row);
        }
        div()
            .flex()
            .flex_col()
            .w(px(240.))
            .h_full()
            .border_r_1()
            .border_color(cx.theme().border)
            .child(list)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(Input::new(&self.add_input))
                    .child(
                        Button::new("add")
                            .label("Add")
                            .primary()
                            .on_click(cx.listener(|this, _, w, cx| this.add_from_input(w, cx))),
                    )
                    .child(div().text_xs().child(self.status.clone())),
            )
    }
}

/// Selected-row styling from theme tokens (keeps contrast in light and dark).
trait SelectedStyle: Styled + Sized {
    fn when_selected(self, selected: bool, bg: gpui_kit::Hsla, fg: gpui_kit::Hsla) -> Self {
        if selected {
            self.bg(bg).text_color(fg)
        } else {
            self
        }
    }
}
impl<T: Styled + Sized> SelectedStyle for T {}

impl Focusable for Shell {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = match self.state.active().and_then(|id| self.host.view(id)) {
            Some(view) => div().flex_1().h_full().child(view),
            None => div()
                .flex_1()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .child("Add a web app, e.g. https://example.com"),
        };
        let root = div()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .id("shell")
            .key_context("Shell")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .on_action(cx.listener(|this, _: &FocusUrl, window, cx| {
                this.add_input.focus_handle(cx).focus(window, cx);
            }))
            .on_action(cx.listener(|this, _: &Reload, _, cx| {
                if let Some(id) = this.state.active() {
                    this.host.reload(cx, id);
                }
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
            .child(self.toolbar(cx))
            .child(div().flex().flex_1().child(self.sidebar(cx)).child(content));
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
    let state = loaded.config.to_state();

    #[cfg(target_os = "linux")]
    let app = gpui_kit::platform::linux(gpui_kit::WindowingModes::X11);
    #[cfg(not(target_os = "linux"))]
    let app = gpui_kit::application();

    app.run(move |cx| {
        gpui_kit::init(cx);
        cx.bind_keys([
            KeyBinding::new(mod_key!("l"), FocusUrl, None),
            KeyBinding::new(mod_key!("r"), Reload, None),
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
        let opts = WindowOptions::default();
        let (_, shell) = gpui_kit::open_window(opts, cx, move |window, cx| {
            cx.new(|cx| Shell::new(state, window_cfg, config_dir, data_dir, status, window, cx))
        })
        .expect("open window");
        let _ = shell;
        cx.activate(true);
    });
}
