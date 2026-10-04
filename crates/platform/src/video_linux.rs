//! GStreamer decodes video and audio. GPUI draws the bounded BGRA frame queue.

use std::cell::{Cell, RefCell};
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use super::video::{VideoFile, VideoFrame, VideoRect};

type Object = *mut c_void;

#[repr(C)]
struct GError {
    domain: u32,
    code: i32,
    message: *mut c_char,
}

// Public GStreamer 1.x C layouts in gstmeta.h, gstvideometa.h, and gstmemory.h.
// Only the prefix of GstVideoMeta is read. The native library owns the full object.
#[repr(C)]
struct VideoMeta {
    meta_flags: c_int,
    meta_info: Object,
    buffer: Object,
    flags: c_int,
    format: c_int,
    id: c_int,
    width: u32,
    height: u32,
    planes: u32,
}

#[repr(C)]
struct MapInfo {
    memory: Object,
    flags: c_int,
    data: *mut u8,
    size: usize,
    maxsize: usize,
    user_data: [Object; 4],
    reserved: [Object; 4],
}

struct Library(Object);
impl Drop for Library {
    fn drop(&mut self) {
        unsafe {
            libc::dlclose(self.0);
        }
    }
}

macro_rules! api {
    ($($name:ident: $signature:ty),* $(,)?) => {
        struct Api { _libraries: Vec<Library>, $($name: $signature),* }
        // Libraries remain loaded until process exit. Functions have the documented C ABI.
        unsafe impl Send for Api {}
        unsafe impl Sync for Api {}
        impl Api {
            fn load() -> Result<Self, String> {
                let mut libraries = Vec::new();
                for name in [c"libgstreamer-1.0.so.0", c"libgstapp-1.0.so.0", c"libgstvideo-1.0.so.0"] {
                    let library = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL) };
                    if library.is_null() {
                        return Err("Install GStreamer with its base, good, and libav plugins to play inline video.".into());
                    }
                    libraries.push(Library(library));
                }
                $(let $name = {
                    let name = CString::new(stringify!($name)).expect("C symbol");
                    let symbol = libraries.iter().find_map(|library| {
                        let symbol = unsafe { libc::dlsym(library.0, name.as_ptr()) };
                        (!symbol.is_null()).then_some(symbol)
                    }).ok_or_else(|| format!("GStreamer is missing {}.", stringify!($name)))?;
                    unsafe { std::mem::transmute::<Object, $signature>(symbol) }
                };)*
                Ok(Self { _libraries: libraries, $($name),* })
            }
        }
    };
}

