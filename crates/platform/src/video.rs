//! Private video files and native inline playback with the platform controls.
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_VIDEO_BYTES: usize = 128 * 1024 * 1024;
static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

/// A provider-authenticated video. Its private file exists only while held.
pub struct VideoFile {
    path: PathBuf,
}
impl VideoFile {
    pub fn new(bytes: &[u8], mime: &str) -> Result<Self, String> {
        if bytes.is_empty() || bytes.len() > MAX_VIDEO_BYTES {
            return Err("The video is empty or exceeds the preview limit.".into());
        }
        let extension = match mime {
            "video/mp4" => "mp4",
            "video/quicktime" => "mov",
            "video/webm" => "webm",
            _ => return Err(format!("Unsupported video type: {mime}")),
        };
        for _ in 0..10 {
            let serial = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "monocode-inbox-{}-{serial}.{extension}",
                std::process::id()
            ));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt as _;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(mut file) => {
                    if let Err(error) = file.write_all(bytes) {
                        let _ = std::fs::remove_file(&path);
                        return Err(error.to_string());
                    }
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.to_string()),
            }
        }
        Err("Could not create a private video preview file.".into())
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
}
impl Drop for VideoFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A tightly packed BGRA image, ready for GPUI's image atlas.
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}
impl VideoRect {
    pub fn intersection(self, other: Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        Self {
            x,
            y,
            width: ((self.x + self.width).min(other.x + other.width) - x).max(0.),
            height: ((self.y + self.height).min(other.y + other.height) - y).max(0.),
        }
    }
}

