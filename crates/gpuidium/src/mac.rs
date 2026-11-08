//! macOS platform services: page notifications in Notification Center, the Dock badge,
//! and WebKit's spell-checking switch.

use std::sync::OnceLock;

use async_channel::Sender;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject, ProtocolObject};
use objc2::{ClassType as _, MainThreadMarker, define_class, msg_send};
use objc2_app_kit::NSApplication;
use objc2_foundation::{NSBundle, NSError, NSObjectProtocol, NSString, NSUserDefaults};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
    UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse,
    UNNotificationSound, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};

use gpuidium_core::{AppId, diagnostics::log};

/// A click on one of our notifications: the app and the page's notification id.
pub type NotificationClick = (AppId, u64);

static CLICKS: OnceLock<Sender<NotificationClick>> = OnceLock::new();

/// Notification Center only works for a bundled app (`task package:macos`); a bare binary
/// has no bundle id and `UNUserNotificationCenter` would raise an exception.
fn bundled() -> bool {
    NSBundle::mainBundle().bundleIdentifier().is_some()
}

define_class!(
    // Callbacks arrive on a private queue, so this class is usable from any thread.
    #[unsafe(super(NSObject))]
    #[name = "GPUIdiumNotificationDelegate"]
    struct NotificationDelegate;

    unsafe impl NSObjectProtocol for NotificationDelegate {}

    unsafe impl UNUserNotificationCenterDelegate for NotificationDelegate {
        /// Show banners even while GPUIdium is the front app (the page you look at is often
        /// not the one that notifies); macOS hides them by default.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            handler: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            handler.call((UNNotificationPresentationOptions::Banner
                | UNNotificationPresentationOptions::List
                | UNNotificationPresentationOptions::Sound,));
        }

        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            handler: &block2::DynBlock<dyn Fn()>,
        ) {
            let id = response.notification().request().identifier().to_string();
            if let (Some(click), Some(tx)) = (parse_request_id(&id), CLICKS.get()) {
                let _ = tx.try_send(click);
            }
            handler.call(());
        }
    }
);

fn request_id(app: AppId, note: u64) -> String {
    format!("{app}|{note}")
}

fn parse_request_id(id: &str) -> Option<NotificationClick> {
    let (app, note) = id.split_once('|')?;
    Some((AppId::parse(app)?, note.parse().ok()?))
}

/// Set up Notification Center once: delegate (front-app banners, click routing) and the
/// permission prompt (macOS asks the user the first time).
pub fn init_notifications(clicks: Sender<NotificationClick>) {
    if !bundled() || CLICKS.set(clicks).is_err() {
        return;
    }
    let center = UNUserNotificationCenter::currentNotificationCenter();
    let delegate: Retained<NotificationDelegate> =
        unsafe { msg_send![NotificationDelegate::class(), new] };
    center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    // The center holds its delegate weakly; keep ours for the life of the process.
    std::mem::forget(delegate);
    let done = RcBlock::new(|granted: Bool, _err: *mut NSError| {
        log(&format!(
            "notification permission granted: {}",
            granted.as_bool()
        ));
    });
    center.requestAuthorizationWithOptions_completionHandler(
        UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
        &done,
    );
}

