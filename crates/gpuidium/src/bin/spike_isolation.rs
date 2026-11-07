//! SPIKE-004/005 (Linux X11/XWayland): two same-origin WebViews with distinct
//! persistent profiles inside one GPUI window.
//!
//! A tiny loopback HTTP server serves one page for both views (identical origin).
//! The page writes a per-profile value to cookie, localStorage and IndexedDB and
//! reports back what it could read *before* writing. The harness then checks:
//!   * no view ever observes the other profile's value (isolation)
//!   * the second load of each view observes its own value (persistence)
//!   * with `--expect-persisted` (a later process run on the same data root),
//!     the very first load already observes its own value (restart persistence).
//!
//! Usage: spike_isolation <data-root> [--expect-persisted]
//! Exit code 0 = PASS, 1 = FAIL, 2 = harness error/timeout.

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled, Window,
    WindowOptions, div,
};
use gpui_webview::WebView;
use gpuidium_core::{ProfileId, paths::profile_dir};
use wry::{WebContext, WebViewBuilder};

#[derive(Debug, Clone)]
struct Report {
    who: String,
    cookie: String,
    ls: String,
    idb: String,
}

type Reports = Arc<Mutex<Vec<Report>>>;

const PAGE: &str = r#"<!doctype html><meta charset=utf-8><title>spike</title>
<body style="font:20px sans-serif"><h2 id=h></h2><script>
(async () => {
  const q = new URLSearchParams(location.search);
  const who = q.get('who'), val = q.get('val');
  document.getElementById('h').textContent = 'profile ' + who;
  document.body.style.background = who === 'A' ? '#dff' : '#fdf';
  const cookie0 = (document.cookie.split('; ').find(c => c.startsWith('sid=')) || '').slice(4);
  const ls0 = localStorage.getItem('sid') || '';
  const db = await new Promise(res => {
    const r = indexedDB.open('spike', 1);
    r.onupgradeneeded = () => r.result.createObjectStore('s');
    r.onsuccess = () => res(r.result); r.onerror = () => res(null);
  });
  const idb0 = db ? await new Promise(res => {
    const g = db.transaction('s').objectStore('s').get('sid');
    g.onsuccess = () => res(g.result || ''); g.onerror = () => res('');
  }) : 'unsupported';
  if (!cookie0) document.cookie = 'sid=' + val + '; max-age=86400; path=/';
  if (!ls0) localStorage.setItem('sid', val);
  if (db && !idb0) await new Promise(res => {
    const t = db.transaction('s', 'readwrite'); t.objectStore('s').put(val, 'sid'); t.oncomplete = () => res();
  });
  await fetch('/report?' + new URLSearchParams({ who, cookie: cookie0, ls: ls0, idb: idb0 }));
})();
</script>"#;

fn query(line: &str) -> Vec<(String, String)> {
    let path = line.split_whitespace().nth(1).unwrap_or("");
    let qs = path.split_once('?').map(|x| x.1).unwrap_or("");
    qs.split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.replace("%3A", ":").replace("%2D", "-")))
        .collect()
}

fn serve(stream: TcpStream, reports: Reports) {
    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    // Drain headers.
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).unwrap_or(0) <= 2 {
            break;
        }
    }
    let mut stream = stream;
    let path = line.split_whitespace().nth(1).unwrap_or("/");
    let (ctype, body) = if path.starts_with("/report") {
        let q = query(&line);
        let get = |k: &str| {
            q.iter()
                .find(|(a, _)| a == k)
                .map(|x| x.1.clone())
                .unwrap_or_default()
        };
        reports.lock().unwrap().push(Report {
            who: get("who"),
            cookie: get("cookie"),
            ls: get("ls"),
            idb: get("idb"),
        });
        ("text/plain", String::from("ok"))
    } else {
        ("text/html", PAGE.to_owned())
    };
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: {ctype}; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut sink = Vec::new();
    let _ = reader.read_to_end(&mut sink);
}

/// Reuse the previous run's port so the origin (scheme+host+port) is stable across
/// restarts; localStorage/IndexedDB are origin-scoped and would otherwise look "lost".
fn start_server(root: &std::path::Path) -> (u16, Reports) {
    let pf = root.join("spike-port.txt");
    let prev = std::fs::read_to_string(&pf)
        .ok()
        .and_then(|t| t.trim().parse::<u16>().ok());
    let listener = prev
        .and_then(|p| TcpListener::bind(("127.0.0.1", p)).ok())
        .unwrap_or_else(|| TcpListener::bind("127.0.0.1:0").expect("bind"));
    let port = listener.local_addr().unwrap().port();
    let _ = std::fs::create_dir_all(root);
    let _ = std::fs::write(&pf, port.to_string());
    let reports: Reports = Arc::default();
    let r = reports.clone();
    std::thread::spawn(move || {
        for s in listener.incoming().flatten() {
            let r = r.clone();
            std::thread::spawn(move || serve(s, r));
        }
    });
    (port, reports)
}

/// Stable per-instance profile ids so a second process run reuses the same profiles.
fn profile_ids(root: &std::path::Path) -> (ProfileId, ProfileId) {
    let f = root.join("spike-ids.txt");
    if let Ok(t) = std::fs::read_to_string(&f) {
        let v: Vec<_> = t.lines().collect();
        if v.len() == 2
            && let (Some(a), Some(b)) = (ProfileId::parse(v[0]), ProfileId::parse(v[1]))
        {
            return (a, b);
        }
    }
    let (a, b) = (ProfileId::new(), ProfileId::new());
    std::fs::create_dir_all(root).unwrap();
    std::fs::write(&f, format!("{a}\n{b}\n")).unwrap();
    (a, b)
}

