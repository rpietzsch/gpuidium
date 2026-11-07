//! App icon, embedded as PNG and handed to GPUI as the window icon.
//!
//! GPUI sets it on the X11 window when the window is created, before it is mapped, so the
//! window manager has the icon from the first frame (setting it afterwards left the default
//! "X" visible for a moment). Other platforms ignore the option for now.

use std::sync::Arc;

const ICON_128: &[u8] = include_bytes!("../assets/icon-128.png");

pub fn window_icon() -> Option<Arc<image::RgbaImage>> {
    image::load_from_memory_with_format(ICON_128, image::ImageFormat::Png)
        .ok()
        .map(|i| Arc::new(i.to_rgba8()))
}
