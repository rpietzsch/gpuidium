//! Linux OIDC login-flow test: does a typical authorization-code login survive an app restart?
//!
//! Spins up a mock OpenID Provider (IdP) and Relying Party (RP) on two loopback ports, loads the
//! RP in one WebView backed by a persistent per-profile store, and records what happened:
//! redirects (RP -> IdP -> RP), a real HTML login form POST, `state` cookie check, code
//! exchange, session cookies. The login form auto-submits, standing in for a user typing.
//!
//! Usage: spike_oidc <data-root> [--profile NAME] [--cookies persistent|session]
//!                   [--expect rp|sso|login] [--wait SECS]
//! Events seen, in order, are printed. `--expect` selects the outcome required for exit 0:
//!   rp    RP session cookie survived (first request is already authenticated)
//!   sso   RP session gone but the IdP session cookie survived (silent re-login)
//!   login the user must log in again (nothing survived)
//! Exit: 0 = matches expectation (or no --expect given), 1 = mismatch, 2 = harness error.

use std::{
    collections::HashSet,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled, Window,
    WindowOptions, div,
};
use gpui_webview::WebView;
use gpuidium_core::{
    ProfileId,
    paths::profile_dir,
    policy::{NavDecision, decide_navigation},
};
use wry::{WebContext, WebViewBuilder};

struct Shared {
    events: Mutex<Vec<String>>,
    codes: Mutex<HashSet<String>>,
    rp_secret: String,
    idp_secret: String,
    persistent: bool,
    rp_port: u16,
    idp_port: u16,
}

impl Shared {
    fn log(&self, e: &str) {
        self.events.lock().unwrap().push(e.to_owned());
    }
}

struct Req {
    method: String,
    path: String,
    query: Vec<(String, String)>,
    cookies: Vec<(String, String)>,
    form: Vec<(String, String)>,
}

fn cookie<'a>(r: &'a Req, k: &str) -> Option<&'a str> {
    r.cookies
        .iter()
        .find(|(n, _)| n == k)
        .map(|(_, v)| v.as_str())
}

fn qget(v: &[(String, String)], k: &str) -> String {
    v.iter()
        .find(|(n, _)| n == k)
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

fn parse(stream: &TcpStream) -> Option<Req> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_owned();
    let target = parts.next()?.to_owned();
    let (mut len, mut cookies) = (0usize, Vec::new());
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).ok()? <= 2 {
            break;
        }
        let lower = h.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            len = v.trim().parse().unwrap_or(0);
        } else if lower.starts_with("cookie:") {
            for c in h["cookie:".len()..].trim().split("; ") {
                if let Some((k, v)) = c.split_once('=') {
                    cookies.push((k.to_owned(), v.trim().to_owned()));
                }
            }
        }
    }
    let mut body = vec![0u8; len.min(1 << 16)];
    reader.read_exact(&mut body).ok()?;
    let u = url::Url::parse(&format!("http://x{target}")).ok()?;
    Some(Req {
        method,
        path: u.path().to_owned(),
        query: u.query_pairs().into_owned().collect(),
        cookies,
        form: url::form_urlencoded::parse(&body).into_owned().collect(),
    })
}

fn reply(mut s: TcpStream, status: &str, headers: &[(&str, String)], body: &str) {
    let mut out = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n",
        body.len()
    );
    for (k, v) in headers {
        out += &format!("{k}: {v}\r\n");
    }
    out += "\r\n";
    let _ = s.write_all(out.as_bytes());
    let _ = s.write_all(body.as_bytes());
}

fn redirect(s: TcpStream, to: &str, cookies: &[String]) {
    let mut h = vec![("Location", to.to_owned())];
    h.extend(cookies.iter().map(|c| ("Set-Cookie", c.clone())));
    reply(s, "302 Found", &h, "");
}

fn rp_uri(sh: &Shared) -> String {
    format!("http://127.0.0.1:{}/callback", sh.rp_port)
}

fn session_cookie(sh: &Shared, name: &str, val: &str) -> String {
    let age = if sh.persistent { "; Max-Age=86400" } else { "" };
    let _ = sh;
    format!("{name}={val}; Path=/; HttpOnly; SameSite=Lax{age}")
}

fn issue_code(sh: &Shared, redirect_uri: &str, state: &str) -> String {
    let code = ProfileId::new().to_string();
    sh.codes.lock().unwrap().insert(code.clone());
    let mut u = url::Url::parse(redirect_uri).unwrap();
    u.query_pairs_mut()
        .append_pair("code", &code)
        .append_pair("state", state);
    u.to_string()
}

