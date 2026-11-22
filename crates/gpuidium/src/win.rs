//! Windows (WebView2): what WebKitGTK signals do on Linux and AppKit on macOS.
//!
//! Like elsewhere, native callbacks run on the UI thread and only push `HostEvent`s; the
//! shell applies them. Covered here: app shortcuts while a page has focus, the permission
//! policy (deny all, notifications only while the app's switch is on), page notifications
//! as Windows toasts, favicons, load errors, mute, the sidebar tooltip and the taskbar badge.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use async_channel::Sender;
use gpuidium_core::{AppId, diagnostics::log, zoom::ZoomChange};
use webview2_com::Microsoft::Web::WebView2::Win32::*;
use webview2_com::{
    AcceleratorKeyPressedEventHandler, FaviconChangedEventHandler, GetFaviconCompletedHandler,
    NavigationCompletedEventHandler, NotificationReceivedEventHandler,
    PermissionRequestedEventHandler, take_pwstr,
};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, MAPVK_VK_TO_VSC, MapVirtualKeyW, ToUnicode, VIRTUAL_KEY, VK_ADD, VK_CONTROL,
    VK_F1, VK_LEFT, VK_MENU, VK_NUMPAD0, VK_NUMPAD1, VK_NUMPAD9, VK_RIGHT, VK_SHIFT, VK_SUBTRACT,
};
use windows::core::{HSTRING, Interface as _, PWSTR};
use wry::WebViewExtWindows as _;

use crate::host::{HostEvent, Shortcut};

/// Same id as the desktop entry and the macOS bundle; also names the toast sender.
const AUMID: &str = "io.github.rpietzsch.GPUIdium";

/// Process setup, first thing in `main`.
///
/// GPUI draws through a DirectComposition swap chain by default, which covers child
/// windows: the WebView2 child window then stays invisible behind GPUI (SPIKE-003). Without
/// it GPUI renders into a classic swap chain and the page shows. An explicit
/// `GPUI_DISABLE_DIRECT_COMPOSITION` in the environment is respected.
pub fn init_process() {
    const KEY: &str = "GPUI_DISABLE_DIRECT_COMPOSITION";
    if std::env::var_os(KEY).is_none() {
        // SAFETY: called first in `main`, before any other thread exists.
        unsafe { std::env::set_var(KEY, "1") };
    }
    attach_console_for_logs();
}

/// Release builds use the GUI subsystem (no console window). With `GPUIDIUM_LOG` set and
/// started from a terminal, write the log to that terminal; a redirected stderr is kept.
fn attach_console_for_logs() {
    use std::os::windows::io::IntoRawHandle as _;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, SetStdHandle,
    };
    if !gpuidium_core::diagnostics::enabled() {
        return;
    }
    unsafe {
        if GetStdHandle(STD_ERROR_HANDLE).is_ok_and(|h| !h.is_invalid() && !h.0.is_null()) {
            return;
        }
        if AttachConsole(ATTACH_PARENT_PROCESS).is_ok()
            && let Ok(f) = std::fs::OpenOptions::new().write(true).open("CONOUT$")
        {
            let _ = SetStdHandle(STD_ERROR_HANDLE, HANDLE(f.into_raw_handle()));
        }
    }
}

/// GPUI's top-level window, for the tooltip and the taskbar badge (set once at startup).
static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);

pub fn set_main_window(window: &gpui_kit::Window) {
    use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    if let Ok(h) = HasWindowHandle::window_handle(window)
        && let RawWindowHandle::Win32(h) = h.as_raw()
    {
        MAIN_HWND.store(h.hwnd.get(), Ordering::Relaxed);
    }
}

fn main_hwnd() -> Option<HWND> {
    let h = MAIN_HWND.load(Ordering::Relaxed);
    (h != 0).then_some(HWND(h as *mut _))
}

// ---------------------------------------------------------------------------------------
// Per-view hooks

/// Hook one WebView2 into the shell. Failures are logged; the page still works without them.
pub fn install(
    view: &wry::WebView,
    app: AppId,
    tx: &Sender<HostEvent>,
    notify_on: Arc<AtomicBool>,
) {
    if let Err(e) = install_hooks(view, app, tx, notify_on) {
        log(&format!("WebView2 hooks for {app} incomplete: {e}"));
    }
}

