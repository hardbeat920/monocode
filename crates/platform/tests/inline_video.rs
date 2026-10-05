//! The native video controls and scrolled frame in a hidden AppKit view.
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn main() {
    println!("inline_video: platform window playback check is unavailable on this target");
}

#[cfg(target_os = "windows")]
fn main() {
    use monocode_platform::video::{NativeVideo, VideoFile, VideoRect};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle};
    use std::num::NonZeroIsize;
    use std::time::{Duration, Instant};
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Gdi::GetWindowRgnBox;
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, DispatchMessageW, FindWindowExW, IsWindow, IsWindowVisible,
        MSG, PM_REMOVE, PeekMessageW, TranslateMessage, WINDOW_EX_STYLE, WS_POPUP,
    };
    use windows::core::w;

    struct TestWindow(HWND);
    impl HasWindowHandle for TestWindow {
        fn window_handle(&self) -> Result<WindowHandle<'_>, raw_window_handle::HandleError> {
            let handle = Win32WindowHandle::new(NonZeroIsize::new(self.0.0 as isize).unwrap());
            Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(handle)) })
        }
    }
    impl Drop for TestWindow {
        fn drop(&mut self) {
            unsafe {
                let _ = DestroyWindow(self.0);
            }
        }
    }
    fn pump() {
        let mut message = MSG::default();
        unsafe {
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }

    let window = TestWindow(unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!(""),
            WS_POPUP,
            0,
            0,
            600,
            400,
            None,
            None,
            None,
            None,
        )
        .unwrap()
    });
    let file = VideoFile::new(include_bytes!("fixtures/inbox-video.mp4"), "video/mp4").unwrap();
    let path = file.path().to_path_buf();
    let video = NativeVideo::new(file).unwrap();
    video
        .place(
            &window,
            VideoRect {
                x: 20.,
                y: -30.,
                width: 560.,
                height: 315.,
            },
            VideoRect {
                x: 0.,
                y: 0.,
                width: 600.,
                height: 400.,
            },
        )
        .unwrap();
    let child = unsafe { FindWindowExW(Some(window.0), None, w!("STATIC"), w!("")).unwrap() };
    let scale = unsafe { (GetDpiForWindow(window.0) as f64 / 96.).max(1.) };
    let mut clip = RECT::default();
    assert_ne!(unsafe { GetWindowRgnBox(child, &mut clip).0 }, 0);
    assert_eq!(clip.top, (30. * scale).round() as i32);
    assert_eq!(clip.bottom, (315. * scale).round() as i32);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !video.is_ready() && Instant::now() < deadline {
        assert!(
            video.failure().is_none(),
            "decoder error {:?}",
            video.failure()
        );
        pump();
    }
    assert!(video.is_ready(), "the H.264 fixture must decode");
    assert_eq!(video.natural_size(), Some((64, 36)));
    video.set_volume(0.4);
    assert!((video.volume() - 0.4).abs() < 0.001);
    video.set_muted(true);
    assert!(video.is_muted());
    video.set_muted(false);
    assert!(!video.is_muted());
    assert!(video.duration() > 0.4 && video.duration() < 0.6);
    video.play();
    assert!(video.is_playing());
    video.pause();
    assert!(!video.is_playing());
    video.seek(0.25);
    let deadline = Instant::now() + Duration::from_secs(2);
    while (video.position() - 0.25).abs() > 0.02 && Instant::now() < deadline {
        pump();
    }
    assert!((video.position() - 0.25).abs() <= 0.02);
    let fullscreen = TestWindow(unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!(""),
            WS_POPUP,
            0,
            0,
            800,
            600,
            None,
            None,
            None,
            None,
        )
        .unwrap()
    });
    video.detach();
    video
        .place(
            &fullscreen,
            VideoRect {
                x: 0.,
                y: 0.,
                width: 800.,
                height: 600.,
            },
            VideoRect {
                x: 0.,
                y: 0.,
                width: 800.,
                height: 600.,
            },
        )
        .unwrap();
    assert_eq!(
        unsafe { FindWindowExW(Some(fullscreen.0), None, w!("STATIC"), w!("")).unwrap() },
        child
    );
    assert!((video.position() - 0.25).abs() <= 0.02);
    assert!(!video.is_playing());
    video.detach();
    drop(fullscreen);
    assert!(unsafe { IsWindow(Some(child)).as_bool() });
    video
        .place(
            &window,
            VideoRect {
                x: 0.,
                y: 0.,
                width: 560.,
                height: 315.,
            },
            VideoRect {
                x: 0.,
                y: 0.,
                width: 600.,
                height: 400.,
            },
        )
        .unwrap();
    assert_eq!(
        unsafe { FindWindowExW(Some(window.0), None, w!("STATIC"), w!("")).unwrap() },
        child
    );
    assert!(video.new_frame().is_none());
    video.play();
    video.hide();
    video.suspend_if_idle(Duration::ZERO);
    assert!(!video.is_playing());
    assert!(!unsafe { IsWindowVisible(window.0).as_bool() });
    drop(video);
    assert!(!unsafe { IsWindow(Some(child)).as_bool() });
    assert!(!path.exists());
    println!("test native_video_decode_play_seek_pause_and_cleanup ... ok");
}

