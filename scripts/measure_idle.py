"""One lightweight idle check, isolated beneath .local; no external packages."""
import ctypes as C
from ctypes import wintypes as W
import json
from pathlib import Path
import shutil
import subprocess
import time
from desktop_fixture import require_interactive, quit_resident
require_interactive()

root=Path(__file__).resolve().parents[1]
folder=root/".local/idle-check";folder.mkdir(parents=True,exist_ok=True)
exe=folder/"constellation.exe";shutil.copy2(root/"target/release/constellation.exe",exe)
subprocess.run([str(exe),"--check-config"],check=True,timeout=5,creationflags=subprocess.CREATE_NO_WINDOW,stdin=subprocess.DEVNULL)
U,K,P=C.windll.user32,C.windll.kernel32,C.windll.psapi
U.FindWindowW.argtypes=[W.LPCWSTR,W.LPCWSTR];U.FindWindowW.restype=W.HWND
U.PostMessageW.argtypes=[W.HWND,W.UINT,C.c_size_t,C.c_ssize_t]
K.OpenProcess.argtypes=[W.DWORD,W.BOOL,W.DWORD];K.OpenProcess.restype=W.HANDLE
K.CloseHandle.argtypes=[W.HANDLE]
class Memory(C.Structure):
    _fields_=[("cb",W.DWORD),("faults",W.DWORD)]+[(name,C.c_size_t) for name in ["peak_working_set","working_set","peak_paged_pool","paged_pool","peak_nonpaged_pool","nonpaged_pool","pagefile","peak_pagefile","private_bytes"]]
P.GetProcessMemoryInfo.argtypes=[W.HANDLE,C.POINTER(Memory),W.DWORD]
K.GetProcessTimes.argtypes=[W.HANDLE,C.POINTER(W.FILETIME),C.POINTER(W.FILETIME),C.POINTER(W.FILETIME),C.POINTER(W.FILETIME)]
def cpu(handle):
    times=[W.FILETIME() for _ in range(4)]
    if not K.GetProcessTimes(handle,*[C.byref(t) for t in times]):raise OSError("GetProcessTimes failed")
    return sum((t.dwHighDateTime<<32)|t.dwLowDateTime for t in times[2:])/1e7
process=subprocess.Popen([str(exe)],creationflags=subprocess.CREATE_NO_WINDOW,stdin=subprocess.DEVNULL)
handle=None
try:
    time.sleep(.5)
    if process.poll() is not None:raise RuntimeError("Resident exited before idle measurement")
    handle=K.OpenProcess(0x410,False,process.pid)
    start=cpu(handle);print("Measuring 60 seconds idle in tray…",flush=True);then=time.monotonic()
    time.sleep(60)
    elapsed=time.monotonic()-then
    memory=Memory();memory.cb=C.sizeof(memory)
    if not P.GetProcessMemoryInfo(handle,C.byref(memory),C.sizeof(memory)):raise OSError("Memory query failed")
    result={"idle_seconds":round(elapsed,2),"working_set_MiB":round(memory.working_set/1048576,2),"private_MiB":round(memory.private_bytes/1048576,2),"cpu_seconds":round(cpu(handle)-start,4),"conditions":"Release x64; default sample tree; radial/editor closed; isolated portable folder"}
    (folder/"measurement.json").write_text(json.dumps(result,indent=2))
    print(json.dumps(result),flush=True)
finally:
    quit_resident(process)
    try:process.wait(timeout=5)
    except subprocess.TimeoutExpired:process.terminate();process.wait(timeout=5)
    if handle:K.CloseHandle(handle)
