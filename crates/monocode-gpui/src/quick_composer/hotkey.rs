//! macOS application hotkeys via HIToolbox; no accessibility permission needed.
use std::ffi::c_void;
#[repr(C)]
struct EventType {
    class: u32,
    kind: u32,
}
#[repr(C)]
struct HotKeyId {
    signature: u32,
    id: u32,
}
type Handler = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32;
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> *mut c_void;
    fn InstallEventHandler(
        target: *mut c_void,
        handler: Handler,
        count: u32,
        types: *const EventType,
        data: *mut c_void,
        reference: *mut *mut c_void,
    ) -> i32;
    fn RemoveEventHandler(reference: *mut c_void) -> i32;
    fn RegisterEventHotKey(
        code: u32,
        modifiers: u32,
        id: HotKeyId,
        target: *mut c_void,
        options: u32,
        reference: *mut *mut c_void,
    ) -> i32;
    fn UnregisterEventHotKey(reference: *mut c_void) -> i32;
}
pub(super) struct Registration {
    hotkey: *mut c_void,
    handler: *mut c_void,
    sender: Box<async_channel::Sender<()>>,
}
unsafe extern "C" fn pressed(_next: *mut c_void, _event: *mut c_void, data: *mut c_void) -> i32 {
    // The box remains alive until its event handler is removed on the UI thread.
    let sender = unsafe { &*data.cast::<async_channel::Sender<()>>() };
    let _ = sender.try_send(());
    0
}
impl Registration {
    pub fn new(shortcut: &str) -> Result<(Self, async_channel::Receiver<()>), String> {
        let (code, mods) = parse(shortcut)?;
        let (tx, rx) = async_channel::bounded(1);
        let mut sender = Box::new(tx);
        let mut handler = std::ptr::null_mut();
        let mut hotkey = std::ptr::null_mut();
        // HIToolbox callbacks and registration are confined to GPUI's main thread.
        unsafe {
            let target = GetApplicationEventTarget();
            let event = EventType {
                class: u32::from_be_bytes(*b"keyb"),
                kind: 5,
            };
            let error = InstallEventHandler(
                target,
                pressed,
                1,
                &event,
                (&mut *sender as *mut async_channel::Sender<()>).cast(),
                &mut handler,
            );
            if error != 0 {
                return Err(format!("Could not register Quick composer ({error})."));
            }
            let error = RegisterEventHotKey(
                code,
                mods,
                HotKeyId {
                    signature: u32::from_be_bytes(*b"Mono"),
                    id: 1,
                },
                target,
                1,
                &mut hotkey,
            );
            if error != 0 {
                RemoveEventHandler(handler);
                return Err(
                    "This shortcut is already in use. Choose another in Keybindings.".into(),
                );
            }
        }
        Ok((
            Self {
                hotkey,
                handler,
                sender,
            },
            rx,
        ))
    }
}
impl Drop for Registration {
    fn drop(&mut self) {
        unsafe {
            UnregisterEventHotKey(self.hotkey);
            RemoveEventHandler(self.handler);
        }
        let _ = &self.sender;
    }
}
fn parse(shortcut: &str) -> Result<(u32, u32), String> {
    let mut modifiers = 0;
    let mut key = None;
    for part in shortcut.split('-') {
        match part {
            "cmd" => modifiers |= 256,
            "shift" => modifiers |= 512,
            "alt" => modifiers |= 2048,
            "ctrl" => modifiers |= 4096,
            v => key = Some(v),
        }
    }
    let key = key.ok_or("Choose a key for Quick composer.")?;
    let code = match key {
        "a" => 0,
        "s" => 1,
        "d" => 2,
        "f" => 3,
        "h" => 4,
        "g" => 5,
        "z" => 6,
        "x" => 7,
        "c" => 8,
        "v" => 9,
        "b" => 11,
        "q" => 12,
        "w" => 13,
        "e" => 14,
        "r" => 15,
        "y" => 16,
        "t" => 17,
        "1" => 18,
        "2" => 19,
        "3" => 20,
        "4" => 21,
        "6" => 22,
        "5" => 23,
        "equal" => 24,
        "9" => 25,
        "7" => 26,
        "minus" => 27,
        "8" => 28,
        "0" => 29,
        "o" => 31,
        "u" => 32,
        "i" => 34,
        "p" => 35,
        "l" => 37,
        "j" => 38,
        "k" => 40,
        "n" => 45,
        "m" => 46,
        "tab" => 48,
        "space" => 49,
        "backspace" => 51,
        "escape" => 53,
        "left" => 123,
        "right" => 124,
        "down" => 125,
        "up" => 126,
        _ => return Err("This Quick composer shortcut is not supported.".into()),
    };
    if modifiers == 0 {
        return Err("Quick composer needs a modifier key.".into());
    }
    Ok((code, modifiers))
}
