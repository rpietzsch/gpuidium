//! App icon, embedded as PNG and handed to GPUI as the window icon.
//!
//! GPUI sets it on the X11 window when the window is created, before it is mapped, so the
//! window manager has the icon from the first frame (setting it afterwards left the default
//! "X" visible for a moment). Other platforms ignore the option.
//!
//! macOS takes the Dock / Cmd+Tab icon from the app bundle (`task package:macos`); for
//! unbundled runs (`task run`) `set_dock_icon` sets it at runtime.

use std::sync::Arc;

const ICON_128: &[u8] = include_bytes!("../assets/icon-128.png");

pub fn window_icon() -> Option<Arc<image::RgbaImage>> {
    image::load_from_memory_with_format(ICON_128, image::ImageFormat::Png)
        .ok()
        .map(|i| Arc::new(i.to_rgba8()))
}

/// macOS, unbundled only: show the app icon in the Dock and the Cmd+Tab switcher. A bundle
/// already has it from `AppIcon.icns`. The padded artwork follows Apple's icon grid
/// (`scripts/make-macos-icon.sh`).
#[cfg(target_os = "macos")]
pub fn set_dock_icon() {
    use objc2::{AllocAnyThread as _, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::{NSBundle, NSData};
    const ICON_MACOS: &[u8] = include_bytes!("../assets/icon-macos-512.png");
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    if NSBundle::mainBundle().bundleIdentifier().is_some() {
        return;
    }
    if let Some(image) = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(ICON_MACOS)) {
        // Safety: called on the main thread with a valid image.
        unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image)) };
    }
}
