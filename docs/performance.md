# Performance investigation (Linux, 2026-11-07)

Method: the real app driven with `xdotool`, a test page reporting `requestAnimationFrame` rate, visibility and focus, process-tree CPU and PSS from `/proc`, and the opt-in log (`GPUIDIUM_LOG=1`, now with millisecond timestamps). Machine: 120 Hz display, Intel Iris Xe + NVIDIA GTX 1650 (hybrid), KDE Wayland session, app on XWayland, WebKitGTK 2.54.2. Release build unless stated. Numbers are from one machine; treat them as a baseline, not a guarantee.

## What was measured, and what it shows

| Finding | Evidence | Status |
|---|---|---|
| **WebKitGTK itself animates at about 32 fps here** (display is 120 Hz) | Same test page: GPUIdium 31.9 fps, **Epiphany (stock WebKitGTK, native Wayland) 32.5 fps**, Firefox 114 fps. Not changed by GTK poll interval (1-16 ms), NVIDIA offload, DMABUF renderer off or compositing off | Engine/platform limit, not our embedding. Native Wayland would not fix it. Only a different engine would (RND-005) |
| Events from WebKit were handled on a 100 ms timer | Key press to page visible: 107 ms median (39-152) before, **58 ms median (20-63) after**; key press to page focused 76-115 ms before, 20-28 ms after | **Fixed**: events now wake the UI immediately (`async-channel`) |
| Web view bounds and visibility re-applied on every shell frame (move, resize, relayout, map) | Hovering the sidebar caused about 20 redundant calls per second; CPU 15.6% to 12.1% during a hover sweep | **Fixed**: `lb-wry` skips unchanged values (see `third_party/lb-wry/PATCHES.md`) |
| Page never focused after switching (caret invisible) | `document.hasFocus()` was false after every switch | **Fixed** earlier (see `docs/verification-linux.md`) |
| Debug builds are much slower to start | First render 1.3 s (debug) vs 0.77 s (release); page loaded 1.7 s vs 1.1 s | `task run` now builds release; `task run:debug` for debugging |
| Startup is dominated by GPUI/Vulkan setup | 0.61 s to a constructed shell, 0.15 s for the first web view, page loaded at 1.1 s. Limiting Vulkan to the Intel GPU saves about 90 ms | Not changed (not ours; forcing a GPU is not safe) |
| GTK is pumped by a 16 ms timer inside `gpui-webview` | Faster polling raised CPU steeply (10.6% at 16 ms to 23% at 1 ms) with no fps gain; slower polling (64 ms) saves only about 1% idle CPU | Left as is |
| Memory per app | Total PSS 261 MB (1 app), 409 MB (3), 558 MB (5): about **75 MB per additional app** (one web + one network process each, a consequence of per-profile isolation); the shell itself is about 131 MB | By design. "Unload inactive apps" frees idle apps |
| Deferring the first web view until after the first paint | Sidebar 130 ms sooner, but page ready 77 ms **later** | Tried and rejected |

## Not reproduced
Pages repainting about two seconds after switching apps (reported on real sites). With test pages, including a 15k-node page with a title changing four times a second, WebKit produced a frame 100-200 ms after the switch and the screen showed the right page by the first sample (about 0.9 s). The `known-limits.md` entry has an A/B recipe for narrowing it down.

## Ideas, not done
- Replace the GTK timer with a GLib main-context integration (event-driven, zero idle wakeups). Real but small gain (about 1% CPU); needs a patched `gpui-webview` and careful threading.
- Keep recently used views mapped instead of hiding them, to avoid WebKit dropping painted layers. Needs a heavy real-world page to prove a benefit.
- A different web engine for smoothness (CEF/Servo-style off-screen rendering): large cost; see RND-005.
