//! AVPlayerView hosts the platform playback controls in GPUI's clipped pane.
use gpui::{AnyElement, canvas, prelude::*};
use objc2::{
    MainThreadMarker, MainThreadOnly, class, msg_send,
    rc::{Allocated, Retained},
    runtime::AnyObject,
};
use objc2_app_kit::{NSView, NSWindowOrderingMode};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, NSURL};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{cell::RefCell, rc::Rc};

#[link(name = "AVKit", kind = "framework")]
unsafe extern "C" {}
#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {}

#[derive(Clone, Copy)]
#[repr(C)]
struct MediaTime {
    value: i64,
    timescale: i32,
    flags: u32,
    epoch: i64,
}
unsafe impl objc2::Encode for MediaTime {
    const ENCODING: objc2::Encoding = objc2::Encoding::Struct(
        "?",
        &[i64::ENCODING, i32::ENCODING, u32::ENCODING, i64::ENCODING],
    );
}
struct Hosted {
    clip: Retained<NSView>,
    view: Retained<NSView>,
    player: Retained<AnyObject>,
}
#[derive(Clone)]
pub(super) struct Video {
    path: String,
    host: Rc<RefCell<Option<Hosted>>>,
}
impl Video {
    pub(super) fn new(path: String) -> Self {
        Self {
            path,
            host: Rc::new(RefCell::new(None)),
        }
    }
    pub(super) fn pause(&self) {
        if let Some(host) = self.host.borrow().as_ref() {
            unsafe {
                let _: () = msg_send![&host.player, pause];
            }
        }
    }
    pub(super) fn hide(&self) {
        self.pause();
        if let Some(host) = self.host.borrow().as_ref() {
            host.clip.setHidden(true);
        }
    }
    pub(super) fn metadata(&self) -> Option<(u32, u32, f64, bool)> {
        let host = self.host.borrow();
        let host = host.as_ref()?;
        unsafe {
            let item: Option<Retained<AnyObject>> = msg_send![&host.player, currentItem];
            let item = item?;
            let status: isize = msg_send![&item, status];
            if status == 0 {
                return None;
            }
            let size: NSSize = msg_send![&item, presentationSize];
            let time: MediaTime = msg_send![&item, duration];
            Some((
                size.width as u32,
                size.height as u32,
                if time.timescale > 0 {
                    time.value as f64 / time.timescale as f64
                } else {
                    0.
                },
                status == -1,
            ))
        }
    }
    pub(super) fn element(&self) -> AnyElement {
        let host = self.host.clone();
        let path = self.path.clone();
        canvas(|_, _, _| {}, move |bounds, _, window, _| {
            let Ok(handle) = HasWindowHandle::window_handle(window) else { return; };
            let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return; };
            let parent = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
            // AppKit frames are in window points; GPUI's are divided by the interface zoom.
            let zoom = window.content_zoom();
            let clipped = bounds.intersect(&window.content_mask().bounds).map(|v| v * zoom);
            let bounds = bounds.map(|v| v * zoom);
            let mut host = host.borrow_mut();
            if clipped.size.width <= gpui::px(0.) || clipped.size.height <= gpui::px(0.) {
                if let Some(host) = host.as_ref() { host.clip.setHidden(true); }
                return;
            }
            let y = if parent.isFlipped() { f64::from(clipped.top()) } else { parent.bounds().size.height - f64::from(clipped.bottom()) };
            let rect = NSRect::new(NSPoint::new(f64::from(clipped.left()), y), NSSize::new(f64::from(clipped.size.width), f64::from(clipped.size.height)));
            let video_rect = NSRect::new(NSPoint::new(f64::from(bounds.left() - clipped.left()), f64::from(clipped.bottom() - bounds.bottom())), NSSize::new(f64::from(bounds.size.width), f64::from(bounds.size.height)));
            if host.is_none() {
                let Some(mtm) = MainThreadMarker::new() else { return; };
                unsafe {
                    let clip = NSView::initWithFrame(NSView::alloc(mtm), rect);
                    clip.setWantsLayer(true);
                    clip.setClipsToBounds(true);
                    let allocated: Allocated<NSView> = msg_send![class!(AVPlayerView), alloc];
                    let view: Retained<NSView> = msg_send![allocated, initWithFrame: video_rect];
                    let url = NSURL::fileURLWithPath(&NSString::from_str(&path));
                    let player: Retained<AnyObject> = msg_send![class!(AVPlayer), playerWithURL: &*url];
                    let _: () = msg_send![&view, setPlayer: &*player];
                    let _: () = msg_send![&view, setControlsStyle: 1isize];
                    let _: () = msg_send![&view, setShowsFullScreenToggleButton: true];
                    let _: () = msg_send![&view, setVideoGravity: &*NSString::from_str("AVLayerVideoGravityResizeAspect")];
                    clip.addSubview(&view);
                    *host = Some(Hosted { clip, view, player });
                }
            }
            if let Some(host) = host.as_ref() {
                // Reparenting or resetting an unchanged frame on every GPUI
                // paint resets AVPlayerView's hover tracking and hides controls.
                if unsafe { host.clip.superview() }.as_deref().is_none_or(|view| !std::ptr::eq(view, parent)) {
                    parent.addSubview_positioned_relativeTo(&host.clip, NSWindowOrderingMode::Above, None);
                }
                if host.clip.frame() != rect { host.clip.setFrame(rect); }
                if host.view.frame() != video_rect { host.view.setFrame(video_rect); }
                if host.clip.isHidden() { host.clip.setHidden(false); }
            }
        }).size_full().into_any_element()
    }
}
impl Drop for Video {
    fn drop(&mut self) {
        if let Some(host) = self.host.borrow_mut().take() {
            unsafe {
                let _: () = msg_send![&host.player, pause];
            }
            host.clip.removeFromSuperview();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::{Encode, sel};
    #[test]
    fn native_player_metadata_encodings_match_avfoundation() {
        let item = class!(AVPlayerItem);
        for (selector, expected) in [
            (sel!(duration), MediaTime::ENCODING),
            (sel!(presentationSize), NSSize::ENCODING),
            (sel!(status), isize::ENCODING),
        ] {
            let method = item
                .instance_method(selector)
                .expect("AVPlayerItem metadata selector");
            let actual = method.return_type();
            let actual = actual.to_str().unwrap();
            assert!(
                expected.equivalent_to_str(actual),
                "{selector}: expected {expected}, actual {actual}"
            );
        }
    }
}
