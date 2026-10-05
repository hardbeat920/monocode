#!/usr/bin/env python3
"""Capture only a window owned by the app process started by this script."""
import ctypes as c
import ctypes.util
import hashlib
import os
from pathlib import Path
import signal
import struct
import subprocess
import sys
import time
import zlib

root = Path('/home/niost/monocode-gpui-qualification')
artifact = root / 'artifacts' / ('linux-x11-render-checkpoint-' + time.strftime('%Y%m%dT%H%M%SZ', time.gmtime()))
artifact.mkdir(parents=True)
(root / 'linux-x11-render-artifact-path.txt').write_text(str(artifact) + '\n')
env = dict(os.environ)
env.pop('WAYLAND_DISPLAY', None)
env.pop('ZED_HEADLESS', None)
env['DISPLAY'] = ':0'
env['XDG_RUNTIME_DIR'] = '/mnt/wslg/runtime-dir'
for name, directory in [('MONOCODE_DATA_DIR','profile'),('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('XDG_DATA_HOME','data')]:
    location = artifact / directory
    location.mkdir()
    env[name] = str(location)
lib = c.CDLL(ctypes.util.find_library('X11'))
Display = c.c_void_p
Window = c.c_ulong
lib.XOpenDisplay.argtypes = [c.c_char_p]
lib.XOpenDisplay.restype = Display
lib.XDefaultRootWindow.argtypes = [Display]
lib.XDefaultRootWindow.restype = Window
lib.XInternAtom.argtypes = [Display,c.c_char_p,c.c_int]
lib.XInternAtom.restype = Window
lib.XQueryTree.argtypes = [Display,Window,c.POINTER(Window),c.POINTER(Window),c.POINTER(c.POINTER(Window)),c.POINTER(c.c_uint)]
lib.XGetWindowProperty.argtypes = [Display,Window,Window,c.c_long,c.c_long,c.c_int,Window,c.POINTER(Window),c.POINTER(c.c_int),c.POINTER(c.c_ulong),c.POINTER(c.c_ulong),c.POINTER(c.POINTER(c.c_ubyte))]
lib.XFree.argtypes = [c.c_void_p]
lib.XGetGeometry.argtypes = [Display,Window,c.POINTER(Window),c.POINTER(c.c_int),c.POINTER(c.c_int),c.POINTER(c.c_uint),c.POINTER(c.c_uint),c.POINTER(c.c_uint),c.POINTER(c.c_uint)]
lib.XGetImage.argtypes = [Display,Window,c.c_int,c.c_int,c.c_uint,c.c_uint,c.c_ulong,c.c_int]
lib.XGetImage.restype = c.c_void_p
lib.XGetPixel.argtypes = [c.c_void_p,c.c_int,c.c_int]
lib.XGetPixel.restype = c.c_ulong
lib.XDestroyImage.argtypes = [c.c_void_p]
lib.XCloseDisplay.argtypes = [Display]
lib.XSync.argtypes = [Display,c.c_int]
lib.XFreePixmap.argtypes = [Display,Window]
ErrorHandler = c.CFUNCTYPE(c.c_int,Display,c.c_void_p)
errors = []
handler = ErrorHandler(lambda display,event: errors.append('X11 request error') or 0)
lib.XSetErrorHandler.argtypes = [ErrorHandler]
lib.XSetErrorHandler(handler)
class XImagePrefix(c.Structure):
    _fields_ = [('width',c.c_int),('height',c.c_int),('xoffset',c.c_int),('format',c.c_int),('data',c.c_void_p),('byte_order',c.c_int),('bitmap_unit',c.c_int),('bitmap_bit_order',c.c_int),('bitmap_pad',c.c_int),('depth',c.c_int),('bytes_per_line',c.c_int),('bits_per_pixel',c.c_int),('red_mask',c.c_ulong),('green_mask',c.c_ulong),('blue_mask',c.c_ulong)]

def owned_window(display, start, expected_pid):
    atom = lib.XInternAtom(display,b'_NET_WM_PID',0)
    stack = [start]
    while stack:
        window = stack.pop()
        actual_type,fmt,count,after,data = Window(),c.c_int(),c.c_ulong(),c.c_ulong(),c.POINTER(c.c_ubyte)()
        status = lib.XGetWindowProperty(display,window,atom,0,1,0,0,c.byref(actual_type),c.byref(fmt),c.byref(count),c.byref(after),c.byref(data))
        if status == 0 and fmt.value == 32 and count.value == 1:
            pid = c.cast(data,c.POINTER(c.c_ulong))[0]
            lib.XFree(data)
            if pid == expected_pid:
                return window
        elif data:
            lib.XFree(data)
        parent,returned_root,children,n = Window(),Window(),c.POINTER(Window)(),c.c_uint()
        if lib.XQueryTree(display,window,c.byref(returned_root),c.byref(parent),c.byref(children),c.byref(n)):
            stack.extend(children[i] for i in range(n.value))
            if children:
                lib.XFree(children)
    return None

def chunk(name,data):
    return struct.pack('!I',len(data))+name+data+struct.pack('!I',zlib.crc32(name+data)&0xffffffff)

def capture(display,window,path):
    returned_root,x,y,w,h,border,depth = Window(),c.c_int(),c.c_int(),c.c_uint(),c.c_uint(),c.c_uint(),c.c_uint()
    if not lib.XGetGeometry(display,window,c.byref(returned_root),c.byref(x),c.byref(y),c.byref(w),c.byref(h),c.byref(border),c.byref(depth)):
        raise RuntimeError('Owned window geometry is unavailable')
    if not 200 <= w.value <= 8192 or not 200 <= h.value <= 8192:
        raise RuntimeError('Unexpected owned window dimensions')
    drawable = window
    pixmap = None
    composite = c.CDLL(ctypes.util.find_library('Xcomposite'))
    composite.XCompositeNameWindowPixmap.argtypes = [Display,Window]
    composite.XCompositeNameWindowPixmap.restype = Window
    errors.clear()
    candidate = composite.XCompositeNameWindowPixmap(display,window)
    lib.XSync(display,0)
    if not errors and candidate:
        pixmap = candidate
        drawable = pixmap
    image = lib.XGetImage(display,drawable,0,0,w.value,h.value,c.c_ulong(-1),2)
    lib.XSync(display,0)
    if not image:
        raise RuntimeError('The owned native window has no readable rendered image')
    try:
        prefix = c.cast(image,c.POINTER(XImagePrefix)).contents
        masks = [prefix.red_mask,prefix.green_mask,prefix.blue_mask]
        shifts = [(mask&-mask).bit_length()-1 for mask in masks]
        rows = bytearray()
        colors = set()
        for y in range(h.value):
            rows.append(0)
            for x in range(w.value):
                pixel = lib.XGetPixel(image,x,y)
                rgb = bytes(((pixel&mask)>>shift)*255//(mask>>shift) for mask,shift in zip(masks,shifts))
                rows.extend(rgb)
                if x%11 == 0 and y%11 == 0:
                    colors.add(rgb)
        if len(colors) < 10:
            raise RuntimeError('The captured window does not contain a rendered native view')
        png = b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('!2I5B',w.value,h.value,8,2,0,0,0))+chunk(b'IDAT',zlib.compress(rows,6))+chunk(b'IEND',b'')
        path.write_bytes(png)
        print(f'Captured own PID window {window:#x}, {w.value}x{h.value}, {len(colors)} sampled colors, {len(png)} bytes',flush=True)
        print('SHA256 '+hashlib.sha256(png).hexdigest(),flush=True)
    finally:
        lib.XDestroyImage(image)
        if pixmap:
            lib.XFreePixmap(display,pixmap)

executable = Path(sys.argv[1]) if len(sys.argv)>1 else root/'target/debug/monocode-app'
log = (artifact/'app.log').open('wb')
process = subprocess.Popen([str(executable),'--view','widgets','--theme','dark','--size','1280x800','--data-dir',env['MONOCODE_DATA_DIR']],env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
display = lib.XOpenDisplay(b':0')
try:
    if not display:
        raise RuntimeError('The existing X11 display is unavailable')
    deadline = time.monotonic()+35
    window = None
    while time.monotonic()<deadline and process.poll() is None:
        window = owned_window(display,lib.XDefaultRootWindow(display),process.pid)
        if window:
            break
        time.sleep(.1)
    if not window:
        raise RuntimeError('The started app did not create an owned native X11 window')
    time.sleep(3)
    output = artifact/'widgets.png'
    capture(display,window,output)
    Path('/mnt/c/Users/niost/wsl-native-widgets.png').write_bytes(output.read_bytes())
    (artifact/'executable.sha256').write_text(hashlib.sha256(executable.read_bytes()).hexdigest()+'  '+str(executable)+'\n')
    print('Artifact directory '+str(artifact),flush=True)
finally:
    if display:
        lib.XCloseDisplay(display)
    if process.poll() is None:
        os.killpg(process.pid,signal.SIGTERM)
        try:
            process.wait(timeout=4)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid,signal.SIGKILL)
            process.wait()
    log.close()
    print('Owned native app process stopped',flush=True)