api! {
    gst_init_check: unsafe extern "C" fn(*mut c_int, *mut *mut *mut c_char, *mut *mut GError) -> c_int,
    gst_element_factory_make: unsafe extern "C" fn(*const c_char, *const c_char) -> Object,
    gst_object_ref_sink: unsafe extern "C" fn(Object) -> Object,
    gst_object_unref: unsafe extern "C" fn(Object),
    g_object_set: unsafe extern "C" fn(Object, *const c_char, ...),
    g_object_get: unsafe extern "C" fn(Object, *const c_char, ...),
    gst_caps_from_string: unsafe extern "C" fn(*const c_char) -> Object,
    gst_caps_unref: unsafe extern "C" fn(Object),
    gst_element_set_state: unsafe extern "C" fn(Object, c_int) -> c_int,
    gst_element_get_bus: unsafe extern "C" fn(Object) -> Object,
    gst_bus_pop_filtered: unsafe extern "C" fn(Object, u32) -> Object,
    gst_message_parse_error: unsafe extern "C" fn(Object, *mut *mut GError, *mut *mut c_char),
    gst_message_unref: unsafe extern "C" fn(Object),
    g_error_free: unsafe extern "C" fn(*mut GError),
    g_free: unsafe extern "C" fn(Object),
    gst_app_sink_try_pull_sample: unsafe extern "C" fn(Object, u64) -> Object,
    gst_app_sink_try_pull_preroll: unsafe extern "C" fn(Object, u64) -> Object,
    gst_app_sink_is_eos: unsafe extern "C" fn(Object) -> c_int,
    gst_sample_get_caps: unsafe extern "C" fn(Object) -> Object,
    gst_sample_get_buffer: unsafe extern "C" fn(Object) -> Object,
    gst_sample_unref: unsafe extern "C" fn(Object),
    gst_caps_get_structure: unsafe extern "C" fn(Object, u32) -> Object,
    gst_structure_get_int: unsafe extern "C" fn(Object, *const c_char, *mut c_int) -> c_int,
    gst_buffer_get_size: unsafe extern "C" fn(Object) -> usize,
    gst_buffer_extract: unsafe extern "C" fn(Object, usize, Object, usize) -> usize,
    gst_buffer_get_video_meta: unsafe extern "C" fn(Object) -> *mut VideoMeta,
    gst_video_meta_map: unsafe extern "C" fn(*mut VideoMeta, u32, *mut MapInfo, *mut Object, *mut c_int, c_int) -> c_int,
    gst_video_meta_unmap: unsafe extern "C" fn(*mut VideoMeta, u32, *mut MapInfo) -> c_int,
    gst_element_query_position: unsafe extern "C" fn(Object, c_int, *mut i64) -> c_int,
    gst_element_query_duration: unsafe extern "C" fn(Object, c_int, *mut i64) -> c_int,
    gst_element_seek_simple: unsafe extern "C" fn(Object, c_int, u32, i64) -> c_int,
    gst_registry_get: unsafe extern "C" fn() -> Object,
    gst_registry_scan_path: unsafe extern "C" fn(Object, *const c_char) -> c_int,
}

fn api() -> Result<&'static Api, String> {
    static API: OnceLock<Result<Api, String>> = OnceLock::new();
    API.get_or_init(|| {
        let api = Api::load()?;
        let mut error = std::ptr::null_mut();
        if unsafe { (api.gst_init_check)(std::ptr::null_mut(), std::ptr::null_mut(), &mut error) }
            == 0
        {
            return Err(unsafe { api.take_error(error) });
        }
        // AppImage carries plugins alongside its copied GStreamer libraries.
        if let Some(app_dir) = std::env::var_os("APPDIR") {
            let plugins = std::path::PathBuf::from(app_dir).join("usr/lib/gstreamer-1.0");
            if let Ok(path) = CString::new(plugins.as_os_str().as_encoded_bytes()) {
                unsafe {
                    (api.gst_registry_scan_path)((api.gst_registry_get)(), path.as_ptr());
                }
            }
        }
        Ok(api)
    })
    .as_ref()
    .map_err(Clone::clone)
}

impl Api {
    unsafe fn take_error(&self, error: *mut GError) -> String {
        if error.is_null() {
            return "The video decoder failed.".into();
        }
        let message = unsafe { CStr::from_ptr((*error).message) }
            .to_string_lossy()
            .into_owned();
        unsafe {
            (self.g_error_free)(error);
        }
        message
    }
}

pub struct NativeVideo {
    api: &'static Api,
    pipeline: Object,
    sink: Object,
    bus: Object,
    _file: VideoFile,
    last_visible: Cell<Instant>,
    ready: Cell<bool>,
    playing: Cell<bool>,
    natural_size: Cell<Option<(u32, u32)>>,
    error: RefCell<Option<String>>,
}

