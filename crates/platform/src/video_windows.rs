//! Media Foundation owns video and audio playback in a clipped child window.

use std::cell::{Cell, RefCell};
use std::mem::ManuallyDrop;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Win32::Foundation::{HWND, RECT, RPC_E_CHANGED_MODE};
use windows::Win32::Graphics::Gdi::{CreateRectRgn, SetWindowRgn};
use windows::Win32::Media::MediaFoundation::{
    CLSID_MFMediaEngineClassFactory, IMFAttributes, IMFMediaEngine, IMFMediaEngineClassFactory,
    IMFMediaEngineEx, IMFMediaEngineNotify, IMFMediaEngineNotify_Impl, MF_MEDIA_ENGINE_CALLBACK,
    MF_MEDIA_ENGINE_EVENT_ERROR, MF_MEDIA_ENGINE_PLAYBACK_HWND, MF_VERSION, MFCreateAttributes,
    MFSTARTUP_FULL, MFShutdown, MFStartup,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, HWND_MESSAGE, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
    SWP_NOZORDER, SetParent, SetWindowPos, ShowWindow, WINDOW_EX_STYLE, WS_CHILD, WS_CLIPSIBLINGS,
};
use windows::core::{BSTR, Interface as _, implement, w};

use super::video::{VideoFile, VideoFrame, VideoRect};

#[implement(IMFMediaEngineNotify)]
struct MediaNotify {
    error: Arc<Mutex<Option<String>>>,
}
impl IMFMediaEngineNotify_Impl for MediaNotify_Impl {
    fn EventNotify(&self, event: u32, param1: usize, param2: u32) -> windows::core::Result<()> {
        if event == MF_MEDIA_ENGINE_EVENT_ERROR.0 as u32
            && let Ok(mut error) = self.error.lock()
        {
            *error = Some(format!(
                "The Windows video decoder failed with media error {param1}, code 0x{param2:08x}."
            ));
        }
        Ok(())
    }
}

struct Player {
    window: HWND,
    parent: HWND,
    engine: ManuallyDrop<IMFMediaEngine>,
    com_initialized: bool,
}
impl Drop for Player {
    fn drop(&mut self) {
        unsafe {
            let _ = self.engine.Shutdown();
            let _ = DestroyWindow(self.window);
            ManuallyDrop::drop(&mut self.engine);
            let _ = MFShutdown();
            if self.com_initialized {
                CoUninitialize();
            }
        }
    }
}

pub struct NativeVideo {
    file: VideoFile,
    player: RefCell<Option<Player>>,
    error: Arc<Mutex<Option<String>>>,
    last_visible: Cell<Instant>,
}

impl Drop for NativeVideo {
    fn drop(&mut self) {
        // Release the decoder's file handle before VideoFile removes the private source.
        self.player.get_mut().take();
    }
}

impl NativeVideo {
    pub fn new(file: VideoFile) -> Result<Self, String> {
        Ok(Self {
            file,
            player: RefCell::new(None),
            error: Arc::default(),
            last_visible: Cell::new(Instant::now()),
        })
    }

