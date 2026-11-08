// macOS page notifications. WKWebView denies the Web Notifications API without private
// WebKit API, so GPUIdium provides it: pages create notifications as usual, they are queued
// here, and the host collects them by evaluating `__gpuidiumNotify.take()` (no channel from
// the page to native code). A click on the system notification calls `click(id)`, which
// fires the page's own click handler. The permission reflects the app's notification switch.
(() => {
  if (window.__gpuidiumNotify) return;
  let permission = "__GPUIDIUM_PERMISSION__";
  const queue = [];
  const live = new Map();
  let seq = 0;
  const text = (v, n) => String(v == null ? "" : v).slice(0, n);
  const push = (title, opts) => {
    const id = ++seq;
    queue.push({ id, title: text(title, 200), body: text(opts && opts.body, 500) });
    if (queue.length > 50) queue.shift();
    return id;
  };

  class GPUIdiumNotification extends EventTarget {
    constructor(title, opts = {}) {
      super();
      this.title = text(title, 200);
      this.body = text(opts.body, 500);
      this.tag = text(opts.tag, 200);
      this.icon = text(opts.icon, 2000);
      this.data = opts.data;
      this.onclick = this.onshow = this.onclose = this.onerror = null;
      if (permission !== "granted") {
        queueMicrotask(() => this._fire("error"));
        return;
      }
      this._id = push(title, opts);
      live.set(this._id, this);
      if (live.size > 100) live.delete(live.keys().next().value);
      queueMicrotask(() => this._fire("show"));
    }
    _fire(type) {
      const ev = new Event(type, { cancelable: true });
      const handler = this["on" + type];
      try {
        if (typeof handler === "function") handler.call(this, ev);
      } catch (_) {}
      this.dispatchEvent(ev);
    }
    close() {
      live.delete(this._id);
      this._fire("close");
    }
    static get permission() {
      return permission;
    }
    static requestPermission(callback) {
      const p = Promise.resolve(permission);
      if (typeof callback === "function") p.then(callback);
      return p;
    }
    static get maxActions() {
      return 0;
    }
  }
  Object.defineProperty(window, "Notification", {
    value: GPUIdiumNotification,
    writable: true,
    configurable: true,
  });

  if (window.ServiceWorkerRegistration) {
    const proto = ServiceWorkerRegistration.prototype;
    proto.showNotification = function (title, opts) {
      if (permission !== "granted") return Promise.reject(new TypeError("Notifications are off"));
      push(title, opts);
      return Promise.resolve();
    };
    proto.getNotifications = () => Promise.resolve([]);
  }

  if (navigator.permissions && navigator.permissions.query) {
    const query = navigator.permissions.query.bind(navigator.permissions);
    navigator.permissions.query = (desc) =>
      desc && desc.name === "notifications"
        ? Promise.resolve({
            name: "notifications",
            state: permission === "granted" ? "granted" : "denied",
            onchange: null,
            addEventListener() {},
            removeEventListener() {},
          })
        : query(desc);
  }

  Object.defineProperty(window, "__gpuidiumNotify", {
    value: Object.freeze({
      take: () => JSON.stringify(queue.splice(0)),
      click: (id) => {
        const n = live.get(id);
        window.focus();
        if (n) n._fire("click");
      },
      setPermission: (on) => {
        permission = on ? "granted" : "denied";
      },
    }),
  });
})();
