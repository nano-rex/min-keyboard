#!/usr/bin/env python3
"""End-to-end XIM test: a real libX11 client types through min-keyboard-ime.

Usage: DISPLAY=:77 python3 desktop/tests/xim_client.py STYLE OUT_PNG KEYS...
STYLE is 0x404 (over the spot) or 0x408 (root). "@shot" saves the candidate window to OUT_PNG.
Example, with min-keyboard-ime running on the same display:
    python3 desktop/tests/xim_client.py 0x404 /tmp/popup.png "<C-2>" nihao @shot "<space>"
Each KEYS item is a key name sequence: "nihao", "<space>", "<C-space>", "<Return>", ...
Prints what the application received for each item.
"""
import ctypes as C
import sys
import time
import zlib
import struct

x = C.CDLL("libX11.so.6")
libc = C.CDLL("libc.so.6")
libc.setlocale.restype = C.c_char_p
libc.setlocale.argtypes = [C.c_int, C.c_char_p]
print("locale:", libc.setlocale(6, b"C.UTF-8"))
x.XSetLocaleModifiers.restype = C.c_char_p
x.XSetLocaleModifiers.argtypes = [C.c_char_p]
print("modifiers:", x.XSetLocaleModifiers(b"@im=minkeyboard"))

x.XOpenDisplay.restype = C.c_void_p
x.XOpenDisplay.argtypes = [C.c_char_p]
dpy = x.XOpenDisplay(None)
assert dpy, "no display"
x.XDefaultRootWindow.restype = C.c_ulong
x.XDefaultRootWindow.argtypes = [C.c_void_p]
root = x.XDefaultRootWindow(dpy)


class XKeyEvent(C.Structure):
    _fields_ = [("type", C.c_int), ("serial", C.c_ulong), ("send_event", C.c_int), ("display", C.c_void_p),
                ("window", C.c_ulong), ("root", C.c_ulong), ("subwindow", C.c_ulong), ("time", C.c_ulong),
                ("x", C.c_int), ("y", C.c_int), ("x_root", C.c_int), ("y_root", C.c_int),
                ("state", C.c_uint), ("keycode", C.c_uint), ("same_screen", C.c_int)]


class XEvent(C.Union):
    _fields_ = [("type", C.c_int), ("xkey", XKeyEvent), ("pad", C.c_long * 24)]


x.XCreateSimpleWindow.restype = C.c_ulong
x.XCreateSimpleWindow.argtypes = [C.c_void_p, C.c_ulong, C.c_int, C.c_int, C.c_uint, C.c_uint, C.c_uint, C.c_ulong, C.c_ulong]
win = x.XCreateSimpleWindow(dpy, root, 100, 100, 400, 200, 0, 0, 0xffffff)
x.XSelectInput.argtypes = [C.c_void_p, C.c_ulong, C.c_long]
x.XSelectInput(dpy, win, 1 | 2 | (1 << 15) | (1 << 21))
x.XMapWindow.argtypes = [C.c_void_p, C.c_ulong]
x.XMapWindow(dpy, win)
x.XFlush.argtypes = [C.c_void_p]
x.XFlush(dpy)

x.XOpenIM.restype = C.c_void_p
x.XOpenIM.argtypes = [C.c_void_p, C.c_void_p, C.c_char_p, C.c_char_p]
im = x.XOpenIM(dpy, None, None, None)
print("XOpenIM:", bool(im))
assert im, "XOpenIM failed"

style = int(sys.argv[1], 16)
x.XCreateIC.restype = C.c_void_p
x.XVaCreateNestedList.restype = C.c_void_p


class XPoint(C.Structure):
    _fields_ = [("x", C.c_short), ("y", C.c_short)]


spot = XPoint(30, 40)
if style & 0x4:
    x.XLoadQueryFont.restype = C.c_void_p
    missing = C.POINTER(C.c_char_p)()
    count = C.c_int()
    defstr = C.c_char_p()
    x.XCreateFontSet.restype = C.c_void_p
    fontset = x.XCreateFontSet(C.c_void_p(dpy), b"-*-*-*-*-*-*-*-*-*-*-*-*-*-*", C.byref(missing), C.byref(count), C.byref(defstr))
    nested = x.XVaCreateNestedList(0, C.c_char_p(b"spotLocation"), C.byref(spot), C.c_char_p(b"fontSet"), C.c_void_p(fontset), None)
    ic = x.XCreateIC(C.c_void_p(im), C.c_char_p(b"inputStyle"), C.c_long(style), C.c_char_p(b"clientWindow"), C.c_ulong(win),
                     C.c_char_p(b"focusWindow"), C.c_ulong(win), C.c_char_p(b"preeditAttributes"), C.c_void_p(nested), None)
else:
    ic = x.XCreateIC(C.c_void_p(im), C.c_char_p(b"inputStyle"), C.c_long(style), C.c_char_p(b"clientWindow"), C.c_ulong(win),
                     C.c_char_p(b"focusWindow"), C.c_ulong(win), None)
print("XCreateIC:", bool(ic))
assert ic, "XCreateIC failed"
x.XGetICValues.restype = C.c_char_p
mask = C.c_long()
print("filterEvents failed attr:", x.XGetICValues(C.c_void_p(ic), C.c_char_p(b"filterEvents"), C.byref(mask), None), "mask", mask.value)
x.XSetICFocus.argtypes = [C.c_void_p]
x.XSetICFocus(ic)