fn install_hooks(
    view: &wry::WebView,
    app: AppId,
    tx: &Sender<HostEvent>,
    notify_on: Arc<AtomicBool>,
) -> windows::core::Result<()> {
    let controller = view.controller();
    let webview = view.webview();
    let mut token = 0i64;
    unsafe {
        // Failed loads end in the shell's own error state (like Linux), not Edge's page.
        webview.Settings()?.SetIsBuiltInErrorPageEnabled(false)?;

        // App shortcuts. WebView2 owns keyboard focus while a page is shown, so GPUI's key
        // bindings never see them; WebView2 offers every Ctrl/Alt/F-key press here first.
        let t = tx.clone();
        controller.add_AcceleratorKeyPressed(
            &AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
                args.KeyEventKind(&mut kind)?;
                if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
                    && kind != COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
                {
                    return Ok(());
                }
                let mut vk = 0u32;
                args.VirtualKey(&mut vk)?;
                let mods = Mods::current();
                if let Some(sc) = shortcut_for(vk, key_char(vk, mods.shift), mods) {
                    args.SetHandled(true)?;
                    let _ = t.try_send(HostEvent::Shortcut(sc));
                }
                Ok(())
            })),
            &mut token,
        )?;

        // Every permission (camera, location, ...) is denied; only notifications may be
        // allowed, and only while this app's notifications are on. Nothing is stored in the
        // WebView2 profile, so the switch decides every time and WebView2 never prompts.
        let flag = notify_on.clone();
        webview.add_PermissionRequested(
            &PermissionRequestedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
                args.PermissionKind(&mut kind)?;
                let on = flag.load(Ordering::Relaxed);
                let allow = kind == COREWEBVIEW2_PERMISSION_KIND_NOTIFICATIONS && on;
                log(&format!(
                    "permission request kind={}: app_on={on} allow={allow}",
                    kind.0
                ));
                if let Ok(a3) = args.cast::<ICoreWebView2PermissionRequestedEventArgs3>() {
                    a3.SetSavesInProfile(false)?;
                }
                args.SetState(if allow {
                    COREWEBVIEW2_PERMISSION_STATE_ALLOW
                } else {
                    COREWEBVIEW2_PERMISSION_STATE_DENY
                })
            })),
            &mut token,
        )?;

        // Load failures. Server error pages (404, 500) have content and stay visible; a
        // cancelled navigation was superseded or refused by policy.
        let t = tx.clone();
        webview.add_NavigationCompleted(
            &NavigationCompletedEventHandler::create(Box::new(move |sender, args| {
                let (Some(w), Some(args)) = (sender, args) else {
                    return Ok(());
                };
                let mut ok = windows::core::BOOL(0);
                args.IsSuccess(&mut ok)?;
                if ok.as_bool() {
                    return Ok(());
                }
                if let Ok(a2) = args.cast::<ICoreWebView2NavigationCompletedEventArgs2>() {
                    let mut http = 0i32;
                    if a2.HttpStatusCode(&mut http).is_ok() && http != 0 {
                        return Ok(());
                    }
                }
                let mut status = COREWEBVIEW2_WEB_ERROR_STATUS::default();
                args.WebErrorStatus(&mut status)?;
                if status == COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED {
                    return Ok(());
                }
                log(&format!("WebView2 load error status {}", status.0));
                let mut src = PWSTR::null();
                w.Source(&mut src)?;
                let _ = t.try_send(HostEvent::LoadFailed(
                    app,
                    take_pwstr(src),
                    load_error_text(status).into(),
                ));
                Ok(())
            })),
            &mut token,
        )?;

        // Favicons (WebView2 1.0.1774+).
        match webview.cast::<ICoreWebView2_15>() {
            Ok(w15) => {
                let t = tx.clone();
                w15.add_FaviconChanged(
                    &FaviconChangedEventHandler::create(Box::new(move |sender, _| {
                        let Some(w) = sender.and_then(|s| s.cast::<ICoreWebView2_15>().ok()) else {
                            return Ok(());
                        };
                        let t = t.clone();
                        w.GetFavicon(
                            COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG,
                            &GetFaviconCompletedHandler::create(Box::new(move |hr, stream| {
                                hr?;
                                match stream.and_then(|s| read_stream(&s, 256 * 1024)) {
                                    Some(png) if valid_png(&png) => {
                                        log(&format!("favicon: {} bytes", png.len()));
                                        let _ = t.try_send(HostEvent::Favicon(app, png));
                                    }
                                    _ => log("favicon: none or not a small PNG"),
                                }
                                Ok(())
                            })),
                        )
                    })),
                    &mut token,
                )?;
            }
            Err(_) => log("WebView2 runtime has no favicon API; using monograms"),
        }

        // Page notifications (WebView2 1.0.2420+): never WebView2's own UI; the shell shows
        // them as toasts while the app's switch is on.
        match webview.cast::<ICoreWebView2_24>() {
            Ok(w24) => {
                let (flag, t) = (notify_on, tx.clone());
                w24.add_NotificationReceived(
                    &NotificationReceivedEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        args.SetHandled(true)?;
                        if !flag.load(Ordering::Relaxed) {
                            return Ok(());
                        }
                        let n = args.Notification()?;
                        let (mut title, mut body) = (PWSTR::null(), PWSTR::null());
                        n.Title(&mut title)?;
                        n.Body(&mut body)?;
                        let clip = |s: String, max: usize| s.chars().take(max).collect::<String>();
                        let (title, body) =
                            (clip(take_pwstr(title), 200), clip(take_pwstr(body), 500));
                        let _ = n.ReportShown();
                        let id = remember_notification(n);
                        let _ = t.try_send(HostEvent::Notify(app, id, title, body));
                        Ok(())
                    })),
                    &mut token,
                )?;
            }
            Err(_) => log("WebView2 runtime has no notification API; page notifications off"),
        }
    }
    Ok(())
}