    fn create_player(&self, parent: HWND) -> Result<Player, String> {
        unsafe {
            let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            if initialized.is_err() && initialized != RPC_E_CHANGED_MODE {
                return Err(initialized.to_string());
            }
            let com_initialized = initialized.is_ok();
            if let Err(error) = MFStartup(MF_VERSION, MFSTARTUP_FULL) {
                if com_initialized {
                    CoUninitialize();
                }
                return Err(format!("Windows Media Foundation is unavailable: {error}"));
            }
            let mut child = None;
            let result = (|| -> windows::core::Result<Player> {
                let window = CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("STATIC"),
                    w!(""),
                    WS_CHILD | WS_CLIPSIBLINGS,
                    0,
                    0,
                    1,
                    1,
                    Some(parent),
                    None,
                    None,
                    None,
                )?;
                child = Some(window);
                let mut attributes: Option<IMFAttributes> = None;
                MFCreateAttributes(&mut attributes, 2)?;
                let attributes = attributes.expect("MFCreateAttributes returned an object");
                let notify: IMFMediaEngineNotify = MediaNotify {
                    error: self.error.clone(),
                }
                .into();
                attributes.SetUnknown(&MF_MEDIA_ENGINE_CALLBACK, &notify)?;
                attributes.SetUINT64(&MF_MEDIA_ENGINE_PLAYBACK_HWND, window.0 as usize as u64)?;
                let factory: IMFMediaEngineClassFactory =
                    CoCreateInstance(&CLSID_MFMediaEngineClassFactory, None, CLSCTX_INPROC_SERVER)?;
                let uri = url::Url::from_file_path(self.file.path()).map_err(|_| {
                    windows::core::Error::from_hresult(windows::core::HRESULT(
                        0x80070057_u32 as i32,
                    ))
                })?;
                let engine = factory.CreateInstance(0, &attributes)?;
                if let Err(error) = engine
                    .SetAutoPlay(false)
                    .and_then(|_| engine.SetSource(&BSTR::from(uri.as_str())))
                    .and_then(|_| engine.Load())
                {
                    let _ = engine.Shutdown();
                    return Err(error);
                }
                Ok(Player {
                    window,
                    parent,
                    engine: ManuallyDrop::new(engine),
                    com_initialized,
                })
            })();
            match result {
                Ok(player) => Ok(player),
                Err(error) => {
                    if let Some(child) = child {
                        let _ = DestroyWindow(child);
                    }
                    let _ = MFShutdown();
                    if com_initialized {
                        CoUninitialize();
                    }
                    Err(error.to_string())
                }
            }
        }
    }

    pub fn place(
        &self,
        window: &impl HasWindowHandle,
        bounds: VideoRect,
        clip: VideoRect,
    ) -> Result<(), String> {
        let RawWindowHandle::Win32(raw) = window
            .window_handle()
            .map_err(|error| error.to_string())?
            .as_raw()
        else {
            return Err("The video window has no Win32 handle.".into());
        };
        let parent = HWND(raw.hwnd.get() as *mut _);
        let mut player = self.player.borrow_mut();
        if player.is_none() {
            *player = Some(self.create_player(parent)?);
        }
        let player = player.as_mut().expect("the player was initialized");
        if player.parent != parent {
            unsafe { SetParent(player.window, Some(parent)) }.map_err(|error| error.to_string())?;
            player.parent = parent;
        }
        let visible = bounds.intersection(clip);
        if visible.width <= 0. || visible.height <= 0. {
            self.hide_player(player);
            return Ok(());
        }
        self.last_visible.set(Instant::now());
        unsafe {
            let scale = (GetDpiForWindow(parent) as f64 / 96.).max(1.);
            let pixel = |value: f64| (value * scale).round() as i32;
            let width = pixel(bounds.width).max(1);
            let height = pixel(bounds.height).max(1);
            SetWindowPos(
                player.window,
                None,
                pixel(bounds.x),
                pixel(bounds.y),
                width,
                height,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
            .map_err(|error| error.to_string())?;
            let region = CreateRectRgn(
                pixel(visible.x - bounds.x),
                pixel(visible.y - bounds.y),
                pixel(visible.x - bounds.x + visible.width),
                pixel(visible.y - bounds.y + visible.height),
            );
            if SetWindowRgn(player.window, Some(region), true) == 0 {
                let _ = windows::Win32::Graphics::Gdi::DeleteObject(region.into()).ok();
                return Err("The video clipping region could not be applied.".into());
            }
            let mut destination = RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            };
            let (mut source_width, mut source_height) = (0, 0);
            if player
                .engine
                .GetNativeVideoSize(Some(&mut source_width), Some(&mut source_height))
                .is_ok()
                && source_width > 0
                && source_height > 0
            {
                let fit = (f64::from(width) / f64::from(source_width))
                    .min(f64::from(height) / f64::from(source_height));
                let fitted_width = (f64::from(source_width) * fit).round() as i32;
                let fitted_height = (f64::from(source_height) * fit).round() as i32;
                destination.left = (width - fitted_width) / 2;
                destination.top = (height - fitted_height) / 2;
                destination.right = destination.left + fitted_width;
                destination.bottom = destination.top + fitted_height;
            }
            player
                .engine
                .cast::<IMFMediaEngineEx>()
                .and_then(|engine| engine.UpdateVideoStream(None, Some(&destination), None))
                .map_err(|error| error.to_string())?;
            let _ = ShowWindow(player.window, SW_SHOWNOACTIVATE);
        }
        Ok(())
    }

    fn hide_player(&self, player: &Player) {
        unsafe {
            let _ = ShowWindow(player.window, SW_HIDE);
        }
    }
    pub fn hide(&self) {
        if let Some(player) = self.player.borrow().as_ref() {
            self.hide_player(player);
        }
    }
    /// Keep the player alive while its fullscreen parent window closes.
    pub fn detach(&self) {
        let mut player = self.player.borrow_mut();
        let failed = if let Some(player) = player.as_mut() {
            self.hide_player(player);
            match unsafe { SetParent(player.window, Some(HWND_MESSAGE)) } {
                Ok(_) => {
                    player.parent = HWND_MESSAGE;
                    false
                }
                Err(error) => {
                    self.record_error(error);
                    true
                }
            }
        } else {
            false
        };
        if failed {
            *player = None;
        }
    }
    pub fn suspend_if_idle(&self, idle: Duration) {
        if self.last_visible.get().elapsed() > idle {
            self.hide();
            self.pause();
        }
    }
    pub fn failure(&self) -> Option<String> {
        self.error.lock().ok().and_then(|error| error.clone())
    }
    pub fn is_ready(&self) -> bool {
        self.player
            .borrow()
            .as_ref()
            .is_some_and(|player| unsafe { player.engine.GetReadyState() >= 2 })
    }
    pub fn is_playing(&self) -> bool {
        self.player.borrow().as_ref().is_some_and(|player| unsafe {
            !player.engine.IsPaused().as_bool() && !player.engine.IsEnded().as_bool()
        })
    }
    pub fn play(&self) {
        if let Some(player) = self.player.borrow().as_ref() {
            unsafe {
                if player.engine.IsEnded().as_bool() {
                    let _ = player.engine.SetCurrentTime(0.);
                }
                if let Err(error) = player.engine.Play() {
                    self.record_error(error);
                }
            }
        }
    }
    pub fn pause(&self) {
        if let Some(player) = self.player.borrow().as_ref() {
            unsafe {
                if let Err(error) = player.engine.Pause() {
                    self.record_error(error);
                }
            }
        }
    }
    pub fn position(&self) -> f64 {
        self.player
            .borrow()
            .as_ref()
            .map_or(0., |player| unsafe { player.engine.GetCurrentTime() })
    }
    pub fn duration(&self) -> f64 {
        self.player
            .borrow()
            .as_ref()
            .map_or(0., |player| unsafe { player.engine.GetDuration() })
    }
    pub fn natural_size(&self) -> Option<(u32, u32)> {
        let player = self.player.borrow();
        let player = player.as_ref()?;
        let (mut width, mut height) = (0, 0);
        unsafe {
            player
                .engine
                .GetNativeVideoSize(Some(&mut width), Some(&mut height))
        }
        .ok()?;
        (width > 0 && height > 0).then_some((width, height))
    }
    pub fn seek(&self, seconds: f64) {
        if seconds.is_finite()
            && seconds >= 0.
            && let Some(player) = self.player.borrow().as_ref()
        {
            unsafe {
                if let Err(error) = player.engine.SetCurrentTime(seconds) {
                    self.record_error(error);
                }
            }
        }
    }
    pub fn volume(&self) -> f64 {
        self.player
            .borrow()
            .as_ref()
            .map_or(1., |player| unsafe { player.engine.GetVolume() })
    }
    pub fn set_volume(&self, value: f64) {
        if value.is_finite()
            && let Some(player) = self.player.borrow().as_ref()
        {
            unsafe {
                if let Err(error) = player.engine.SetVolume(value.clamp(0., 1.)) {
                    self.record_error(error);
                }
            }
        }
    }
    pub fn is_muted(&self) -> bool {
        self.player
            .borrow()
            .as_ref()
            .is_some_and(|player| unsafe { player.engine.GetMuted().as_bool() })
    }
    pub fn set_muted(&self, value: bool) {
        if let Some(player) = self.player.borrow().as_ref() {
            unsafe {
                if let Err(error) = player.engine.SetMuted(value) {
                    self.record_error(error);
                }
            }
        }
    }
    fn record_error(&self, error: windows::core::Error) {
        if let Ok(mut stored) = self.error.lock() {
            *stored = Some(error.to_string());
        }
    }
    pub fn new_frame(&self) -> Option<VideoFrame> {
        None
    }
}