impl NativeVideo {
    pub fn new(file: VideoFile) -> Result<Self, String> {
        let api = api()?;
        let uri = url::Url::from_file_path(file.path()).map_err(|_| "Invalid video path")?;
        let uri = CString::new(uri.as_str()).map_err(|_| "Invalid video URI")?;
        let pipeline =
            unsafe { (api.gst_element_factory_make)(c"playbin".as_ptr(), std::ptr::null()) };
        let sink = unsafe { (api.gst_element_factory_make)(c"appsink".as_ptr(), std::ptr::null()) };
        if pipeline.is_null() || sink.is_null() {
            unsafe {
                if !pipeline.is_null() {
                    (api.gst_object_unref)(pipeline);
                }
                if !sink.is_null() {
                    (api.gst_object_unref)(sink);
                }
            }
            return Err("Install GStreamer's playback and app sink plugins.".into());
        }
        unsafe {
            (api.gst_object_ref_sink)(pipeline);
            (api.gst_object_ref_sink)(sink);
            let caps = (api.gst_caps_from_string)(c"video/x-raw,format=BGRA".as_ptr());
            (api.g_object_set)(
                sink,
                c"caps".as_ptr(),
                caps,
                c"max-buffers".as_ptr(),
                1u32,
                c"drop".as_ptr(),
                1 as c_int,
                c"sync".as_ptr(),
                1 as c_int,
                std::ptr::null::<c_char>(),
            );
            (api.gst_caps_unref)(caps);
            (api.g_object_set)(
                pipeline,
                c"uri".as_ptr(),
                uri.as_ptr(),
                c"video-sink".as_ptr(),
                sink,
                std::ptr::null::<c_char>(),
            );
        }
        let bus = unsafe { (api.gst_element_get_bus)(pipeline) };
        let video = Self {
            api,
            pipeline,
            sink,
            bus,
            _file: file,
            last_visible: Cell::new(Instant::now()),
            ready: Cell::new(false),
            playing: Cell::new(false),
            natural_size: Cell::new(None),
            error: RefCell::new(None),
        };
        // Preroll a preview frame without starting the audio.
        if unsafe { (api.gst_element_set_state)(pipeline, 3) } == 0 {
            return Err(video
                .failure()
                .unwrap_or_else(|| "The video could not load.".into()));
        }
        Ok(video)
    }