/// Engine-level mute of one page.
pub fn set_muted(view: &wry::WebView, muted: bool) {
    if let Ok(w8) = view.webview().cast::<ICoreWebView2_8>() {
        let _ = unsafe { w8.SetIsMuted(muted) };
    }
}

/// Short, user-facing reason for a failed load.
fn load_error_text(status: COREWEBVIEW2_WEB_ERROR_STATUS) -> &'static str {
    match status {
        COREWEBVIEW2_WEB_ERROR_STATUS_HOST_NAME_NOT_RESOLVED => "Server not found",
        COREWEBVIEW2_WEB_ERROR_STATUS_CANNOT_CONNECT
        | COREWEBVIEW2_WEB_ERROR_STATUS_SERVER_UNREACHABLE => "Cannot connect to the server",
        COREWEBVIEW2_WEB_ERROR_STATUS_TIMEOUT => "The connection timed out",
        COREWEBVIEW2_WEB_ERROR_STATUS_DISCONNECTED => "No network connection",
        COREWEBVIEW2_WEB_ERROR_STATUS_CONNECTION_ABORTED
        | COREWEBVIEW2_WEB_ERROR_STATUS_CONNECTION_RESET => "The connection was interrupted",
        COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_COMMON_NAME_IS_INCORRECT
        | COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_EXPIRED
        | COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_IS_INVALID
        | COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_REVOKED
        | COREWEBVIEW2_WEB_ERROR_STATUS_CLIENT_CERTIFICATE_CONTAINS_ERRORS => {
            "The server's certificate is not valid"
        }
        COREWEBVIEW2_WEB_ERROR_STATUS_REDIRECT_FAILED => "Too many redirects",
        _ => "The page could not be loaded",
    }
}

fn read_stream(s: &windows::Win32::System::Com::IStream, max: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf = [0u8; 16 * 1024];
    loop {
        let mut n = 0u32;
        let hr = unsafe { s.Read(buf.as_mut_ptr().cast(), buf.len() as u32, Some(&mut n)) };
        if hr.is_err() {
            return None;
        }
        if n == 0 {
            return Some(out);
        }
        out.extend_from_slice(&buf[..n as usize]);
        if out.len() > max {
            return None;
        }
    }
}

fn valid_png(png: &[u8]) -> bool {
    !png.is_empty() && image::load_from_memory_with_format(png, image::ImageFormat::Png).is_ok()
}

// ---------------------------------------------------------------------------------------
// Shortcuts

#[derive(Debug, Clone, Copy, Default)]
struct Mods {
    ctrl: bool,
    shift: bool,
    alt: bool,
}

impl Mods {
    fn current() -> Self {
        let down = |vk: VIRTUAL_KEY| unsafe { GetKeyState(i32::from(vk.0)) } < 0;
        Self {
            ctrl: down(VK_CONTROL),
            shift: down(VK_SHIFT),
            alt: down(VK_MENU),
        }
    }
}

