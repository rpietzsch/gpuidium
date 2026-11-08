// macOS favicon (UX-011): WKWebView has no favicon API, so the page's own WebKit
// loads the icon (profile cookies, cache and CSP apply; no GPUIdium network fetch)
// and redraws it as a 64x64 PNG. The host reads the result later with
// favicon_read.js; pages get no channel to native code.
(() => {
  const K = "__gpuidiumFavicon";
  const st = window[K] || (window[K] = { seq: 0, out: "" });
  const my = ++st.seq;
  const sizeOf = (l) => {
    const s = (l.getAttribute("sizes") || "").toLowerCase();
    if (s === "any") return 64;
    const n = Math.max(0, ...s.split(/\s+/).map((v) => parseInt(v, 10) || 0));
    if (n) return n;
    return /apple-touch/i.test(l.rel) ? 180 : 32;
  };
  const links = [
    ...document.querySelectorAll(
      'link[rel~="icon" i], link[rel="apple-touch-icon" i], link[rel="apple-touch-icon-precomposed" i]'
    ),
  ]
    .filter((l) => l.href)
    .map((l) => ({ u: l.href, s: sizeOf(l) }));
  // Smallest icon of at least 32 px first, then the largest of the smaller ones.
  links.sort((a, b) =>
    (a.s >= 32) !== (b.s >= 32) ? (a.s >= 32 ? -1 : 1) : a.s >= 32 ? a.s - b.s : b.s - a.s
  );
  const urls = links.map((l) => l.u);
  urls.push(new URL("/favicon.ico", location.origin).href);
  const draw = (src) =>
    new Promise((ok, no) => {
      const i = new Image();
      i.crossOrigin = "anonymous";
      i.onload = () => {
        try {
          const c = document.createElement("canvas");
          c.width = c.height = 64;
          c.getContext("2d").drawImage(i, 0, 0, 64, 64);
          ok(c.toDataURL("image/png"));
        } catch (e) {
          no(e);
        }
      };
      i.onerror = no;
      i.src = src;
    });
  const viaFetch = async (u) => {
    const r = await fetch(u);
    if (!r.ok) throw new Error("status " + r.status);
    const o = URL.createObjectURL(await r.blob());
    try {
      return await draw(o);
    } finally {
      URL.revokeObjectURL(o);
    }
  };
  (async () => {
    for (const u of [...new Set(urls)]) {
      for (const how of [draw, viaFetch]) {
        try {
          const data = atob((await how(u)).split(",")[1]);
          let hex = "";
          for (let i = 0; i < data.length; i++) hex += data.charCodeAt(i).toString(16).padStart(2, "0");
          if (my === st.seq) st.out = hex;
          return;
        } catch (_) {}
      }
    }
  })();
})();
