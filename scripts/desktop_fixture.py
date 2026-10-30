"""Safety/ownership helpers for explicitly interactive desktop probes only."""
import argparse
import ctypes as C
from ctypes import wintypes as W
import sys

def require_interactive():
    parser = argparse.ArgumentParser(description='Requires a dedicated, unused Windows desktop.')
    parser.add_argument('--interactive-session', action='store_true',
                        help='Confirm this desktop is reserved for interactive tests')
    args = parser.parse_args()
    if not args.interactive_session:
        parser.error('Pass --interactive-session only in a dedicated desktop test session.')
    if sys.platform != 'win32':
        parser.error('A Windows desktop is required.')
    u = C.windll.user32
    u.FindWindowW.argtypes = [W.LPCWSTR, W.LPCWSTR]; u.FindWindowW.restype = W.HWND
    if any(u.FindWindowW(name, None) for name in ['Constellation.Native.Radial', 'Constellation.Config']):
        parser.error('Close the ordinary Constellation resident and Settings first; no user window will be targeted.')

def find_window(class_name, pids):
    """Return a window only when it belongs to a live fixture process."""
    u = C.windll.user32
    callback = C.WINFUNCTYPE(W.BOOL, W.HWND, C.c_ssize_t)
    u.GetWindowThreadProcessId.argtypes = [W.HWND, C.POINTER(W.DWORD)]
    u.GetClassNameW.argtypes = [W.HWND, W.LPWSTR, C.c_int]
    u.EnumWindows.argtypes = [callback, C.c_ssize_t]
    found = []
    @callback
    def visit(hwnd, _):
        pid = W.DWORD(); u.GetWindowThreadProcessId(hwnd, C.byref(pid))
        if pid.value in pids:
            name = C.create_unicode_buffer(256)
            u.GetClassNameW(hwnd, name, len(name))
            if name.value == class_name: found.append(hwnd)
        return True
    u.EnumWindows(visit, 0)
    if len(found) > 1: raise RuntimeError('Ambiguous fixture window ownership')
    return found[0] if found else None

def quit_resident(process):
    hwnd = find_window('Constellation.Native.Radial', {process.pid})
    if hwnd:
        u = C.windll.user32
        u.PostMessageW.argtypes = [W.HWND, W.UINT, C.c_size_t, C.c_ssize_t]
        if not u.PostMessageW(hwnd, 0x8009, 3, 0): raise OSError('Fixture Quit failed')