#[cfg(target_os = "linux")]
pub use crate::video_linux::NativeVideo;
#[cfg(windows)]
pub use crate::video_windows::NativeVideo;
#[cfg(target_os = "macos")]
pub use macos::NativeVideo;
#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub struct NativeVideo;
#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
impl NativeVideo {
    pub fn new(_: VideoFile) -> Result<Self, String> {
        Err("Inline video playback is unavailable on this platform. Open the video link to play it.".into())
    }
    pub fn place<W>(&self, _: &W, _: VideoRect, _: VideoRect) -> Result<(), String> {
        Ok(())
    }
    pub fn hide(&self) {}
    pub fn detach(&self) {}
    pub fn suspend_if_idle(&self, _: std::time::Duration) {}
    pub fn failure(&self) -> Option<String> {
        None
    }
    pub fn is_ready(&self) -> bool {
        false
    }
    pub fn play(&self) {}
    pub fn pause(&self) {}
    pub fn seek(&self, _: f64) {}
    pub fn position(&self) -> f64 {
        0.
    }
    pub fn duration(&self) -> f64 {
        0.
    }
    pub fn natural_size(&self) -> Option<(u32, u32)> {
        None
    }
    pub fn new_frame(&self) -> Option<VideoFrame> {
        None
    }
    pub fn volume(&self) -> f64 {
        1.
    }
    pub fn set_volume(&self, _: f64) {}
    pub fn is_muted(&self) -> bool {
        false
    }
    pub fn set_muted(&self, _: bool) {}
    pub fn is_playing(&self) -> bool {
        false
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::{MainThreadMarker, MainThreadOnly, msg_send};
    use objc2_app_kit::NSView;
    use objc2_core_media::CMTime;
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::{
        cell::Cell,
        time::{Duration, Instant},
    };

    #[link(name = "AVKit", kind = "framework")]
    unsafe extern "C" {}
    #[link(name = "AVFoundation", kind = "framework")]
    unsafe extern "C" {}

    pub struct NativeVideo {
        view: Retained<NSView>,
        crop: Retained<NSView>,
        player: Retained<AnyObject>,
        _file: VideoFile,
        last_visible: Cell<Instant>,
    }
    impl NativeVideo {
        pub fn new(file: VideoFile) -> Result<Self, String> {
            let mtm =
                MainThreadMarker::new().ok_or("Video playback must start on the main thread.")?;
            let player_class =
                AnyClass::get(c"AVPlayer").ok_or("The native video player is unavailable.")?;
            let view_class = AnyClass::get(c"AVPlayerView")
                .ok_or("The native video controls are unavailable.")?;
            let url_class =
                AnyClass::get(c"NSURL").ok_or("The native file URL API is unavailable.")?;
            let path = NSString::from_str(&file.path().to_string_lossy());
            let zero = NSRect::new(NSPoint::new(0., 0.), NSSize::new(0., 0.));
            let crop = NSView::initWithFrame(NSView::alloc(mtm), zero);
            unsafe {
                let url: Retained<AnyObject> = msg_send![url_class, fileURLWithPath: &*path];
                let player: Retained<AnyObject> = msg_send![player_class, playerWithURL: &*url];
                let allocated: *mut NSView = msg_send![view_class, alloc];
                let initialized: *mut NSView = msg_send![allocated, initWithFrame: zero];
                let view = Retained::from_raw(initialized)
                    .ok_or("The native video view could not initialize.")?;
                let _: () = msg_send![&*view, setPlayer: &*player];
                let _: () = msg_send![&*view, setControlsStyle: 1isize];
                let _: () = msg_send![&*view, setShowsSharingServiceButton: false];
                let _: () = msg_send![&*crop, setClipsToBounds: true];
                crop.addSubview(&view);
                crop.setHidden(true);
                Ok(Self {
                    view,
                    crop,
                    player,
                    _file: file,
                    last_visible: Cell::new(Instant::now()),
                })
            }
        }
        pub fn place(
            &self,
            window: &impl HasWindowHandle,
            bounds: VideoRect,
            clip: VideoRect,
        ) -> Result<(), String> {
            MainThreadMarker::new().ok_or("Video layout must run on the main thread.")?;
            let RawWindowHandle::AppKit(raw) =
                window.window_handle().map_err(|e| e.to_string())?.as_raw()
            else {
                return Err("The video window has no AppKit view.".into());
            };
            let parent = unsafe { &*raw.ns_view.as_ptr().cast::<NSView>() };
            self.place_in_view(parent, bounds, clip);
            Ok(())
        }
        pub fn place_in_view(&self, parent: &NSView, bounds: VideoRect, clip: VideoRect) {
            let visible = bounds.intersection(clip);
            if visible.width <= 0. || visible.height <= 0. {
                self.hide();
                return;
            }
            let current = unsafe { self.crop.superview() };
            if current.as_deref().is_none_or(|v| !std::ptr::eq(v, parent)) {
                self.crop.removeFromSuperview();
                parent.addSubview(&self.crop);
            }
            let parent_h = parent.bounds().size.height;
            let y = if parent.isFlipped() {
                visible.y
            } else {
                parent_h - visible.y - visible.height
            };
            self.crop.setFrame(NSRect::new(
                NSPoint::new(visible.x, y),
                NSSize::new(visible.width, visible.height),
            ));
            // The crop view uses AppKit's bottom-left origin. The player keeps
            // its complete frame while the surrounding view clips a scrolled edge.
            self.view.setFrame(NSRect::new(
                NSPoint::new(
                    bounds.x - visible.x,
                    visible.y + visible.height - bounds.y - bounds.height,
                ),
                NSSize::new(bounds.width, bounds.height),
            ));
            self.crop.setHidden(false);
            self.last_visible.set(Instant::now());
        }
        pub fn hide(&self) {
            self.crop.setHidden(true);
        }
        /// Stops audio when a cached media view no longer draws in its window.
        pub fn suspend_if_idle(&self, timeout: Duration) {
            if self.last_visible.get().elapsed() >= timeout {
                self.hide();
                unsafe {
                    let _: () = msg_send![&*self.player, pause];
                }
            }
        }
        pub fn failure(&self) -> Option<String> {
            unsafe {
                let item: Option<Retained<AnyObject>> = msg_send![&*self.player, currentItem];
                let item = item?;
                let status: isize = msg_send![&*item, status];
                if status != 2 {
                    return None;
                }
                let error: Option<Retained<AnyObject>> = msg_send![&*item, error];
                let message: Retained<NSString> = msg_send![&*error?, localizedDescription];
                Some(message.to_string())
            }
        }
        pub fn controls_style(&self) -> isize {
            unsafe { msg_send![&*self.view, controlsStyle] }
        }
        pub fn is_ready(&self) -> bool {
            unsafe {
                let item: Option<Retained<AnyObject>> = msg_send![&*self.player, currentItem];
                item.is_some_and(|item| {
                    let status: isize = msg_send![&*item, status];
                    status == 1
                })
            }
        }
        pub fn play(&self) {
            unsafe {
                let _: () = msg_send![&*self.player, play];
            }
        }
        pub fn pause(&self) {
            unsafe {
                let _: () = msg_send![&*self.player, pause];
            }
        }
        pub fn seek(&self, seconds: f64) {
            if !seconds.is_finite() || seconds < 0. {
                return;
            }
            unsafe {
                let time = CMTime::with_seconds(seconds, 600);
                let _: () = msg_send![&*self.player, seekToTime: time];
            }
        }
        pub fn position(&self) -> f64 {
            unsafe {
                let time: CMTime = msg_send![&*self.player, currentTime];
                let seconds = time.seconds();
                if seconds.is_finite() {
                    seconds.max(0.)
                } else {
                    0.
                }
            }
        }
        pub fn duration(&self) -> f64 {
            unsafe {
                let item: Option<Retained<AnyObject>> = msg_send![&*self.player, currentItem];
                let Some(item) = item else {
                    return 0.;
                };
                let time: CMTime = msg_send![&*item, duration];
                let seconds = time.seconds();
                if seconds.is_finite() {
                    seconds.max(0.)
                } else {
                    0.
                }
            }
        }
        pub fn new_frame(&self) -> Option<VideoFrame> {
            None
        }
        pub fn natural_size(&self) -> Option<(u32, u32)> {
            unsafe {
                let item: Option<Retained<AnyObject>> = msg_send![&*self.player, currentItem];
                let item = item?;
                let size: NSSize = msg_send![&*item, presentationSize];
                if size.width.is_finite()
                    && size.height.is_finite()
                    && size.width >= 1.
                    && size.height >= 1.
                {
                    Some((size.width.round() as u32, size.height.round() as u32))
                } else {
                    None
                }
            }
        }
        pub fn volume(&self) -> f64 {
            unsafe {
                let volume: f32 = msg_send![&*self.player, volume];
                f64::from(volume)
            }
        }
        pub fn set_volume(&self, volume: f64) {
            if volume.is_finite() {
                unsafe {
                    let _: () = msg_send![&*self.player, setVolume: volume.clamp(0., 1.) as f32];
                }
            }
        }
        pub fn is_muted(&self) -> bool {
            unsafe { msg_send![&*self.player, isMuted] }
        }
        pub fn set_muted(&self, muted: bool) {
            unsafe {
                let _: () = msg_send![&*self.player, setMuted: muted];
            }
        }
        pub fn is_playing(&self) -> bool {
            unsafe {
                let rate: f32 = msg_send![&*self.player, rate];
                rate > 0.
            }
        }
    }
    impl Drop for NativeVideo {
        fn drop(&mut self) {
            unsafe {
                let _: () = msg_send![&*self.player, pause];
            }
            self.crop.removeFromSuperview();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clips_a_partially_scrolled_video_without_changing_its_frame() {
        let video = VideoRect {
            x: 10.,
            y: -40.,
            width: 640.,
            height: 360.,
        };
        assert_eq!(
            video.intersection(VideoRect {
                x: 0.,
                y: 0.,
                width: 600.,
                height: 500.
            }),
            VideoRect {
                x: 10.,
                y: 0.,
                width: 590.,
                height: 320.
            }
        );
    }
    #[test]
    fn files_exist_only_while_the_preview_owns_them() {
        let file = VideoFile::new(b"video fixture", "video/mp4").unwrap();
        let path = file.path().to_path_buf();
        assert_eq!(std::fs::read(&path).unwrap(), b"video fixture");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        drop(file);
        assert!(!path.exists());
    }
    #[test]
    fn rejects_unknown_and_empty_videos_before_creating_files() {
        assert!(VideoFile::new(b"", "video/mp4").is_err());
        assert!(VideoFile::new(b"data", "application/octet-stream").is_err());
    }
}