x.XStringToKeysym.restype = C.c_ulong
x.XStringToKeysym.argtypes = [C.c_char_p]
x.XKeysymToKeycode.restype = C.c_ubyte
x.XKeysymToKeycode.argtypes = [C.c_void_p, C.c_ulong]
x.XFilterEvent.argtypes = [C.POINTER(XEvent), C.c_ulong]
x.XPending.argtypes = [C.c_void_p]
x.XNextEvent.argtypes = [C.c_void_p, C.POINTER(XEvent)]
x.Xutf8LookupString.argtypes = [C.c_void_p, C.POINTER(XKeyEvent), C.c_char_p, C.c_int, C.POINTER(C.c_ulong), C.POINTER(C.c_int)]

received = []


def pump(seconds):
    end = time.time() + seconds
    ev = XEvent()
    while time.time() < end:
        while x.XPending(dpy):
            x.XNextEvent(dpy, C.byref(ev))
            if x.XFilterEvent(C.byref(ev), 0):
                continue
            if ev.type == 2:
                buf = C.create_string_buffer(256)
                sym = C.c_ulong()
                status = C.c_int()
                n = x.Xutf8LookupString(ic, C.byref(ev.xkey), buf, 255, C.byref(sym), C.byref(status))
                if n > 0:
                    received.append(buf.raw[:n].decode())
        time.sleep(0.01)


def press(name, state=0):
    ev = XEvent()
    k = ev.xkey
    k.type = 2
    k.display = dpy
    k.window = win
    k.root = root
    k.time = int(time.time() * 1000) & 0xffffffff
    k.state = state
    sym = x.XStringToKeysym(name.encode())
    k.keycode = x.XKeysymToKeycode(dpy, sym)
    k.same_screen = 1
    # Real events come from the server; the client filters them through the IM.
    if not x.XFilterEvent(C.byref(ev), 0):
        buf = C.create_string_buffer(256)
        s = C.c_ulong()
        status = C.c_int()
        n = x.Xutf8LookupString(ic, C.byref(k), buf, 255, C.byref(s), C.byref(status))
        if n > 0:
            received.append(buf.raw[:n].decode())
    x.XFlush(dpy)
    pump(0.08)


NAMES = {" ": "space", ",": "comma", ".": "period", "'": "apostrophe", "-": "minus", ">": "greater", "\"": "quotedbl"}


def screenshot(path):
    """Saves the min-keyboard window (the mapped override-redirect child of root) as PNG."""
    class Attrs(C.Structure):
        _fields_ = [("x", C.c_int), ("y", C.c_int), ("width", C.c_int), ("height", C.c_int), ("border_width", C.c_int),
                    ("depth", C.c_int), ("visual", C.c_void_p), ("root", C.c_ulong), ("class_", C.c_int),
                    ("bit_gravity", C.c_int), ("win_gravity", C.c_int), ("backing_store", C.c_int),
                    ("backing_planes", C.c_ulong), ("backing_pixel", C.c_ulong), ("save_under", C.c_int),
                    ("colormap", C.c_ulong), ("map_installed", C.c_int), ("map_state", C.c_int),
                    ("all_event_masks", C.c_long), ("your_event_mask", C.c_long), ("do_not_propagate_mask", C.c_long),
                    ("override_redirect", C.c_int), ("screen", C.c_void_p)]
    r = C.c_ulong(); p = C.c_ulong(); kids = C.POINTER(C.c_ulong)(); n = C.c_uint()
    x.XQueryTree(C.c_void_p(dpy), C.c_ulong(root), C.byref(r), C.byref(p), C.byref(kids), C.byref(n))
    for i in range(n.value):
        a = Attrs()
        x.XGetWindowAttributes(C.c_void_p(dpy), C.c_ulong(kids[i]), C.byref(a))
        if a.override_redirect and a.map_state == 2 and a.width > 4:
            print(f"popup at {a.x},{a.y} size {a.width}x{a.height}")
            x.XGetImage.restype = C.c_void_p
            img = x.XGetImage(C.c_void_p(dpy), C.c_ulong(kids[i]), 0, 0, a.width, a.height, C.c_ulong(0xffffffff), 2)
            data_ptr = C.cast(C.c_void_p(img + 16), C.POINTER(C.c_void_p)).contents.value
            bpl = C.cast(C.c_void_p(img + 44), C.POINTER(C.c_int)).contents.value
            raw = C.string_at(data_ptr, bpl * a.height)
            rows = b"".join(b"\0" + b"".join(bytes([raw[y * bpl + 4 * xx + 2], raw[y * bpl + 4 * xx + 1], raw[y * bpl + 4 * xx]]) for xx in range(a.width)) for y in range(a.height))
            def chunk(t, d):
                return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
            png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", a.width, a.height, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")
            open(path, "wb").write(png)
            return True
    print("no popup window mapped")
    return False


pump(0.2)
out = sys.argv[2]
for item in sys.argv[3:]:
    before = len(received)
    if item.startswith("<") and item.endswith(">"):
        name = item[1:-1]
        state = 0
        if name.startswith("C-"):
            state, name = 4, name[2:]
        press(name, state)
    elif item == "@shot":
        screenshot(out)
        continue
    else:
        for ch in item:
            if ch.isupper():
                press(ch, 1)
            else:
                press(NAMES.get(ch, ch))
    pump(0.15)
    print(f"{item!r} -> {''.join(received[before:])!r}")