#[cfg(target_os = "linux")]
fn main() {
    use monocode_platform::video::{NativeVideo, VideoFile};
    use std::time::{Duration, Instant};
    let file = VideoFile::new(include_bytes!("fixtures/inbox-video.mp4"), "video/mp4").unwrap();
    let path = file.path().to_path_buf();
    let video = NativeVideo::new(file).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let frame = loop {
        if let Some(frame) = video.new_frame() {
            break frame;
        }
        assert!(video.failure().is_none());
        assert!(Instant::now() < deadline, "the H.264 fixture must decode");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!((frame.width, frame.height), (64, 36));
    assert_eq!(video.natural_size(), Some((64, 36)));
    assert_eq!(frame.bgra.len(), 64 * 36 * 4);
    assert!(
        frame
            .bgra
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255)
    );
    assert!(video.is_ready());
    video.set_volume(0.4);
    assert!((video.volume() - 0.4).abs() < 0.001);
    video.set_muted(true);
    assert!(video.is_muted());
    video.set_muted(false);
    assert!(!video.is_muted());
    assert!(video.duration() > 0.4 && video.duration() < 0.6);
    video.play();
    assert!(video.is_playing());
    video.pause();
    assert!(!video.is_playing());
    video.seek(0.25);
    let deadline = Instant::now() + Duration::from_secs(2);
    while (video.position() - 0.25).abs() > 0.02 && Instant::now() < deadline {
        let _ = video.new_frame();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!((video.position() - 0.25).abs() <= 0.02);
    video.play();
    video.suspend_if_idle(Duration::ZERO);
    assert!(!video.is_playing());
    drop(video);
    assert!(!path.exists());
    println!("test native_video_decode_play_seek_pause_and_cleanup ... ok");
}

#[cfg(target_os = "macos")]
fn main() {
    use monocode_platform::video::{NativeVideo, VideoFile, VideoRect};
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSApplication, NSBackingStoreType, NSBitmapImageFileType, NSView, NSWindow,
        NSWindowStyleMask,
    };
    use objc2_foundation::{NSDate, NSDictionary, NSPoint, NSRect, NSRunLoop, NSSize};
    let mtm = MainThreadMarker::new().expect("main thread");
    let app = NSApplication::sharedApplication(mtm);
    let active = app.isActive();
    let parent = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::new(0., 0.), NSSize::new(600., 400.)),
    );
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            parent.bounds(),
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setContentView(Some(&parent));
    let file = VideoFile::new(include_bytes!("fixtures/inbox-video.mp4"), "video/mp4").unwrap();
    let path = file.path().to_path_buf();
    let video = NativeVideo::new(file).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !video.is_ready() && std::time::Instant::now() < deadline {
        NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.02));
    }
    assert!(
        video.is_ready(),
        "fixture must decode, error {:?}",
        video.failure()
    );
    assert_eq!(video.natural_size(), Some((64, 36)));
    video.play();
    NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.04));
    assert!(video.is_playing());
    assert!(video.duration() > 0.4 && video.duration() < 0.6);
    video.pause();
    assert!(!video.is_playing());
    video.seek(0.25);
    let seek_deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while (video.position() - 0.25).abs() > 0.02 && std::time::Instant::now() < seek_deadline {
        NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.02));
    }
    assert!((video.position() - 0.25).abs() <= 0.02);
    assert!(video.new_frame().is_none());
    video.play();
    assert_eq!(video.controls_style(), 1);
    video.place_in_view(
        &parent,
        VideoRect {
            x: 20.,
            y: -30.,
            width: 560.,
            height: 315.,
        },
        VideoRect {
            x: 0.,
            y: 0.,
            width: 600.,
            height: 400.,
        },
    );
    assert_eq!(parent.subviews().len(), 1);
    let crop = parent.subviews().objectAtIndex(0);
    assert!(!crop.isHidden());
    assert_eq!(crop.frame().size.height, 285.);
    crop.layoutSubtreeIfNeeded();
    crop.displayIfNeeded();
    if let Some(bitmap) = crop.bitmapImageRepForCachingDisplayInRect(crop.bounds()) {
        crop.cacheDisplayInRect_toBitmapImageRep(crop.bounds(), &bitmap);
        let png = unsafe {
            bitmap.representationUsingType_properties(
                NSBitmapImageFileType::PNG,
                &NSDictionary::new(),
            )
        }
        .unwrap();
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/video-shots");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("player.png"), png.to_vec()).unwrap();
    }
    video.hide();
    assert!(crop.isHidden());
    video.suspend_if_idle(std::time::Duration::ZERO);
    assert!(
        !video.is_playing(),
        "a hidden cached page must stop video audio"
    );
    drop(video);
    assert_eq!(parent.subviews().len(), 0);
    assert!(!path.exists());
    assert_eq!(app.isActive(), active);
    assert!(!window.isVisible());
    println!("test native_video_decode_play_controls_clip_hide_and_cleanup ... ok");
}