fn rp(s: TcpStream, r: Req, sh: &Shared) {
    match r.path.as_str() {
        "/" => {
            if cookie(&r, "rp_sid") == Some(sh.rp_secret.as_str()) {
                sh.log("rp_authed");
                reply(s, "200 OK", &[], "<h1>Hello alice (RP session)</h1>");
            } else {
                sh.log("rp_unauth");
                let state = ProfileId::new().to_string();
                let mut u = url::Url::parse(&format!("http://127.0.0.1:{}/authorize", sh.idp_port))
                    .unwrap();
                u.query_pairs_mut()
                    .append_pair("response_type", "code")
                    .append_pair("client_id", "rp")
                    .append_pair("redirect_uri", &rp_uri(sh))
                    .append_pair("scope", "openid profile")
                    .append_pair("state", &state)
                    .append_pair("nonce", &ProfileId::new().to_string());
                // Short-lived session cookie binding the flow to this browser (CSRF check).
                let c = format!("oidc_state={state}; Path=/; HttpOnly; SameSite=Lax");
                redirect(s, u.as_str(), &[c]);
            }
        }
        "/callback" => {
            let (code, state) = (qget(&r.query, "code"), qget(&r.query, "state"));
            if cookie(&r, "oidc_state") != Some(state.as_str()) {
                sh.log("rp_state_mismatch");
                return reply(s, "400 Bad Request", &[], "state mismatch");
            }
            if !sh.codes.lock().unwrap().remove(&code) {
                sh.log("rp_bad_code");
                return reply(s, "400 Bad Request", &[], "bad code");
            }
            sh.log("rp_callback_ok");
            let clear = "oidc_state=; Path=/; Max-Age=0".to_owned();
            redirect(
                s,
                "/",
                &[session_cookie(sh, "rp_sid", &sh.rp_secret), clear],
            );
        }
        _ => reply(s, "404 Not Found", &[], "nope"),
    }
}

const LOGIN: &str = r#"<!doctype html><meta charset=utf-8><title>Sign in</title>
<form name=f method=post action=/login>
<input type=hidden name=state value="@STATE@"><input type=hidden name=redirect_uri value="@RU@">
<input name=user><input name=pass type=password></form>
<script>setTimeout(()=>{f.user.value='alice';f.pass.value='s3cret';f.submit()},300)</script>"#;

fn idp(s: TcpStream, r: Req, sh: &Shared) {
    match (r.method.as_str(), r.path.as_str()) {
        ("GET", "/authorize") => {
            let (ru, state) = (qget(&r.query, "redirect_uri"), qget(&r.query, "state"));
            if ru != rp_uri(sh) || qget(&r.query, "client_id") != "rp" {
                sh.log("idp_bad_client");
                return reply(s, "400 Bad Request", &[], "bad client");
            }
            if cookie(&r, "idp_sid") == Some(sh.idp_secret.as_str()) {
                sh.log("idp_sso_silent");
                redirect(s, &issue_code(sh, &ru, &state), &[]);
            } else {
                sh.log("idp_login_form");
                reply(
                    s,
                    "200 OK",
                    &[],
                    &LOGIN.replace("@STATE@", &state).replace("@RU@", &ru),
                );
            }
        }
        ("POST", "/login") => {
            let (ru, state) = (qget(&r.form, "redirect_uri"), qget(&r.form, "state"));
            if qget(&r.form, "user") == "alice"
                && qget(&r.form, "pass") == "s3cret"
                && ru == rp_uri(sh)
            {
                sh.log("idp_login_ok");
                let c = session_cookie(sh, "idp_sid", &sh.idp_secret);
                redirect(s, &issue_code(sh, &ru, &state), &[c]);
            } else {
                sh.log("idp_login_denied");
                reply(s, "401 Unauthorized", &[], "denied");
            }
        }
        _ => reply(s, "404 Not Found", &[], "nope"),
    }
}

/// Stable value stored in `root/<name>`, created on first use.
fn stable(root: &Path, name: &str) -> String {
    let f = root.join(name);
    if let Ok(t) = std::fs::read_to_string(&f) {
        return t.trim().to_owned();
    }
    let v = ProfileId::new().to_string();
    std::fs::create_dir_all(root).unwrap();
    std::fs::write(&f, &v).unwrap();
    v
}

/// Reuse ports across restarts so origins (and thus cookies/storage scopes) stay identical.
fn listener(root: &Path, name: &str) -> TcpListener {
    let pf = root.join(name);
    let prev = std::fs::read_to_string(&pf)
        .ok()
        .and_then(|t| t.trim().parse::<u16>().ok());
    let l = prev
        .and_then(|p| TcpListener::bind(("127.0.0.1", p)).ok())
        .unwrap_or_else(|| TcpListener::bind("127.0.0.1:0").expect("bind"));
    std::fs::create_dir_all(root).unwrap();
    std::fs::write(&pf, l.local_addr().unwrap().port().to_string()).unwrap();
    l
}