/// The character `vk` types on the current keyboard layout with or without Shift (no
/// Ctrl), so "+" and "?" are found wherever the layout puts them. Dead keys give `None`.
fn key_char(vk: u32, shift: bool) -> Option<char> {
    let mut state = [0u8; 256];
    if shift {
        state[usize::from(VK_SHIFT.0)] = 0x80;
    }
    let mut buf = [0u16; 4];
    // Flag 0x4: leave the keyboard state (dead-key buffer) untouched.
    let n = unsafe {
        let scan = MapVirtualKeyW(vk, MAPVK_VK_TO_VSC);
        ToUnicode(vk, scan, Some(&state), &mut buf, 0x4)
    };
    if n == 1 {
        char::from_u32(u32::from(buf[0]))
    } else {
        None
    }
}

/// Same combinations as the Linux backend (`host::shortcut_for`): only exact Ctrl+key, so
/// pages keep their Ctrl+Shift shortcuts, Ctrl+K and AltGr (Ctrl+Alt) characters.
fn shortcut_for(vk: u32, ch: Option<char>, m: Mods) -> Option<Shortcut> {
    let is = |k: VIRTUAL_KEY| vk == u32::from(k.0);
    if is(VK_F1) {
        return Some(Shortcut::Keys);
    }
    if m.alt && !m.ctrl {
        return if is(VK_LEFT) {
            Some(Shortcut::Back)
        } else if is(VK_RIGHT) {
            Some(Shortcut::Forward)
        } else {
            None
        };
    }
    if !m.ctrl || m.alt {
        return None;
    }
    // Number pad keys, whatever the NumLock state.
    if is(VK_ADD) {
        return Some(Shortcut::Zoom(ZoomChange::In));
    }
    if is(VK_SUBTRACT) {
        return Some(Shortcut::Zoom(ZoomChange::Out));
    }
    if is(VK_NUMPAD0) {
        return Some(Shortcut::Zoom(ZoomChange::Reset));
    }
    if (u32::from(VK_NUMPAD1.0)..=u32::from(VK_NUMPAD9.0)).contains(&vk) && !m.shift {
        return Some(Shortcut::Select((vk - u32::from(VK_NUMPAD1.0)) as usize));
    }
    match ch?.to_lowercase().next()? {
        // Zoom: "+" needs Shift on US layouts, is its own key on German ones.
        '+' | '=' => Some(Shortcut::Zoom(ZoomChange::In)),
        '-' => Some(Shortcut::Zoom(ZoomChange::Out)),
        '0' => Some(Shortcut::Zoom(ZoomChange::Reset)),
        '?' => Some(Shortcut::Keys),
        '/' if m.shift => Some(Shortcut::Keys),
        _ if m.shift => None,
        'l' => Some(Shortcut::Open),
        'r' => Some(Shortcut::Reload),
        ',' => Some(Shortcut::Manage),
        c => {
            let n = c.to_digit(10)? as usize;
            (1..=9).contains(&n).then(|| Shortcut::Select(n - 1))
        }
    }
}

// ---------------------------------------------------------------------------------------
// Notifications

/// A click on a toast: the app and the page's notification id.
pub type NotificationClick = (AppId, u64);

static CLICKS: OnceLock<Sender<NotificationClick>> = OnceLock::new();
static NEXT_NOTE: AtomicU64 = AtomicU64::new(1);

thread_local! {
    /// Recent page notifications (UI thread), so a toast click can reach the page's handler.
    static NOTES: RefCell<VecDeque<(u64, ICoreWebView2Notification)>> =
        const { RefCell::new(VecDeque::new()) };
    /// Shown toasts; their `Activated` handlers must stay alive.
    static TOASTS: RefCell<VecDeque<windows::UI::Notifications::ToastNotification>> =
        const { RefCell::new(VecDeque::new()) };
}

const KEEP: usize = 50;

fn remember_notification(n: ICoreWebView2Notification) -> u64 {
    let id = NEXT_NOTE.fetch_add(1, Ordering::Relaxed);
    NOTES.with_borrow_mut(|q| {
        q.push_back((id, n));
        if q.len() > KEEP {
            q.pop_front();
        }
    });
    id
}

/// The user clicked the toast of notification `note`: fire the page's own click handler.
pub fn click_notification(note: u64) {
    NOTES.with_borrow(|q| {
        if let Some((_, n)) = q.iter().find(|(id, _)| *id == note) {
            let _ = unsafe { n.ReportClicked() };
        }
    });
}

