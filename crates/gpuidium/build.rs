//! Windows (MSVC): embed the app icon as icon resource 1, which GPUI loads for the window
//! and taskbar and Explorer shows for the .exe. The `.res` file is written directly from
//! the PNG assets (PNG icon entries, Windows Vista+), so no resource compiler is needed.

use std::path::PathBuf;

const SIZES: [u32; 3] = [32, 64, 128];

fn main() {
    for size in SIZES {
        println!("cargo:rerun-if-changed=assets/icon-{size}.png");
    }
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if !(windows && msvc) {
        return;
    }
    let pngs: Vec<Vec<u8>> = SIZES
        .iter()
        .map(|s| std::fs::read(format!("assets/icon-{s}.png")).expect("icon PNG"))
        .collect();
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("icon.res");
    std::fs::write(&out, icon_res(&pngs)).expect("write icon.res");
    // Only the app binaries; the spike/test binaries keep the default icon either way.
    println!("cargo:rustc-link-arg-bins={}", out.display());
}

const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;

/// A `.res` file: an empty header entry, one RT_ICON per image (ids 1..), and RT_GROUP_ICON 1.
fn icon_res(pngs: &[Vec<u8>]) -> Vec<u8> {
    let mut res = Vec::new();
    entry(&mut res, 0, 0, 0, &[]);
    for (i, png) in pngs.iter().enumerate() {
        entry(&mut res, RT_ICON, i as u16 + 1, 0x1010, png);
    }
    let mut group = Vec::new();
    group.extend_from_slice(&0u16.to_le_bytes()); // reserved
    group.extend_from_slice(&1u16.to_le_bytes()); // type: icon
    group.extend_from_slice(&(pngs.len() as u16).to_le_bytes());
    for (i, (png, size)) in pngs.iter().zip(SIZES).enumerate() {
        let dim = if size >= 256 { 0 } else { size as u8 };
        group.extend_from_slice(&[dim, dim, 0, 0]); // width, height, colours, reserved
        group.extend_from_slice(&1u16.to_le_bytes()); // planes
        group.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        group.extend_from_slice(&(png.len() as u32).to_le_bytes());
        group.extend_from_slice(&(i as u16 + 1).to_le_bytes()); // RT_ICON id
    }
    entry(&mut res, RT_GROUP_ICON, 1, 0x1030, &group);
    res
}

/// One resource with numeric type and name, language neutral, padded to 4 bytes.
fn entry(res: &mut Vec<u8>, kind: u16, name: u16, flags: u16, data: &[u8]) {
    let header_size = 32u32;
    res.extend_from_slice(&(data.len() as u32).to_le_bytes());
    res.extend_from_slice(&header_size.to_le_bytes());
    res.extend_from_slice(&[0xff, 0xff]);
    res.extend_from_slice(&kind.to_le_bytes());
    res.extend_from_slice(&[0xff, 0xff]);
    res.extend_from_slice(&name.to_le_bytes());
    res.extend_from_slice(&0u32.to_le_bytes()); // data version
    res.extend_from_slice(&flags.to_le_bytes());
    res.extend_from_slice(&0u16.to_le_bytes()); // language: neutral
    res.extend_from_slice(&0u32.to_le_bytes()); // version
    res.extend_from_slice(&0u32.to_le_bytes()); // characteristics
    res.extend_from_slice(data);
    while !res.len().is_multiple_of(4) {
        res.push(0);
    }
}