fn spawn_server(l: TcpListener, sh: Arc<Shared>, f: fn(TcpStream, Req, &Shared)) {
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let sh = sh.clone();
            std::thread::spawn(move || {
                if let Some(r) = parse(&s) {
                    f(s, r, &sh);
                }
            });
        }
    });
}

struct View(Entity<WebView>, #[allow(dead_code)] WebContext);

impl Render for View {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}

/// Same binding as `host::bind_profile_store`: on macOS WKWebView ignores the
/// `WebContext` directory, so each profile needs its own `WKWebsiteDataStore`.
fn bind_store(b: WebViewBuilder<'_>, profile: ProfileId) -> WebViewBuilder<'_> {
    #[cfg(target_os = "macos")]
    {
        use wry::WebViewBuilderExtDarwin as _;
        b.with_data_store_identifier(profile.as_uuid().into_bytes())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = profile;
        b
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().expect("usage: spike_oidc <data-root> [flags]"));
    let rest: Vec<String> = args.collect();
    let flag = |name: &str| {
        rest.iter()
            .position(|a| a == name)
            .and_then(|i| rest.get(i + 1).cloned())
    };
    let persistent = flag("--cookies").as_deref() != Some("session");
    let expect = flag("--expect");
    let wait: u64 = flag("--wait").and_then(|v| v.parse().ok()).unwrap_or(3);
    let profile_name = flag("--profile").unwrap_or_else(|| "default".into());

    #[cfg(target_os = "linux")]
    {
        gtk::gdk::set_allowed_backends("x11");
        gtk::init().expect("gtk init (is DISPLAY set / Xwayland running?)");
    }

    let (lrp, lidp) = (listener(&root, "rp-port"), listener(&root, "idp-port"));
    let sh = Arc::new(Shared {
        events: Mutex::default(),
        codes: Mutex::default(),
        rp_secret: stable(&root, "rp-secret"),
        idp_secret: stable(&root, "idp-secret"),
        persistent,
        rp_port: lrp.local_addr().unwrap().port(),
        idp_port: lidp.local_addr().unwrap().port(),
    });
    spawn_server(lrp, sh.clone(), rp);
    spawn_server(lidp, sh.clone(), idp);

    let pid = ProfileId::parse(&stable(&root, &format!("profile-{profile_name}"))).unwrap();
    let start = format!("http://127.0.0.1:{}/", sh.rp_port);

    #[cfg(target_os = "linux")]
    let app = gpui_kit::platform::linux(gpui_kit::WindowingModes::X11);
    #[cfg(not(target_os = "linux"))]
    let app = gpui_kit::application();

    app.run(move |cx| {
        gpui_kit::init(cx);
        let dir = profile_dir(&root, pid);
        let (_, view) = gpui_kit::open_window(WindowOptions::default(), cx, move |window, cx| {
            std::fs::create_dir_all(&dir).unwrap();
            let mut ctx = WebContext::new(Some(dir));
            let b = bind_store(WebViewBuilder::new_with_web_context(&mut ctx), pid)
                .with_url(start)
                .with_navigation_handler(|u| decide_navigation(&u) == NavDecision::Allow);
            let w = WebView::build(b, window, cx).expect("webview");
            let e = cx.new(|_| w);
            cx.new(|_| View(e, ctx))
        })
        .expect("window");
        let _keep = view;

        let sh = sh.clone();
        cx.spawn(async move |cx| {
            let mut done = false;
            for _ in 0..250 {
                if sh.events.lock().unwrap().iter().any(|e| e == "rp_authed") {
                    done = true;
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
            }
            // Give WebKit time to flush cookie storage to disk before the process ends.
            cx.background_executor()
                .timer(Duration::from_secs(wait))
                .await;
            let ev = sh.events.lock().unwrap().clone();
            println!("events: {}", ev.join(" -> "));
            let outcome = if !done {
                "TIMEOUT"
            } else if ev.first().map(String::as_str) == Some("rp_authed") {
                "rp"
            } else if ev.iter().any(|e| e == "idp_login_form") {
                "login"
            } else if ev.iter().any(|e| e == "idp_sso_silent") {
                "sso"
            } else {
                "unknown"
            };
            println!("outcome: {outcome}");
            let code = match (&expect, outcome) {
                (_, "TIMEOUT") => 2,
                (Some(want), got) if want != got => 1,
                _ => 0,
            };
            std::process::exit(code);
        })
        .detach();
        cx.activate(true);
    });
}