/// Register the toast sender once (per-user registry, no admin rights) and keep the channel
/// for toast clicks. Unpackaged apps need an `AppUserModelId` entry for toasts to appear.
pub fn init_notifications(clicks: Sender<NotificationClick>) {
    if CLICKS.set(clicks).is_err() {
        return;
    }
    if let Err(e) = register_toast_sender() {
        log(&format!("registering the toast sender failed: {e}"));
    }
}

fn register_toast_sender() -> windows::core::Result<()> {
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
        RegCreateKeyExW, RegSetValueExW,
    };
    let path = HSTRING::from(format!(r"Software\Classes\AppUserModelId\{AUMID}"));
    let mut key = HKEY::default();
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &path,
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .ok()?;
    }
    let mut values = vec![("DisplayName", "GPUIdium".to_owned())];
    // `task install:win` puts the icon next to the program.
    if let Some(ico) = std::env::current_exe()
        .ok()
        .and_then(|e| Some(e.parent()?.join("gpuidium.ico")))
        .filter(|p| p.is_file())
    {
        values.push(("IconUri", ico.to_string_lossy().into_owned()));
    }
    let mut res = Ok(());
    for (name, value) in values {
        let wide: Vec<u8> = value
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        let r = unsafe { RegSetValueExW(key, &HSTRING::from(name), None, REG_SZ, Some(&wide)) };
        if res.is_ok() {
            res = r.ok();
        }
    }
    unsafe {
        let _ = RegCloseKey(key);
    }
    res
}

/// Show one page notification as a toast: app name, title, body; grouped per app; silent
/// unless `sound`. A click is routed back through `init_notifications`.
pub fn notify(app: AppId, app_name: &str, note: u64, title: &str, body: &str, sound: bool) -> bool {
    match show_toast(app, app_name, note, title, body, sound) {
        Ok(()) => true,
        Err(e) => {
            log(&format!("toast failed: {e}"));
            false
        }
    }
}

fn toast_xml(app_name: &str, title: &str, body: &str, sound: bool) -> String {
    let audio = if sound {
        ""
    } else {
        r#"<audio silent="true"/>"#
    };
    format!(
        r#"<toast><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text><text>{}</text></binding></visual>{audio}</toast>"#,
        xml_escape(app_name),
        xml_escape(title),
        xml_escape(body)
    )
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Control characters are not allowed in XML 1.0.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

fn show_toast(
    app: AppId,
    app_name: &str,
    note: u64,
    title: &str,
    body: &str,
    sound: bool,
) -> windows::core::Result<()> {
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Foundation::TypedEventHandler;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(toast_xml(app_name, title, body, sound)))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;
    toast.SetGroup(&HSTRING::from(app.to_string()))?;
    toast.SetTag(&HSTRING::from(note.to_string()))?;
    toast.Activated(&TypedEventHandler::new(move |_, _| {
        if let Some(tx) = CLICKS.get() {
            let _ = tx.try_send((app, note));
        }
        Ok(())
    }))?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))?.Show(&toast)?;
    TOASTS.with_borrow_mut(|q| {
        q.push_back(toast);
        if q.len() > KEEP {
            q.pop_front();
        }
    });
    Ok(())
}

// ---------------------------------------------------------------------------------------
// Sidebar tooltip and taskbar badge

thread_local! {
    static TOOLTIP: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
}

/// Native tooltip for the sidebar icons. GPUI draws tooltips inside its own window, and the
/// WebView2 child window covers the part that sticks out of the narrow sidebar. A tooltip
/// control is a top-level window, so it shows above the page. `None` hides it.
pub fn set_tooltip(text: Option<&str>) {
    if let Err(e) = update_tooltip(text) {
        log(&format!("tooltip: {e}"));
    }
}

