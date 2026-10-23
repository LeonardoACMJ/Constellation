"""Windows-only unattended smoke checks against an isolated portable copy.

Run after `cargo build --release`: python tests/native_smoke.py
Temporarily creates its own foreground input-probe window, then restores cursor
and foreground window. No browsers or user programs are launched. Artifacts and
runtime config stay in .local/native-smoke. Pillow is optional (screenshots only).
"""
import ctypes as C
from ctypes import wintypes as W
import json
from pathlib import Path
import shutil
import subprocess
import sys
import time

if sys.platform != "win32":
    raise SystemExit("This smoke test requires a Windows desktop session")
ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / ".local" / "native-smoke"
OUT.mkdir(parents=True, exist_ok=True)
EXE = OUT / "constellation.exe"
shutil.copy2(ROOT / "target/release/constellation.exe", EXE)
U, G, K = C.windll.user32, C.windll.gdi32, C.windll.kernel32
U.SetProcessDpiAwarenessContext(C.c_void_p(-4))
HWND, UINT, WP, LP, LR = W.HWND, W.UINT, C.c_size_t, C.c_ssize_t, C.c_ssize_t
PROC = C.WINFUNCTYPE(LR, HWND, UINT, WP, LP)

class WC(C.Structure):
    _fields_ = [("style", UINT), ("proc", PROC), ("extra", C.c_int), ("winextra", C.c_int), ("instance", W.HINSTANCE), ("icon", W.HICON), ("cursor", W.HANDLE), ("brush", W.HBRUSH), ("menu", W.LPCWSTR), ("name", W.LPCWSTR)]
class MOUSE(C.Structure):
    _fields_ = [("dx", W.LONG), ("dy", W.LONG), ("data", W.DWORD), ("flags", W.DWORD), ("time", W.DWORD), ("extra", WP)]
class KEY(C.Structure):
    _fields_ = [("vk", W.WORD), ("scan", W.WORD), ("flags", W.DWORD), ("time", W.DWORD), ("extra", WP)]
class UNION(C.Union):
    _fields_ = [("mouse", MOUSE), ("key", KEY)]
class INPUT(C.Structure):
    _fields_ = [("type", W.DWORD), ("data", UNION)]
class MONINFO(C.Structure):
    _fields_ = [("size", W.DWORD), ("monitor", W.RECT), ("work", W.RECT), ("flags", W.DWORD)]

for name, args, restype in [
    ("FindWindowW", [W.LPCWSTR, W.LPCWSTR], HWND),
    ("CreateWindowExW", [W.DWORD,W.LPCWSTR,W.LPCWSTR,W.DWORD,C.c_int,C.c_int,C.c_int,C.c_int,HWND,W.HMENU,W.HINSTANCE,C.c_void_p], HWND),
    ("DefWindowProcW", [HWND,UINT,WP,LP], LR),
    ("SendMessageW", [HWND,UINT,WP,LP], LR),
    ("PostMessageW", [HWND,UINT,WP,LP], W.BOOL),
    ("SetForegroundWindow", [HWND], W.BOOL),
    ("SetFocus", [HWND], HWND),
    ("IsWindowVisible", [HWND], W.BOOL),
    ("GetForegroundWindow", [], HWND),
    ("GetDlgItem", [HWND,C.c_int], HWND),
    ("GetWindowRect", [HWND,C.POINTER(W.RECT)], W.BOOL),
    ("SetWindowTextW", [HWND,W.LPCWSTR], W.BOOL),
    ("DestroyWindow", [HWND], W.BOOL),
    ("GetDC", [HWND], W.HDC),
    ("ReleaseDC", [HWND,W.HDC], C.c_int),
    ("MonitorFromPoint", [W.POINT,W.DWORD], W.HANDLE),
    ("GetMonitorInfoW", [W.HANDLE,C.POINTER(MONINFO)], W.BOOL),
    ("GetDpiForWindow", [HWND], UINT),
    ("SetWindowPos", [HWND,HWND,C.c_int,C.c_int,C.c_int,C.c_int,UINT], W.BOOL),
    ("PrintWindow", [HWND,W.HDC,UINT], W.BOOL),
    ("SendInput", [UINT,C.POINTER(INPUT),C.c_int], UINT),
]:
    fn=getattr(U,name); fn.argtypes=args; fn.restype=restype