    pub fn place<W>(&self, _: &W, _: VideoRect, _: VideoRect) -> Result<(), String> {
        self.last_visible.set(Instant::now());
        Ok(())
    }
    pub fn hide(&self) {}
    pub fn detach(&self) {
        self.hide();
    }
    pub fn suspend_if_idle(&self, idle: Duration) {
        if self.last_visible.get().elapsed() > idle {
            self.pause();
        }
    }
    pub fn failure(&self) -> Option<String> {
        if self.bus.is_null() {
            return self.error.borrow().clone();
        }
        let message = unsafe { (self.api.gst_bus_pop_filtered)(self.bus, 1 << 1) };
        if message.is_null() {
            return self.error.borrow().clone();
        }
        let mut error = std::ptr::null_mut();
        let mut debug = std::ptr::null_mut();
        let error = unsafe {
            (self.api.gst_message_parse_error)(message, &mut error, &mut debug);
            (self.api.gst_message_unref)(message);
            (self.api.g_free)(debug.cast());
            self.api.take_error(error)
        };
        *self.error.borrow_mut() = Some(error.clone());
        Some(error)
    }
    pub fn is_ready(&self) -> bool {
        self.ready.get()
    }
    pub fn is_playing(&self) -> bool {
        self.playing.get() && unsafe { (self.api.gst_app_sink_is_eos)(self.sink) == 0 }
    }
    pub fn play(&self) {
        if unsafe { (self.api.gst_app_sink_is_eos)(self.sink) } != 0 {
            self.seek(0.);
        }
        if unsafe { (self.api.gst_element_set_state)(self.pipeline, 4) } != 0 {
            self.playing.set(true);
        }
    }
    pub fn pause(&self) {
        unsafe {
            (self.api.gst_element_set_state)(self.pipeline, 3);
        }
        self.playing.set(false);
    }
    pub fn position(&self) -> f64 {
        self.query(self.api.gst_element_query_position)
    }
    pub fn duration(&self) -> f64 {
        self.query(self.api.gst_element_query_duration)
    }
    fn query(&self, function: unsafe extern "C" fn(Object, c_int, *mut i64) -> c_int) -> f64 {
        let mut value = 0;
        if unsafe { function(self.pipeline, 3, &mut value) } != 0 {
            value.max(0) as f64 / 1_000_000_000.
        } else {
            0.
        }
    }
    pub fn seek(&self, seconds: f64) {
        if seconds.is_finite() && seconds >= 0. {
            unsafe {
                (self.api.gst_element_seek_simple)(
                    self.pipeline,
                    3,
                    1 | 2,
                    (seconds * 1_000_000_000.) as i64,
                );
            }
        }
    }
    pub fn volume(&self) -> f64 {
        let mut value = 1.;
        unsafe {
            (self.api.g_object_get)(
                self.pipeline,
                c"volume".as_ptr(),
                &mut value as *mut f64,
                std::ptr::null::<c_char>(),
            );
        }
        value
    }
    pub fn set_volume(&self, value: f64) {
        if value.is_finite() {
            unsafe {
                (self.api.g_object_set)(
                    self.pipeline,
                    c"volume".as_ptr(),
                    value.clamp(0., 1.),
                    std::ptr::null::<c_char>(),
                );
            }
        }
    }
    pub fn is_muted(&self) -> bool {
        let mut value: c_int = 0;
        unsafe {
            (self.api.g_object_get)(
                self.pipeline,
                c"mute".as_ptr(),
                &mut value as *mut c_int,
                std::ptr::null::<c_char>(),
            );
        }
        value != 0
    }
    pub fn set_muted(&self, value: bool) {
        unsafe {
            (self.api.g_object_set)(
                self.pipeline,
                c"mute".as_ptr(),
                value as c_int,
                std::ptr::null::<c_char>(),
            );
        }
    }
    pub fn natural_size(&self) -> Option<(u32, u32)> {
        self.natural_size.get()
    }
    /// Poll without blocking the UI. The sink retains at most one pending frame.
    pub fn new_frame(&self) -> Option<VideoFrame> {
        unsafe {
            // Pulling a sample while paused discards the preview's preroll.
            let sample = if self.playing.get() {
                (self.api.gst_app_sink_try_pull_sample)(self.sink, 0)
            } else {
                (self.api.gst_app_sink_try_pull_preroll)(self.sink, 0)
            };
            if sample.is_null() {
                return None;
            }
            let result = self.frame_from_sample(sample);
            (self.api.gst_sample_unref)(sample);
            match result {
                Ok(frame) => {
                    self.ready.set(true);
                    self.natural_size.set(Some((frame.width, frame.height)));
                    Some(frame)
                }
                Err(error) => {
                    *self.error.borrow_mut() = Some(error.into());
                    self.pause();
                    None
                }
            }
        }
    }
    unsafe fn frame_from_sample(&self, sample: Object) -> Result<VideoFrame, &'static str> {
        unsafe {
            let caps = (self.api.gst_sample_get_caps)(sample);
            let buffer = (self.api.gst_sample_get_buffer)(sample);
            if caps.is_null() || buffer.is_null() {
                return Err("The video decoder returned no image buffer.");
            }
            let structure = (self.api.gst_caps_get_structure)(caps, 0);
            let (mut width, mut height) = (0, 0);
            if structure.is_null()
                || (self.api.gst_structure_get_int)(structure, c"width".as_ptr(), &mut width) == 0
                || (self.api.gst_structure_get_int)(structure, c"height".as_ptr(), &mut height) == 0
            {
                return Err("The video decoder returned no image dimensions.");
            }
            let (width, height) = (
                u32::try_from(width).map_err(|_| "Invalid video width.")?,
                u32::try_from(height).map_err(|_| "Invalid video height.")?,
            );
            let size = (width as usize)
                .checked_mul(height as usize)
                .and_then(|size| size.checked_mul(4))
                .ok_or("The video frame is too large.")?;
            if width == 0 || height == 0 || size > 128 * 1024 * 1024 {
                return Err("The video frame is too large or has no pixels.");
            }
            let meta = (self.api.gst_buffer_get_video_meta)(buffer);
            let bgra = if meta.is_null() {
                if (self.api.gst_buffer_get_size)(buffer) != size {
                    return Err("The decoder returned padded pixels without video metadata.");
                }
                let mut pixels = vec![0; size];
                if (self.api.gst_buffer_extract)(buffer, 0, pixels.as_mut_ptr().cast(), size)
                    != size
                {
                    return Err("The video frame could not be copied.");
                }
                pixels
            } else {
                if (*meta).planes != 1 || (*meta).width != width || (*meta).height != height {
                    return Err("The video decoder returned an unexpected pixel layout.");
                }
                let mut map: MapInfo = std::mem::zeroed();
                let mut data = std::ptr::null_mut();
                let mut stride = 0;
                if (self.api.gst_video_meta_map)(meta, 0, &mut map, &mut data, &mut stride, 1) == 0
                {
                    return Err("The video frame could not be mapped.");
                }
                let pixels = if !map.data.is_null() && map.size <= isize::MAX as usize {
                    let bytes = std::slice::from_raw_parts(map.data, map.size);
                    (data as usize)
                        .checked_sub(map.data as usize)
                        .and_then(|offset| {
                            copy_bgra_rows(bytes, offset, stride as isize, width, height)
                        })
                } else {
                    None
                };
                (self.api.gst_video_meta_unmap)(meta, 0, &mut map);
                pixels.ok_or("The video frame has invalid row offsets or stride.")?
            };
            Ok(VideoFrame {
                width,
                height,
                bgra,
            })
        }
    }
}