/// Show one page notification. Bundled: Notification Center, grouped per app, click routed
/// back via `init_notifications`. Unbundled (`task run`): `osascript` as a fallback (no click).
pub fn notify(app: AppId, app_name: &str, note: u64, title: &str, body: &str) -> bool {
    if !bundled() {
        // Text goes in as arguments, never into the script source.
        return std::process::Command::new("osascript")
            .args([
                "-e",
                "on run argv",
                "-e",
                "display notification (item 3 of argv) with title (item 1 of argv) subtitle (item 2 of argv)",
                "-e",
                "end run",
                app_name,
                title,
                body,
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok();
    }
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(app_name));
    content.setSubtitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    content.setThreadIdentifier(&NSString::from_str(&app.to_string()));
    content.setSound(Some(&UNNotificationSound::defaultSound()));
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &NSString::from_str(&request_id(app, note)),
        &content,
        None,
    );
    let done = RcBlock::new(|err: *mut NSError| {
        if !err.is_null() {
            log("notification request failed");
        }
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .addNotificationRequest_withCompletionHandler(&request, Some(&done));
    true
}

/// Dock icon badge ("3", "99+", "•"), or none.
pub fn set_dock_badge(label: Option<&str>) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let tile = NSApplication::sharedApplication(mtm).dockTile();
    tile.setBadgeLabel(label.map(NSString::from_str).as_deref());
}

/// Hand navigation keys straight to a focused web page.
///
/// Arrows, Home/End, Page Up/Down, Forward Delete and F-keys carry AppKit's "function" flag,
/// so the window offers them to GPUI's view as key equivalents first. GPUI passes them to the
/// text-input system, which then talks to the focused web view as a text client: the page
/// got `moveUp:`-style commands or the key's raw character inserted (control characters,
/// or U+F700-F703 shown as "no glyph" boxes) but never a `keydown` event, so pickers like
/// Mattermost's Ctrl/Cmd+K could not be browsed with Up/Down. While a web view is the first
/// responder, this monitor delivers such keys (without Cmd, so app shortcuts stay with GPUI)
/// to the web view's `keyDown:` and drops them from normal dispatch. WebKit sends a key the
/// page did not handle back through the application (the same event object); that copy is
/// dropped too, otherwise the text system would insert its control character.
pub fn install_page_key_monitor() {
    use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSEventType};
    use std::ptr::NonNull;

    let Some(web_view_class) = objc2::runtime::AnyClass::get(c"WKWebView") else {
        log("page keys: WKWebView class missing");
        return;
    };
    let last: std::cell::RefCell<Option<Retained<NSEvent>>> = std::cell::RefCell::new(None);
    let handler = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        let ev = unsafe { event.as_ref() };
        let flags = ev.modifierFlags();
        if ev.r#type() != NSEventType::KeyDown
            || !flags.contains(NSEventModifierFlags::Function)
            || flags.contains(NSEventModifierFlags::Command)
        {
            return event.as_ptr();
        }
        // Local monitors run on the main thread.
        let Some(mtm) = MainThreadMarker::new() else {
            return event.as_ptr();
        };
        let Some(responder) = ev.window(mtm).and_then(|w| w.firstResponder()) else {
            return event.as_ptr();
        };
        if !responder.isKindOfClass(web_view_class) {
            return event.as_ptr();
        }
        let mut last = last.borrow_mut();
        if last.as_deref().is_some_and(|l| std::ptr::eq(l, ev)) {
            // WebKit's resend of a key the page left unhandled.
            *last = None;
            return std::ptr::null_mut();
        }
        // Kept retained, so a later event cannot reuse the address.
        *last = Some(objc2::Message::retain(ev));
        drop(last);
        let _: () = unsafe { msg_send![&*responder, keyDown: ev] };
        std::ptr::null_mut()
    });
    let monitor = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &handler)
    };
    // The monitor lives for the whole process.
    std::mem::forget(monitor);
}

const SPELL_KEY: &str = "WebContinuousSpellCheckingEnabled";

/// WebKit's "check spelling while typing" switch. WebKit reads it when the first web view
/// starts, so this must run before any view exists; `toggle_spellcheck` changes it later.
pub fn set_spellcheck_default(on: bool) {
    NSUserDefaults::standardUserDefaults().setBool_forKey(on, &NSString::from_str(SPELL_KEY));
}

/// Current WebKit spell-checking state (WebKit writes the key back when it toggles).
pub fn spellcheck_default() -> bool {
    NSUserDefaults::standardUserDefaults().boolForKey(&NSString::from_str(SPELL_KEY))
}

/// Switch spell checking for running views: WebKit keeps the state process-wide, so one
/// `toggleContinuousSpellChecking:` on any view flips it for all of them.
pub fn toggle_spellcheck(view: &NSObject) {
    let nil: Option<&NSObject> = None;
    let _: () = unsafe { msg_send![view, toggleContinuousSpellChecking: nil] };
}

/// Open the system's spelling settings (languages macOS detects automatically).
pub fn open_spelling_settings() {
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.Keyboard-Settings.extension")
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_ids_round_trip() {
        let app = AppId::new();
        assert_eq!(parse_request_id(&request_id(app, 42)), Some((app, 42)));
        assert_eq!(parse_request_id("garbage"), None);
        assert_eq!(parse_request_id("not-a-uuid|1"), None);
    }
}
