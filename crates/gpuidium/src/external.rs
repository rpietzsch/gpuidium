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
/// clipped. Returns false if neither path worked.
pub fn notify(app: &str, title: &str, body: &str) -> bool {
    #[cfg(target_os = "linux")]
    {
        let clip = |s: &str| s.chars().take(200).collect::<String>();
        let (title, body) = (clip(&format!("{app}: {title}")), clip(body));
        if notify_gio(&title, &body) {
            return true;
        }
        Command::new("notify-send")
            .arg("--app-name=GPUIdium")
            .arg("--")
            .arg(&title)
            .arg(&body)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (app, title, body);
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