K.GetModuleHandleW.argtypes=[W.LPCWSTR]; K.GetModuleHandleW.restype=W.HINSTANCE
G.CreateCompatibleDC.argtypes=[W.HDC]; G.CreateCompatibleDC.restype=W.HDC
G.CreateCompatibleBitmap.argtypes=[W.HDC,C.c_int,C.c_int]; G.CreateCompatibleBitmap.restype=W.HBITMAP
G.SelectObject.argtypes=[W.HDC,W.HANDLE]; G.SelectObject.restype=W.HANDLE
G.DeleteObject.argtypes=[W.HANDLE]; G.DeleteDC.argtypes=[W.HDC]
G.GetDIBits.argtypes=[W.HDC,W.HBITMAP,UINT,UINT,C.c_void_p,C.c_void_p,UINT]

old_cursor=W.POINT(); U.GetCursorPos(C.byref(old_cursor)); old_foreground=U.GetForegroundWindow()
observed=[]; processes=[]; checks=[]
@PROC
def probe_proc(hwnd,msg,w,l):
    if msg==0x100:
        observed.append(int(w))
    return U.DefWindowProcW(hwnd,msg,w,l)
wc=WC(); wc.proc=probe_proc; wc.instance=K.GetModuleHandleW(None); wc.name="Constellation.NativeTest.Probe"
U.RegisterClassW(C.byref(wc))
probe=U.CreateWindowExW(0,wc.name,"Constellation input probe",0x10CF0000,80,80,440,260,None,None,wc.instance,None)

def pump():
    msg=W.MSG()
    while U.PeekMessageW(C.byref(msg),None,0,0,1):
        U.TranslateMessage(C.byref(msg)); U.DispatchMessageW(C.byref(msg))
def wait(predicate, timeout=5):
    until=time.monotonic()+timeout
    while time.monotonic()<until:
        pump()
        if predicate(): return
        time.sleep(.015)
    raise AssertionError("Timed out waiting for native state")
def pause(seconds=.12):
    until=time.monotonic()+seconds
    while time.monotonic()<until: pump(); time.sleep(.01)
def keys(*events):
    array=(INPUT*len(events))()
    for i,(vk,up) in enumerate(events): array[i].type=1; array[i].data.key=KEY(vk,0,2 if up else 0,0,0)
    assert U.SendInput(len(array),array,C.sizeof(INPUT))==len(array)
def invoke(): keys((0x11,False),(0x51,False),(0x51,True),(0x11,True))
def mouse_side(down):
    event=INPUT();event.type=0;event.data.mouse=MOUSE(0,0,1,0x80 if down else 0x100,0,0)
    assert U.SendInput(1,C.byref(event),C.sizeof(INPUT))==1
def move_cursor(x,y):
    # Send actual virtual-desktop mouse input so the hook is exercised, too.
    left,top=U.GetSystemMetrics(76),U.GetSystemMetrics(77)
    width,height=U.GetSystemMetrics(78),U.GetSystemMetrics(79)
    event=INPUT();event.type=0;event.data.mouse=MOUSE(round((x-left)*65535/(width-1)),round((y-top)*65535/(height-1)),0,0xC001,0,0)
    assert U.SendInput(1,C.byref(event),C.sizeof(INPUT))==1
def point():
    p=W.POINT(); U.GetCursorPos(C.byref(p)); return p.x,p.y
def visible(hwnd): return bool(U.IsWindowVisible(hwnd))
def mark(name): checks.append(name); print("PASS:",name,flush=True)
def screenshot(hwnd,name):
    try: from PIL import Image
    except ImportError: return
    r=W.RECT(); U.GetWindowRect(hwnd,C.byref(r)); width,height=r.right-r.left,r.bottom-r.top
    dc=U.GetDC(hwnd); mem=G.CreateCompatibleDC(dc); bmp=G.CreateCompatibleBitmap(dc,width,height); old=G.SelectObject(mem,bmp)
    U.PrintWindow(hwnd,mem,2)
    info=C.create_string_buffer(40+256*4)
    import struct
    info[:40]=struct.pack("<IiiHHIIiiII",40,width,-height,1,32,0,width*height*4,0,0,0,0)
    pixels=C.create_string_buffer(width*height*4); G.GetDIBits(mem,bmp,0,height,pixels,info,0)
    image=Image.frombuffer("RGB",(width,height),pixels.raw,"raw","BGRX",0,1);image.save(OUT/name)
    G.SelectObject(mem,old); G.DeleteObject(bmp); G.DeleteDC(mem); U.ReleaseDC(hwnd,dc)
    return image

