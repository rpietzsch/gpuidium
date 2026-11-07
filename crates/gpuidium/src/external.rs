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

/// Show a desktop notification through `notify-send` (Linux). Title/body are passed as
/// separate arguments after `--`, never through a shell. Returns false if it cannot run.
pub fn notify(app: &str, title: &str, body: &str) -> bool {
    #[cfg(target_os = "linux")]
    {
        let clip = |s: &str| s.chars().take(200).collect::<String>();
        Command::new("notify-send")
            .arg("--app-name=GPUIdium")
            .arg("--")
            .arg(clip(&format!("{app}: {title}")))
            .arg(clip(body))
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