struct Spike {
    a: Entity<WebView>,
    b: Entity<WebView>,
    _contexts: Vec<WebContext>,
}

impl Render for Spike {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .child(div().flex_1().h_full().child(self.a.clone()))
            .child(div().flex_1().h_full().child(self.b.clone()))
    }
}

fn verdict(reports: &[Report], vals: &[(&str, String)], expect_persisted: bool) -> (bool, String) {
    let mut ok = true;
    let mut out = String::new();
    for (who, own) in vals {
        let mine: Vec<_> = reports.iter().filter(|r| r.who == *who).collect();
        out += &format!("profile {who} ({own}):\n");
        for (i, r) in mine.iter().enumerate() {
            out += &format!(
                "  load {}: cookie={:?} localStorage={:?} indexedDB={:?}\n",
                i + 1,
                r.cookie,
                r.ls,
                r.idb
            );
            for v in [&r.cookie, &r.ls, &r.idb] {
                // Anything non-empty must be our own value (or the idb marker).
                if !v.is_empty() && v != own && v != "unsupported" {
                    ok = false;
                    out += "  !! LEAK: observed a foreign value\n";
                }
            }
        }
        // Persistence: the *last* load must see own cookie + localStorage.
        match mine.last() {
            Some(r) if r.cookie == *own && r.ls == *own => {}
            _ => {
                ok = false;
                out += "  !! no persistence of cookie/localStorage\n";
            }
        }
        if let Some(last) = mine.last() {
            out += &format!(
                "  indexedDB persisted: {}\n",
                if last.idb == *own { "yes" } else { "NO" }
            );
            if last.idb != *own && last.idb != "unsupported" {
                ok = false;
            }
        }
        if expect_persisted {
            match mine.first() {
                Some(r) if r.cookie == *own && r.ls == *own && (r.idb == *own) => {
                    out += "  restart persistence: yes\n";
                }
                _ => {
                    ok = false;
                    out += "  !! restart persistence: NO\n";
                }
            }
        }
    }
    (ok, out)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(
        args.next()
            .expect("usage: spike_isolation <data-root> [--expect-persisted]"),
    );
    let expect_persisted = args.any(|a| a == "--expect-persisted");

    // FINDING: WebContext::new() touches WebKitGTK, so GTK must already be initialized.
    // gpui-webview only does that inside WebView::build(), which is too late for
    // per-profile contexts. Init on the X11 backend first (gpui-webview then sees it done).
    gtk::gdk::set_allowed_backends("x11");
    gtk::init().expect("gtk init (is DISPLAY set / Xwayland running?)");

    let (port, reports) = start_server(&root);
    let (pa, pb) = profile_ids(&root);
    let val_a = format!("A-{pa}");
    let val_b = format!("B-{pb}");
    let url = move |who: &str, val: &str, n: u32| {
        format!("http://127.0.0.1:{port}/?who={who}&val={val}&n={n}")
    };

    gpui_kit::platform::linux(gpui_kit::WindowingModes::X11).run(move |cx| {
        gpui_kit::init(cx);
        let (ua, ub) = (url("A", &val_a, 1), url("B", &val_b, 1));
        let (da, db) = (profile_dir(&root, pa), profile_dir(&root, pb));
        let (_, spike_entity) =
            gpui_kit::open_window(WindowOptions::default(), cx, move |window, cx| {
                std::fs::create_dir_all(&da).unwrap();
                std::fs::create_dir_all(&db).unwrap();
                let mut ctx_a = WebContext::new(Some(da));
                let mut ctx_b = WebContext::new(Some(db));
                let wa = WebView::build(
                    WebViewBuilder::new_with_web_context(&mut ctx_a).with_url(ua),
                    window,
                    cx,
                )
                .expect("webview A");
                let wb = WebView::build(
                    WebViewBuilder::new_with_web_context(&mut ctx_b).with_url(ub),
                    window,
                    cx,
                )
                .expect("webview B");
                let a = cx.new(|_| wa);
                let b = cx.new(|_| wb);
                cx.new(|_| Spike {
                    a,
                    b,
                    _contexts: vec![ctx_a, ctx_b],
                })
            })
            .expect("open window");

        let reports = reports.clone();
        let (v_a, v_b) = (val_a.clone(), val_b.clone());
        cx.spawn(async move |cx| {
            let wait_for = async |n: usize, cx: &mut gpui_kit::AsyncApp| -> bool {
                for _ in 0..300 {
                    if reports.lock().unwrap().len() >= n {
                        return true;
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(100))
                        .await;
                }
                false
            };
            // Phase 1: first loads of both views.
            if !wait_for(2, cx).await {
                eprintln!("TIMEOUT waiting for first loads (is the window on X11/XWayland?)");
                std::process::exit(2);
            }
            // Phase 2: reload both from the same profiles; must see own values.
            let (u2a, u2b) = (url("A", &v_a, 2), url("B", &v_b, 2));
            spike_entity.update(cx, |spike, cx| {
                spike.a.update(cx, |w, _| w.load_url(&u2a));
                spike.b.update(cx, |w, _| w.load_url(&u2b));
            });
            if !wait_for(4, cx).await {
                eprintln!("TIMEOUT waiting for second loads");
                std::process::exit(2);
            }
            let r = reports.lock().unwrap().clone();
            let (ok, text) = verdict(
                &r,
                &[("A", v_a.clone()), ("B", v_b.clone())],
                expect_persisted,
            );
            println!("{text}RESULT: {}", if ok { "PASS" } else { "FAIL" });
            std::process::exit(if ok { 0 } else { 1 });
        })
        .detach();
        cx.activate(true);
    });
}