try:
    # Each run owns this exact isolated config file; keep runs independent.
    (OUT/"data/config.json").unlink(missing_ok=True)
    subprocess.run([str(EXE),"--check-config"],check=True,timeout=5)
    config_path=OUT/"data/config.json"; c=json.loads(config_path.read_text())
    marker=OUT/"launched.txt"
    if marker.exists(): marker.unlink()
    pythonw=Path(sys.executable).with_name("pythonw.exe")
    target={"type":"program","path":str(pythonw),"args":["-c",f"from pathlib import Path; Path({str(marker)!r}).write_text('launched')"]}
    c["root"]["children"]=[{"id":10,"name":"Native launch test","target":target},{"id":20,"name":"Nested","children":[{"id":21,"name":"Nested Star","target":target}]}]
    c["profiles"]=[c["profiles"][0],{"name":"Hold test","binding":"Ctrl+E","mode":"hold","hover":True,"restore_cursor":True}]
    c["layout"]={"radius":144,"distance":.52,"spread":148,"gap":2}
    c["sound"]["enabled"]=False
    config_path.write_text(json.dumps(c),encoding="utf-8")
    resident=subprocess.Popen([str(EXE)]); processes.append(resident)
    wait(lambda:U.FindWindowW("Constellation.Native.Radial",None))
    overlay=U.FindWindowW("Constellation.Native.Radial",None)
    U.SetForegroundWindow(probe); U.SetFocus(probe); pause()
    U.SetCursorPos(300,250); origin=point(); observed.clear(); invoke()
    wait(lambda:visible(overlay))
    center=point(); pause(); assert 0x51 not in observed, observed
    mark("global Ctrl+Q consumed while another app has focus")
    keys((0x11,False),(0x45,False),(0x45,True),(0x11,True)); pause(); assert visible(overlay)
    mark("another profile is ignored while the Click profile remains open")
    scale=U.GetDpiForWindow(overlay)/96
    U.SetCursorPos(center[0],round(center[1]-144*.65*scale)); pause(); screenshot(overlay,"radial.png")
    keys((0x20,False),(0x20,True)); wait(lambda:not visible(overlay));wait(marker.exists)
    assert point()==origin,(point(),origin)
    mark("Space launches one target and restores the saved cursor")
    U.SetForegroundWindow(probe);U.SetFocus(probe);pause();U.SetCursorPos(320,260);origin=point()
    keys((0x11,False),(0x45,False));wait(lambda:visible(overlay));keys((0x45,True),(0x11,True));wait(lambda:not visible(overlay));assert point()==origin
    mark("Hold release over the neutral hub cancels and restores cursor")
    U.SetForegroundWindow(probe);U.SetFocus(probe);pause()
    keys((0x11,False),(0x45,False));wait(lambda:visible(overlay));center=point()
    U.SetCursorPos(center[0],round(center[1]-144*.65*scale));pause()
    marker.unlink();keys((0x11,True),(0x45,True));wait(lambda:not visible(overlay));wait(marker.exists)
    mark("releasing a required modifier completes Hold selection")
    # Reload a profile and a portable relative script without restarting the resident.
    scripts=OUT/"scripts";scripts.mkdir(exist_ok=True)
    (scripts/"mark.py").write_text(f"from pathlib import Path\nPath({str(marker)!r}).write_text('script')\n")
    c["root"]["children"][0]["target"]={"type":"script","path":"scripts/mark.py","runner":str(pythonw)}
    c["profiles"].append({"name":"Mouse hold","binding":"Mouse4","mode":"hold","hover":True,"restore_cursor":True})
    c["profiles"].append({"name":"Single key hold","binding":"F8","mode":"hold","hover":True,"restore_cursor":True})
    config_path.write_text(json.dumps(c),encoding="utf-8");U.PostMessageW(overlay,0x8008,0,0);pause()
    U.SetForegroundWindow(probe);U.SetFocus(probe);pause();keys((0x77,False));wait(lambda:visible(overlay));keys((0x77,True));wait(lambda:not visible(overlay))
    mark("single-key F8 Hold opens and cancels on release")
    U.SetForegroundWindow(probe);U.SetFocus(probe);pause();marker.unlink()
    mouse_side(True);wait(lambda:visible(overlay));center=point();U.SetCursorPos(center[0],round(center[1]-144*.65*scale));pause();mouse_side(False)
    wait(lambda:not visible(overlay));wait(marker.exists);assert marker.read_text()=="script"
    mark("live profile reload, Mouse4 Hold, and executable-root-relative script launch")
    # A real nested branch; snapshot captures the illuminated path and connection.
    U.SetForegroundWindow(probe);U.SetFocus(probe);pause();invoke();wait(lambda:visible(overlay));center=point()
    move_cursor(center[0],round(center[1]+144*.65*scale));pause()
    child_y=round(center[1]+144*(.52+(.58+1.15)/2)*scale)
    move_cursor(center[0],child_y);pause();nested=screenshot(overlay,"nested-radial.png")
    if nested:
        r=W.RECT();U.GetWindowRect(overlay,C.byref(r));pixel=nested.getpixel((center[0]-r.left,child_y-r.top))
        assert pixel!=(0,0,0),"Expected a visible child fan outside the root circle"
    keys((0x1b,False),(0x1b,True));wait(lambda:not visible(overlay))
    mark("nested hover path renders and Escape restores the interaction")
    # A borderless test window spans the monitor. Confirm input passes through.
    U.SetForegroundWindow(probe);pause(); info=MONINFO();info.size=C.sizeof(info);U.GetMonitorInfoW(U.MonitorFromPoint(W.POINT(100,100),2),C.byref(info))
    # SetWindowLongPtrW is pointer-sized; GWL_STYLE still receives a 32-bit style value.
    U.SetWindowLongPtrW.argtypes=[HWND,C.c_int,LP];U.SetWindowLongPtrW.restype=LP
    U.SetWindowLongPtrW(probe,-16,0x90000000)
    r=info.monitor;U.SetWindowPos(probe,None,r.left,r.top,r.right-r.left,r.bottom-r.top,0x20);U.SetForegroundWindow(probe);U.SetFocus(probe);pause(.3)
    observed.clear();before=point();invoke();pause(.3);assert not visible(overlay);assert point()==before;assert 0x51 in observed,observed
    mark("fullscreen suppression leaves Ctrl+Q and cursor with the foreground app")
    U.SetWindowLongPtrW(probe,-16,0x10CF0000);U.SetWindowPos(probe,None,80,80,440,260,0x20);pause(.2)
    # Stop resident through its tray Quit command path, with no process killing.
    # This process is an isolated test instance; WM_DESTROY is the native exit path.
    editor=subprocess.Popen([str(EXE),"--configure"]);processes.append(editor)
    wait(lambda:U.FindWindowW("Constellation.Config",None));window=U.FindWindowW("Constellation.Config",None);pause(.3)
    screenshot(window,"configuration.png")
    # Graphical profiles edit a draft, capture configured keys without invoking
    # the resident, validate duplicate bindings, and propagate via normal Save.
    def text_at(parent, child):
        buffer=C.create_unicode_buffer(2048)
        U.SendMessageW(U.GetDlgItem(parent,child),0x000D,len(buffer),C.addressof(buffer))
        return buffer.value
    def set_text(parent, child, value):
        buffer=C.create_unicode_buffer(value)
        U.SendMessageW(U.GetDlgItem(parent,child),0x000C,0,C.addressof(buffer))
    def click(parent, child):
        control=U.GetDlgItem(parent,child)
        assert control, f"Missing button {child}"
        U.SendMessageW(control,0xF5,0,0);pause()
    click(window,404)
    wait(lambda:U.FindWindowW("Constellation.Profiles",None))
    profiles=U.FindWindowW("Constellation.Profiles",None)
    U.SetForegroundWindow(profiles);pause(.25)
    click(profiles,611);invoke();pause()
    assert text_at(profiles,603)=="Ctrl+Q",text_at(profiles,603)
    assert "Recorded" in text_at(profiles,612),text_at(profiles,612)
    assert not visible(overlay)
    mark("profile recorder receives existing Ctrl+Q without invoking the resident")
    click(profiles,607)  # creates a new profile with an unused function key
    set_text(profiles,602,"Recorded hold")
    set_text(profiles,603,"Ctrl+E");click(profiles,609)
    assert "already used" in text_at(profiles,612)
    assert len(json.loads(config_path.read_text())["profiles"])==4
    mark("duplicate profile bindings show an inline error without saving")
    click(profiles,611);mouse_side(True);mouse_side(False);pause()
    assert text_at(profiles,603)=="Mouse4",text_at(profiles,603)
    click(profiles,611);keys((0x1b,False),(0x1b,True));pause()
    assert text_at(profiles,603)=="Mouse4" and "cancelled" in text_at(profiles,612)
    click(profiles,611);keys((0x78,False),(0x78,True));pause()
    assert text_at(profiles,603)=="F9",text_at(profiles,603)
    U.SendMessageW(U.GetDlgItem(profiles,604),0x14E,1,0)
    for child in [605,606]: U.SendMessageW(U.GetDlgItem(profiles,child),0xF1,0,0)
    outer=W.RECT();button=W.RECT()
    U.GetWindowRect(profiles,C.byref(outer));U.GetWindowRect(U.GetDlgItem(profiles,610),C.byref(button))
    assert button.bottom < outer.bottom, "Profile actions must fit inside the window"
    screenshot(profiles,"shortcut-profiles.png")
    click(profiles,610);wait(lambda:not U.FindWindowW("Constellation.Profiles",None))
    click(window,401)
    saved=json.loads(config_path.read_text())["profiles"]
    assert saved[-1]=={"name":"Recorded hold","binding":"F9","mode":"hold","hover":False,"restore_cursor":False},saved[-1]
    mark("keyboard/mouse recording and per-profile options persist through Done and Save")
    U.SetForegroundWindow(probe);U.SetFocus(probe);pause(.25)
    U.SetCursorPos(320,260);keys((0x78,False));wait(lambda:visible(overlay));center=point()
    keys((0x78,True));wait(lambda:not visible(overlay));assert point()==center
    mark("graphically added F9 Hold profile reloads live with cursor restoration disabled")
    U.SetForegroundWindow(window);pause();click(window,404)
    profiles=U.FindWindowW("Constellation.Profiles",None)
    click(profiles,608);click(profiles,613)
    wait(lambda:not U.FindWindowW("Constellation.Profiles",None));click(window,401)
    assert json.loads(config_path.read_text())["profiles"]==saved
    mark("cancelling profile removals preserves the saved profiles")
    U.PostMessageW(overlay,0x2,0,0);resident.wait(timeout=5)
    mark("resident exits and releases its hooks")
    assert U.GetDlgItem(window,100) and U.GetDlgItem(window,501)
    new_radius=C.create_unicode_buffer("4000")
    U.SendMessageW(U.GetDlgItem(window,501),0x000C,0,C.addressof(new_radius))
    U.SendMessageW(window,0x111,401,0);pause(.2)
    if json.loads(config_path.read_text())["layout"]["radius"]!=4000:
        screenshot(window,"save-failure.png")
        error_file=OUT/"data/editor-error.txt"
        raise AssertionError(error_file.read_text() if error_file.exists() else "Save did not persist edited radius")
    screenshot(window,"overflow-warning.png")
    mark("configuration saves an overflowing layout without blocking")
    U.PostMessageW(window,0x10,0,0);editor.wait(timeout=5)
    mark("configuration closes normally")
    (OUT/"results.json").write_text(json.dumps({"checks":checks},indent=2))
finally:
    for proc in processes:
        if proc.poll() is None: proc.terminate();proc.wait(timeout=5)
    if probe: U.DestroyWindow(probe)
    U.SetCursorPos(old_cursor.x,old_cursor.y)
    if old_foreground: U.SetForegroundWindow(old_foreground)