fn update_tooltip(text: Option<&str>) -> windows::core::Result<()> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Controls::{
        TOOLTIPS_CLASSW, TTF_ABSOLUTE, TTF_TRACK, TTM_ADDTOOLW, TTM_SETMAXTIPWIDTH,
        TTM_TRACKACTIVATE, TTM_TRACKPOSITION, TTM_UPDATETIPTEXTW, TTS_ALWAYSTIP, TTS_NOPREFIX,
        TTTOOLINFOW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CW_USEDEFAULT, CreateWindowExW, GetCursorPos, SendMessageW, WINDOW_STYLE, WS_EX_TOPMOST,
        WS_POPUP,
    };
    let Some(owner) = main_hwnd() else {
        return Ok(());
    };
    let mut tip = HWND(TOOLTIP.get() as *mut _);
    let mut wide: Vec<u16> = text.unwrap_or("").encode_utf16().chain([0]).collect();
    let mut info = TTTOOLINFOW {
        // Without a common-controls 6 manifest only the shorter (V2) struct is accepted.
        cbSize: (std::mem::size_of::<TTTOOLINFOW>() - std::mem::size_of::<*mut ()>()) as u32,
        uFlags: TTF_TRACK | TTF_ABSOLUTE,
        hwnd: owner,
        uId: 1,
        lpszText: PWSTR(wide.as_mut_ptr()),
        ..Default::default()
    };
    let lp = LPARAM(&mut info as *mut _ as isize);
    unsafe {
        if tip.0.is_null() {
            if text.is_none() {
                return Ok(());
            }
            tip = CreateWindowExW(
                WS_EX_TOPMOST,
                TOOLTIPS_CLASSW,
                None,
                WS_POPUP | WINDOW_STYLE(TTS_NOPREFIX | TTS_ALWAYSTIP),
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                Some(owner),
                None,
                None,
                None,
            )?;
            TOOLTIP.set(tip.0 as isize);
            SendMessageW(tip, TTM_SETMAXTIPWIDTH, None, Some(LPARAM(400)));
            SendMessageW(tip, TTM_ADDTOOLW, None, Some(lp));
        }
        match text {
            Some(_) => {
                SendMessageW(tip, TTM_UPDATETIPTEXTW, None, Some(lp));
                let mut p = POINT::default();
                GetCursorPos(&mut p)?;
                // Below and right of the pointer, like a regular tooltip.
                let (x, y) = (p.x + 12, p.y + 20);
                let pos = ((y as u32 & 0xffff) << 16) | (x as u32 & 0xffff);
                SendMessageW(tip, TTM_TRACKPOSITION, None, Some(LPARAM(pos as isize)));
                SendMessageW(tip, TTM_TRACKACTIVATE, Some(WPARAM(1)), Some(lp));
            }
            None => {
                SendMessageW(tip, TTM_TRACKACTIVATE, Some(WPARAM(0)), Some(lp));
            }
        }
    }
    Ok(())
}

thread_local! {
    static BADGE_ICON: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
}

/// Unread indicator on the taskbar button: a red dot overlay, the label ("3", "99+", "•") as
/// its accessible description. `None` removes it.
pub fn set_taskbar_badge(label: Option<&str>) {
    if let Err(e) = update_taskbar_badge(label) {
        log(&format!("taskbar badge: {e}"));
    }
}

fn update_taskbar_badge(label: Option<&str>) -> windows::core::Result<()> {
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
    use windows::Win32::UI::Shell::{ITaskbarList3, TaskbarList};
    use windows::Win32::UI::WindowsAndMessaging::HICON;
    let Some(hwnd) = main_hwnd() else {
        return Ok(());
    };
    unsafe {
        let bar: ITaskbarList3 = CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER)?;
        bar.HrInit()?;
        match label {
            Some(l) => {
                let mut icon = BADGE_ICON.get();
                if icon == 0 {
                    icon = dot_icon()?.0 as isize;
                    BADGE_ICON.set(icon);
                }
                let text = if l == "•" {
                    "Unread messages".to_owned()
                } else {
                    format!("{l} unread")
                };
                bar.SetOverlayIcon(hwnd, HICON(icon as *mut _), &HSTRING::from(text))
            }
            None => bar.SetOverlayIcon(hwnd, HICON::default(), &HSTRING::new()),
        }
    }
}

