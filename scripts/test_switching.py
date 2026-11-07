#!/usr/bin/env python3
"""Regression test for switching apps (needs a display and xdotool; ~25 s).

Runs GPUIdium with three complex local pages (20k nodes each) that report their viewport,
focus and resize events, switches between them with Ctrl+1/2/3 and asserts:
  * the page is never a small square (GTK resets a re-shown wrapper window to 200x200),
  * a switch causes no relayout (no resize events) and the page is focused afterwards,
  * resizing the window still resizes the page.
Usage: scripts/test_switching.py   (BIN=path overrides target/release/gpuidium)
"""

import http.server, json, os, re, subprocess, sys, tempfile, threading, time, uuid
env_extra = dict(a.split("=", 1) for a in sys.argv[1:])
PAGE = """<html><body style="background:#eef;margin:0"><h1>{n}</h1><div id=big></div><script>
let h='';for(let i=0;i<20000;i++)h+='<div style="display:inline-block;width:'+(30+i%40)+'px;height:14px;margin:1px;background:hsl('+(i%360)+',70%,70%);border-radius:4px"></div>';
document.getElementById('big').innerHTML=h;
const r=(ev)=>navigator.sendBeacon('/report?ev='+ev+'&page={n}&iw='+innerWidth+'&ih='+innerHeight+'&f='+document.hasFocus()+'&t='+Date.now());
document.addEventListener('visibilitychange',()=>{{ if(!document.hidden){{ r('vis'); requestAnimationFrame(()=>r('raf')); }} }});
window.addEventListener('resize',()=>r('resize')); r('load');
</script></body></html>"""
events = []
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        n = self.path.strip("/").removesuffix(".html"); body = PAGE.format(n=n).encode()
        self.send_response(200); self.send_header("Content-Type","text/html"); self.send_header("Content-Length",str(len(body))); self.end_headers(); self.wfile.write(body)
    def do_POST(self):
        self.send_response(204); self.end_headers()
        m = re.search(r"ev=(\w+)&page=(\w+)&iw=(\d+)&ih=(\d+)&f=(\w+)&t=(\d+)", self.path)
        if m: events.append((int(m.group(6)), m.group(1), m.group(2), int(m.group(3)), int(m.group(4)), m.group(5) == 'true'))
    def log_message(self,*a): pass
srv = http.server.HTTPServer(("127.0.0.1", 0), H); threading.Thread(target=srv.serve_forever, daemon=True).start()
port = srv.server_port
def key(k): subprocess.run(["xdotool","key",f"ctrl+{k}"])
def wait(pred, timeout):
    t0=time.time()
    while time.time()-t0<timeout:
        if pred(): return True
        time.sleep(0.05)
    return False
with tempfile.TemporaryDirectory() as root:
    os.makedirs(f"{root}/cfg/GPUIdium")
    names=["alpha","beta","gamma"]
    apps=[{"id":str(uuid.uuid4()),"name":n,"start_url":f"http://127.0.0.1:{port}/{n}.html","profile_id":str(uuid.uuid4()),"order":i} for i,n in enumerate(names)]
    json.dump({"schema_version":1,"apps":apps,"active_app_id":apps[0]["id"],"window":{"width":1200,"height":800}},open(f"{root}/cfg/GPUIdium/config.json","w"))
    env=dict(os.environ,XDG_CONFIG_HOME=f"{root}/cfg",XDG_DATA_HOME=f"{root}/data",XDG_CACHE_HOME=f"{root}/cache",**env_extra)
    app=subprocess.Popen([os.environ.get("BIN","target/release/gpuidium")],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    try:
        wait(lambda: any(e[1]=="load" and e[2]=="alpha" for e in events), 20); time.sleep(1.5)
        for k,n in ((2,"beta"),(3,"gamma"),(1,"alpha")):          # warm-up: create the other views
            key(k); wait(lambda: any(e[1]=="load" and e[2]==n for e in events), 10); time.sleep(1.2)
        time.sleep(2)
        rows=[]
        for k in (2,3,1,2,3,1,2,3,1):
            t=int(time.time()*1000); key(k); time.sleep(1.5)
            w=[e for e in events if e[0]>=t]
            raf=[e for e in w if e[1]=="raf"]
            rows.append((sum(1 for e in w if e[1]=="resize"), (raf[0][3],raf[0][4]) if raf else None, (raf[0][0]-t) if raf else None, raf[0][5] if raf else False))
        win = None
        for w_id in subprocess.run(["xdotool","search","--pid",str(app.pid)],capture_output=True,text=True).stdout.split():
            g = dict(l.split("=") for l in subprocess.run(["xdotool","getwindowgeometry","--shell",w_id],capture_output=True,text=True).stdout.split())
            if int(g["WIDTH"])*int(g["HEIGHT"]) > 500000: win = w_id
        before = len(events)
        subprocess.run(["xdotool","windowsize",win,"1000","700"]); time.sleep(2.5)
        g = dict(l.split("=") for l in subprocess.run(["xdotool","getwindowgeometry","--shell",win],capture_output=True,text=True).stdout.split())
        win_w = int(g["WIDTH"])
        resized = [e for e in events[before:] if e[1]=="resize"]
    finally:
        app.terminate()
        try: app.wait(5)
        except subprocess.TimeoutExpired: app.kill()

tiny = sum(1 for r in rows if r[1] and (r[1][0] < 400 or r[1][1] < 400))
relayouts = sum(r[0] for r in rows)
unfocused = sum(1 for r in rows if not r[3])
lat = sorted(r[2] for r in rows if r[2] is not None)
print("relayouts per switch:", [r[0] for r in rows])
print("viewport at first frame:", sorted({r[1] for r in rows if r[1]}), f"| tiny: {tiny} of {len(rows)} | unfocused: {unfocused}")
print("key -> first frame (ms): median", lat[len(lat)//2] if lat else None)
ok = len(rows) == 9 and tiny == 0 and relayouts == 0 and unfocused == 0 and all(r[1] for r in rows)
if resized:
    _, _, _, w, h, _ = resized[-1]
    print(f"window resized to width {win_w}: page viewport {w}x{h} (expected width about {win_w - 40})")
    ok = ok and abs(w - (win_w - 40)) <= 4
else:
    print("no resize event after resizing the window"); ok = False
print("RESULT:", "PASS" if ok else "FAIL")
sys.exit(0 if ok else 1)
