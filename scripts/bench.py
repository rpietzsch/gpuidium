#!/usr/bin/env python3
"""PERF-001 (Linux): process-tree memory/CPU baseline for GPUIdium.

Serves three deterministic local pages, runs the app with an isolated XDG tree and N apps
(N in 0/1/3/5), switches through all N so every WebView is created, then samples the whole
process tree (PSS from /proc/<pid>/smaps_rollup, CPU from /proc/<pid>/stat) for --seconds.
Requires a display (X11/XWayland), xdotool, and a built binary (`task build:release`).
Reports p50/p95 over --runs. Raw JSON goes to stdout.
"""
import argparse, json, os, shutil, statistics, subprocess, tempfile, threading, time, uuid
from http.server import BaseHTTPRequestHandler, HTTPServer

PAGES = {f"/p{i}": f"<html><title>page {i}</title><body><h1>page {i}</h1></body></html>" for i in range(1, 4)}


class H(BaseHTTPRequestHandler):
    def do_GET(self):
        body = PAGES.get(self.path.split("?")[0], PAGES["/p1"]).encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/html")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *a):
        pass


def tree(pid):
    out, todo = [], [pid]
    while todo:
        p = todo.pop()
        out.append(p)
        try:
            with open(f"/proc/{p}/task/{p}/children") as f:
                todo += [int(x) for x in f.read().split()]
        except OSError:
            pass
    return out


def pss_kb(pids):
    total = 0
    for p in pids:
        try:
            with open(f"/proc/{p}/smaps_rollup") as f:
                for line in f:
                    if line.startswith("Pss:"):
                        total += int(line.split()[1])
        except OSError:
            pass
    return total


def cpu_ticks(pids):
    t = 0
    for p in pids:
        try:
            parts = open(f"/proc/{p}/stat").read().rsplit(")", 1)[1].split()
            t += int(parts[11]) + int(parts[12])
        except (OSError, IndexError):
            pass
    return t


def run(binary, n, port, seconds):
    root = tempfile.mkdtemp(prefix="gpuidium-bench-")
    cfg_home, data_home = os.path.join(root, "cfg"), os.path.join(root, "data")
    os.makedirs(os.path.join(cfg_home, "GPUIdium"))
    apps = [
        {"id": str(uuid.uuid4()), "name": f"bench {i}", "start_url": f"http://127.0.0.1:{port}/p{i % 3 + 1}",
         "profile_id": str(uuid.uuid4()), "order": i}
        for i in range(n)
    ]
    cfg = {"schema_version": 1, "apps": apps, "active_app_id": apps[0]["id"] if apps else None,
           "window": {"width": 1200, "height": 800}}
    json.dump(cfg, open(os.path.join(cfg_home, "GPUIdium", "config.json"), "w"))
    env = dict(os.environ, XDG_CONFIG_HOME=cfg_home, XDG_DATA_HOME=data_home, XDG_CACHE_HOME=os.path.join(root, "cache"))
    t0 = time.time()
    proc = subprocess.Popen([binary], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        time.sleep(6)
        startup = time.time() - t0
        win = subprocess.run(["xdotool", "search", "--sync", "--pid", str(proc.pid)], capture_output=True, text=True, timeout=20).stdout.split()
        for i in range(1, n + 1):  # create every view
            if win:
                subprocess.run(["xdotool", "windowactivate", "--sync", win[-1]], capture_output=True)
                subprocess.run(["xdotool", "mousemove", "--window", win[-1], "600", "400", "click", "1"], capture_output=True)
                subprocess.run(["xdotool", "key", "--clearmodifiers", f"ctrl+{i}"], capture_output=True)
            time.sleep(1.5)
        time.sleep(3)
        pids = tree(proc.pid)
        mem = pss_kb(pids)
        c0, w0 = cpu_ticks(pids), time.time()
        time.sleep(seconds)
        pids2 = tree(proc.pid)
        cpu = (cpu_ticks(pids2) - c0) / os.sysconf("SC_CLK_TCK") / (time.time() - w0) * 100
        return {"apps": n, "pss_mb": round(max(mem, pss_kb(pids2)) / 1024, 1), "processes": len(pids2),
                "cpu_pct_idle": round(cpu, 2), "alive": proc.poll() is None, "settle_s": round(startup, 1)}
    finally:
        proc.terminate()
        try:
            proc.wait(5)
        except subprocess.TimeoutExpired:
            proc.kill()
        shutil.rmtree(root, ignore_errors=True)


def pct(xs, q):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(round(q * (len(xs) - 1))))]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--binary", default="target/release/gpuidium")
    ap.add_argument("--workloads", default="0,1,3,5")
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--seconds", type=int, default=20)
    a = ap.parse_args()
    srv = HTTPServer(("127.0.0.1", 0), H)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    result = {"host": os.uname().release, "runs": []}
    for n in [int(x) for x in a.workloads.split(",")]:
        rs = [run(a.binary, n, srv.server_port, a.seconds) for _ in range(a.runs)]
        result["runs"] += rs
        result[f"summary_{n}"] = {
            "pss_mb_p50": pct([r["pss_mb"] for r in rs], .5), "pss_mb_p95": pct([r["pss_mb"] for r in rs], .95),
            "cpu_pct_p50": pct([r["cpu_pct_idle"] for r in rs], .5), "cpu_pct_p95": pct([r["cpu_pct_idle"] for r in rs], .95),
            "all_alive": all(r["alive"] for r in rs)}
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