/// 16x16 anti-aliased red dot (same red as the sidebar badges).
fn dot_icon() -> windows::core::Result<windows::Win32::UI::WindowsAndMessaging::HICON> {
    use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
    use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, ICONINFO};
    const N: i32 = 16;
    let mut px = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let (dx, dy) = (x as f32 + 0.5 - 8.0, y as f32 + 0.5 - 8.0);
            let a = (7.5 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
            let a = (a * 255.0) as u8;
            // BGRA, premultiplied.
            let pm = |c: u8| ((u16::from(c) * u16::from(a)) / 255) as u8;
            px.extend_from_slice(&[pm(0x4d), pm(0x48), pm(0xe5), a]);
        }
    }
    unsafe {
        let color = CreateBitmap(N, N, 1, 32, Some(px.as_ptr().cast()));
        let mask_bits = vec![0u8; (N * N / 8) as usize];
        let mask = CreateBitmap(N, N, 1, 1, Some(mask_bits.as_ptr().cast()));
        let info = ICONINFO {
            fIcon: true.into(),
            hbmMask: mask,
            hbmColor: color,
            ..Default::default()
        };
        let icon = CreateIconIndirect(&info);
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CTRL: Mods = Mods {
        ctrl: true,
        shift: false,
        alt: false,
    };
    const CTRL_SHIFT: Mods = Mods {
        ctrl: true,
        shift: true,
        alt: false,
    };

    fn vk(k: VIRTUAL_KEY) -> u32 {
        u32::from(k.0)
    }

    #[test]
    fn maps_ctrl_shortcuts_like_linux() {
        let sc = |c: char, m| shortcut_for(u32::from(c.to_ascii_uppercase()), Some(c), m);
        assert_eq!(sc('l', CTRL), Some(Shortcut::Open));
        assert_eq!(sc('r', CTRL), Some(Shortcut::Reload));
        assert_eq!(sc('3', CTRL), Some(Shortcut::Select(2)));
        assert_eq!(sc('0', CTRL), Some(Shortcut::Zoom(ZoomChange::Reset)));
        // Pages keep Ctrl+K and the Shift variants.
        assert_eq!(sc('k', CTRL), None);
        assert_eq!(sc('l', CTRL_SHIFT), None);
        // Plain keys and AltGr (Ctrl+Alt) never trigger.
        assert_eq!(sc('l', Mods::default()), None);
        let altgr = Mods { alt: true, ..CTRL };
        assert_eq!(sc('l', altgr), None);
    }

    #[test]
    fn zoom_and_help_follow_the_typed_character() {
        // VK_OEM_PLUS types "=" (US) or "+" (German); VK_OEM_2 types "/" and with Shift "?".
        assert_eq!(
            shortcut_for(0xBB, Some('='), CTRL),
            Some(Shortcut::Zoom(ZoomChange::In))
        );
        assert_eq!(
            shortcut_for(0xBB, Some('+'), CTRL_SHIFT),
            Some(Shortcut::Zoom(ZoomChange::In))
        );
        assert_eq!(
            shortcut_for(0xBD, Some('-'), CTRL),
            Some(Shortcut::Zoom(ZoomChange::Out))
        );
        assert_eq!(
            shortcut_for(0xBF, Some('?'), CTRL_SHIFT),
            Some(Shortcut::Keys)
        );
        assert_eq!(
            shortcut_for(vk(VK_F1), None, Mods::default()),
            Some(Shortcut::Keys)
        );
        assert_eq!(
            shortcut_for(vk(VK_ADD), None, CTRL),
            Some(Shortcut::Zoom(ZoomChange::In))
        );
        assert_eq!(
            shortcut_for(vk(VK_NUMPAD1) + 4, None, CTRL),
            Some(Shortcut::Select(4))
        );
    }

    #[test]
    fn alt_arrows_navigate() {
        let alt = Mods {
            alt: true,
            ..Mods::default()
        };
        assert_eq!(shortcut_for(vk(VK_LEFT), None, alt), Some(Shortcut::Back));
        assert_eq!(
            shortcut_for(vk(VK_RIGHT), None, alt),
            Some(Shortcut::Forward)
        );
        assert_eq!(shortcut_for(vk(VK_LEFT), None, CTRL), None);
    }

    #[test]
    fn toast_text_is_escaped() {
        let xml = toast_xml("A&B", "<b>hi</b>", "say \"x\"\u{7}", false);
        assert!(xml.contains("<text>A&amp;B</text>"));
        assert!(xml.contains("<text>&lt;b&gt;hi&lt;/b&gt;</text>"));
        assert!(xml.contains("<text>say &quot;x&quot; </text>"));
        assert!(xml.contains(r#"<audio silent="true"/>"#));
        assert!(!toast_xml("a", "b", "c", true).contains("audio"));
        // Well-formed for the toast parser.
        let doc = windows::Data::Xml::Dom::XmlDocument::new().unwrap();
        doc.LoadXml(&HSTRING::from(xml)).unwrap();
    }
}
