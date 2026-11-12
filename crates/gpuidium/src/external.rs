//! Hand a vetted web URL to the system browser. Never runs arbitrary handlers.

use gpuidium_core::policy::{NavDecision, decide_new_window};
use std::process::{Command, Stdio};

/// Open `url` in the default browser iff policy says it is a plain web URL.
pub fn open(url: &str) -> bool {
    if decide_new_window(url) != NavDecision::OpenExternal {
        return false;
    }
    #[cfg(target_os = "linux")]
    let mut cmd = Command::new("xdg-open");
    #[cfg(target_os = "macos")]
    let mut cmd = Command::new("open");
    #[cfg(target_os = "windows")]
    let mut cmd = {
        // rundll32 avoids cmd.exe metacharacter parsing of the URL.
        let mut c = Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    cmd.arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

/// Show a desktop notification (Linux): through GLib's notification API, which talks to the
/// desktop's notification service and, inside Flatpak, to the notification portal. Falls back
/// to `notify-send` (arguments passed after `--`, never through a shell). Title/body are
/// clipped. Returns false if neither path worked. Without `sound`, `notify-send` goes first,
/// because only it can ask the server to stay quiet (`suppress-sound` hint; GNotification
/// has no such option).
// macOS delivers through `mac::notify` (Notification Center).
#[cfg(not(target_os = "macos"))]
pub fn notify(app: &str, title: &str, body: &str, sound: bool) -> bool {
    #[cfg(target_os = "linux")]
    {
        let clip = |s: &str| s.chars().take(200).collect::<String>();
        let (title, body) = (clip(&format!("{app}: {title}")), clip(body));
        let notify_send = |quiet: bool| {
            let mut cmd = Command::new("notify-send");
            cmd.arg("--app-name=GPUIdium");
            if quiet {
                cmd.arg("--hint=boolean:suppress-sound:true");
            }
            cmd.arg("--")
                .arg(&title)
                .arg(&body)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .is_ok()
        };
        if !sound && notify_send(true) {
            return true;
        }
        notify_gio(&title, &body) || (sound && notify_send(false))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (app, title, body, sound);
        false
    }
}

#[cfg(target_os = "linux")]
fn notify_gio(title: &str, body: &str) -> bool {
    use gtk::gio::{self, prelude::*};
    use std::cell::RefCell;
    thread_local! {
        // Registered once, on the UI thread that initialised GTK.
        static APP: RefCell<Option<gio::Application>> = const { RefCell::new(None) };
    }
    APP.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            let app = gio::Application::new(
                Some("io.github.rpietzsch.GPUIdium"),
                gio::ApplicationFlags::NON_UNIQUE,
            );
            if app.register(gio::Cancellable::NONE).is_err() {
                return false;
            }
            *slot = Some(app);
        }
        let note = gio::Notification::new(title);
        note.set_body(Some(body));
        slot.as_ref()
            .map(|a| a.send_notification(None, &note))
            .is_some()
    })
}