fn copy_bgra_rows(
    bytes: &[u8],
    offset: usize,
    stride: isize,
    width: u32,
    height: u32,
) -> Option<Vec<u8>> {
    let row_size = (width as usize).checked_mul(4)?;
    let size = row_size.checked_mul(height as usize)?;
    if size == 0 || size > 128 * 1024 * 1024 || stride.unsigned_abs() < row_size {
        return None;
    }
    let mut result = Vec::with_capacity(size);
    for row in 0..height {
        let delta = stride.checked_mul(row as isize)?;
        let start = offset.checked_add_signed(delta)?;
        result.extend_from_slice(bytes.get(start..start.checked_add(row_size)?)?);
    }
    Some(result)
}

impl Drop for NativeVideo {
    fn drop(&mut self) {
        unsafe {
            (self.api.gst_element_set_state)(self.pipeline, 1);
            if !self.bus.is_null() {
                (self.api.gst_object_unref)(self.bus);
            }
            (self.api.gst_object_unref)(self.pipeline);
            (self.api.gst_object_unref)(self.sink);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::copy_bgra_rows;

    #[test]
    fn copies_padded_and_bottom_up_frames_without_padding_pixels() {
        let bytes = [
            0, 0, 0, 0, 1, 2, 3, 4, 99, 99, 99, 99, 5, 6, 7, 8, 99, 99, 99, 99,
        ];
        assert_eq!(
            copy_bgra_rows(&bytes, 4, 8, 1, 2),
            Some(vec![1, 2, 3, 4, 5, 6, 7, 8])
        );
        assert_eq!(
            copy_bgra_rows(&bytes, 12, -8, 1, 2),
            Some(vec![5, 6, 7, 8, 1, 2, 3, 4])
        );
        assert_eq!(copy_bgra_rows(&bytes, 4, 8, 1, 3), None);
        assert_eq!(copy_bgra_rows(&bytes, 4, 2, 1, 2), None);
        assert_eq!(copy_bgra_rows(&bytes, 4, isize::MAX, 1, 3), None);
    }
}
