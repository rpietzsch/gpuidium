# GPUIdium

A lightweight, keyboard-friendly [GPUI](https://gpui.rs) shell for web apps. Paste a URL, get a persistent sidebar entry with its own isolated browser profile. No recipe catalog, no telemetry, no account.

**Status:** pre-alpha, in the feasibility phase. See [`tasks/spec.md`](tasks/spec.md) and [`tasks/backlog.md`](tasks/backlog.md).

## Prerequisites

- Rust, pinned in `rust-toolchain.toml` (installed automatically by `rustup`).
- [go-task](https://taskfile.dev) (`task`).
- **Linux (X11 / XWayland only; native Wayland is not supported):**
  GTK 3, WebKitGTK 4.1, X11/xkbcommon and Vulkan loader libraries.
  - Arch: `sudo pacman -S gtk3 webkit2gtk-4.1 libxkbcommon-x11 vulkan-icd-loader`
  - Debian/Ubuntu: `sudo apt install libgtk-3-dev libwebkit2gtk-4.1-dev libxkbcommon-x11-dev libvulkan1`
  - On a Wayland session the app is forced onto XWayland; `Xwayland` must be installed.
- **Windows 11:** WebView2 runtime (preinstalled on Windows 11).
- **macOS:** per-app profile isolation needs macOS 14+ (see ADR-001).

## Tasks

```text
task              # list targets
task build        # debug build
task build:release
task run
task test
task fmt
task lint         # clippy, warnings denied
task check        # fmt check + lint + test (the CI gate)
task clean
task test:isolation  # needs a display: same-origin profile isolation + restart persistence
task test:oidc       # needs a display: mock OIDC login survives restart, stays per-profile
task bench           # process-tree memory/CPU baseline (needs display + xdotool)
task package:linux   # dist/gpuidium-<ver>-linux-x86_64.tar.gz (+ .sha256)
task release:linux   # package + checksum + optional GPG signature + verification (docs/updates.md)
```

See [`docs/known-limits.md`](docs/known-limits.md) for privacy, isolation limits and login behaviour,
and [`docs/security-review.md`](docs/security-review.md).

## Layout

- `crates/gpuidium-core` - GUI-free domain logic: model, URL policy, storage (fully unit-tested).
- `crates/gpuidium` - GPUI application and feasibility spikes (`src/bin/spike_*.rs`).
- `third_party/lb-wry` - vendored Wry fork with a per-profile cache/HSTS patch (`PATCHES.md`).
- `docs/` - ADRs, limits, security review.
- `scripts/`, `packaging/` - benchmark harness, Linux packaging.

## License

Apache-2.0
