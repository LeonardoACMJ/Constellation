"""Native preview stress samples; isolated config only, no targets launched."""
import ctypes as C
from ctypes import wintypes as W
import json
from pathlib import Path
import shutil
import subprocess
import time
import sys
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/"scripts"))
from desktop_fixture import require_interactive, find_window
require_interactive()

ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/".local/editor-stress";OUT.mkdir(parents=True,exist_ok=True)
EXE=OUT/"constellation.exe";shutil.copy2(ROOT/"target/release/constellation.exe",EXE)
U=C.windll.user32
U.FindWindowW.argtypes=[W.LPCWSTR,W.LPCWSTR];U.FindWindowW.restype=W.HWND
U.PostMessageW.argtypes=[W.HWND,W.UINT,C.c_size_t,C.c_ssize_t]
U.GetForegroundWindow.restype=W.HWND
U.SetForegroundWindow.argtypes=[W.HWND]
previous=U.GetForegroundWindow()
def star(i):return {"id":i,"name":f"Star {i}","target":{"type":"folder","path":"."}}
def balanced(depth,width,counter):
    counter[0]+=1;i=counter[0]
    return star(i) if depth==0 else {"id":i,"name":f"Branch {i}","children":[balanced(depth-1,width,counter) for _ in range(width)]}
cases=[("flat-10000",{"id":1,"name":"10,000 Stars","children":[star(i) for i in range(2,10002)]}),
       ("balanced-4681",balanced(4,8,[0])),("deep-64",balanced(63,1,[0]))]
results=[]
try:
    for name,tree in cases:
        config={"version":1,"root":tree,"profiles":[{"id":1,"name":"Default","binding":"Ctrl+Q","mode":"click","hover":True,"restore_cursor":True}],
                "layout":{"radius":144,"distance":.52,"spread":148,"gap":0},"sound":{"enabled":False,"volume":.18}}
        (OUT/"data").mkdir(exist_ok=True)
        (OUT/"data/config.json").write_text(json.dumps(config),encoding="utf-8")
        report=OUT/"data/preview-measurement.json";report.unlink(missing_ok=True)
        process=subprocess.Popen([str(EXE),"--configure","--measure-preview"],creationflags=subprocess.CREATE_NO_WINDOW,stdin=subprocess.DEVNULL)
        then=time.monotonic();measurement=None
        try:
            deadline=then+30
            while time.monotonic()<deadline:
                if process.poll() is not None:raise AssertionError(f"{name} editor exited: {process.returncode}")
                if report.exists():
                    try:measurement=json.loads(report.read_text());break
                    except json.JSONDecodeError:pass
                time.sleep(.025)
            assert measurement is not None,f"{name}: no completed preview within test timeout"
            measurement.update(case=name,startup_to_report_ms=round((time.monotonic()-then)*1000,2))
            results.append(measurement)
            hwnd=find_window("Constellation.Config",{process.pid});assert hwnd
            U.PostMessageW(hwnd,0x10,0,0);process.wait(timeout=5)
            assert process.returncode==0
            print(json.dumps(measurement),flush=True)
        finally:
            if process.poll() is None:process.terminate();process.wait(timeout=5)
    (OUT/"results.json").write_text(json.dumps(results,indent=2))
finally:
    if previous:U.SetForegroundWindow(previous)
