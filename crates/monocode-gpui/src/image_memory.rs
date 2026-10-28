//! Bounded image memory. GPUI keeps every `img(path)` decode and its atlas
//! sprite for the life of the app; these caches cap both. Least recently drawn
//! images are evicted past a byte budget, giving their GPU sprites back too.
use gpui::{
    App, AppContext, Asset, Context, Entity, EntityId, ImageAssetLoader, ImageCache,
    ImageCacheError, ImageSource, Img, RenderImage, Resource, Task, WeakEntity, Window, img,
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};

/// Images drawn this recently stay, even past the budget: a cached view may
/// still be painting them without asking the cache again.
const KEEP_RECENT: Duration = Duration::from_secs(2);

struct Caches {
    thumbnails: Entity<BoundedImages>,
    previews: Entity<BoundedImages>,
    images: Entity<BoundedImages>,
}
thread_local! { static CACHES: RefCell<Option<Caches>> = const { RefCell::new(None) }; }

pub(crate) fn init(cx: &mut App) {
    let caches = Caches {
        // Project logos and other icon-sized artwork, drawn at 24pt or less.
        thumbnails: BoundedImages::new(128, 8 << 20, cx),
        // Settings previews a few hundred points across.
        previews: BoundedImages::new(1024, 24 << 20, cx),
        // Note and inbox images. Wider than any pane, so layouts keep their size.
        images: BoundedImages::new(2560, 96 << 20, cx),
    };
    CACHES.with(|slot| *slot.borrow_mut() = Some(caches));
}

fn with_cache(source: impl Into<ImageSource>, pick: fn(&Caches) -> &Entity<BoundedImages>) -> Img {
    let image = img(source);
    // Tests that never call `init` draw through GPUI's own cache.
    CACHES.with(|slot| match slot.borrow().as_ref() {
        Some(caches) => image.image_cache(pick(caches)),
        None => image,
    })
}
/// `img` for icon-sized artwork; the decode is shrunk to 128px.
pub(crate) fn thumbnail(source: impl Into<ImageSource>) -> Img {
    with_cache(source, |c| &c.thumbnails)
}
/// `img` for previews a few hundred points across; the decode is shrunk to 1024px.
pub(crate) fn preview(source: impl Into<ImageSource>) -> Img {
    with_cache(source, |c| &c.previews)
}
/// `img` for content images, kept within the shared budget.
pub(crate) fn image(source: impl Into<ImageSource>) -> Img {
    with_cache(source, |c| &c.images)
}
/// The content cache, for images whose bytes come from somewhere GPUI cannot load.
pub(crate) fn images(cx: &mut App) -> Entity<BoundedImages> {
    if let Some(images) = CACHES.with(|slot| slot.borrow().as_ref().map(|c| c.images.clone())) {
        return images;
    }
    init(cx);
    images(cx)
}

type Loaded = Result<Arc<RenderImage>, ImageCacheError>;

enum Entry {
    Loading {
        views: HashSet<EntityId>,
        _task: Task<()>,
    },
    Ready {
        result: Loaded,
        bytes: usize,
        used: Instant,
    },
}

pub(crate) struct BoundedImages {
    max_side: u32,
    budget: usize,
    bytes: usize,
    entries: HashMap<u64, Entry>,
    trimming: bool,
    this: WeakEntity<Self>,
}

impl BoundedImages {
    fn new(max_side: u32, budget: usize, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self {
            max_side,
            budget,
            bytes: 0,
            entries: HashMap::new(),
            trimming: false,
            this: cx.weak_entity(),
        })
    }

    /// The image for `key` if it is ready; otherwise starts `load` once and
    /// notifies `view` when it finishes.
    pub(crate) fn request<F>(
        &mut self,
        key: u64,
        view: EntityId,
        load: impl FnOnce(&mut App) -> F,
        cx: &mut App,
    ) -> Option<Loaded>
    where
        F: Future<Output = Loaded> + Send + 'static,
    {
        match self.entries.get_mut(&key) {
            Some(Entry::Ready { result, used, .. }) => {
                *used = Instant::now();
                return Some(result.clone());
            }
            Some(Entry::Loading { views, .. }) => {
                views.insert(view);
                return None;
            }
            None => {}
        }
        let max_side = self.max_side;
        let load = load(cx);
        let work = cx
            .background_executor()
            .spawn(async move { load.await.map(|image| fit(image, max_side)) });
        let this = self.this.clone();
        let task = cx.spawn(async move |cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| this.finish(key, result, cx));
        });
        self.entries.insert(
            key,
            Entry::Loading {
                views: HashSet::from([view]),
                _task: task,
            },
        );
        None
    }

    fn finish(&mut self, key: u64, result: Loaded, cx: &mut Context<Self>) {
        let Some(Entry::Loading { views, .. }) = self.entries.remove(&key) else {
            return;
        };
        let bytes = result.as_deref().map_or(0, decoded_bytes);
        self.bytes += bytes;
        self.entries.insert(
            key,
            Entry::Ready {
                result,
                bytes,
                used: Instant::now(),
            },
        );
        let app: &mut App = cx;
        for view in views {
            app.notify(view);
        }
        if self.bytes > self.budget && !self.trimming {
            self.trimming = true;
            // Evict after this frame, never while a window is mid-draw.
            let this = self.this.clone();
            cx.defer(move |cx| {
                let _ = this.update(cx, |this, cx| this.trim(cx));
            });
        }
    }

    fn trim(&mut self, cx: &mut Context<Self>) {
        self.trimming = false;
        let now = Instant::now();
        let mut idle: Vec<(Instant, u64)> = self
            .entries
            .iter()
            .filter_map(|(&key, entry)| match entry {
                Entry::Ready {
                    result: Ok(_),
                    used,
                    ..
                } if now.duration_since(*used) >= KEEP_RECENT => Some((*used, key)),
                _ => None,
            })
            .collect();
        idle.sort_unstable();
        let mut evicted = false;
        for (_, key) in idle {
            if self.bytes <= self.budget {
                break;
            }
            if let Some(Entry::Ready {
                result: Ok(image),
                bytes,
                ..
            }) = self.entries.remove(&key)
            {
                self.bytes -= bytes;
                cx.drop_image(image, None);
                evicted = true;
            }
        }
        // Cached views reuse last frame's sprites; redraw them all so none
        // paints an evicted one. An image still on screen loads again.
        if evicted {
            cx.refresh_windows();
        }
    }
}

impl ImageCache for BoundedImages {
    fn load(&mut self, resource: &Resource, window: &mut Window, cx: &mut App) -> Option<Loaded> {
        let source = resource.clone();
        self.request(
            gpui::hash(resource),
            window.current_view(),
            // Boxed: the loader's future borrows `cx` in its type, though not in fact.
            move |cx| -> Pin<Box<dyn Future<Output = Loaded> + Send>> {
                Box::pin(ImageAssetLoader::load(source, cx))
            },
            cx,
        )
    }
}

fn decoded_bytes(image: &RenderImage) -> usize {
    (0..image.frame_count())
        .map(|frame| {
            let size = image.size(frame);
            size.width.0.max(0) as usize * size.height.0.max(0) as usize * 4
        })
        .sum()
}

/// Shrink a still image whose longer side exceeds `max_side`. Animations keep
/// their frames as decoded.
fn fit(image: Arc<RenderImage>, max_side: u32) -> Arc<RenderImage> {
    if image.frame_count() != 1 {
        return image;
    }
    let size = image.size(0);
    let (width, height) = (size.width.0 as u32, size.height.0 as u32);
    let longest = width.max(height);
    if longest <= max_side {
        return image;
    }
    let Some(pixels) = image
        .as_bytes(0)
        .and_then(|bytes| image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(width, height, bytes))
    else {
        return image;
    };
    let scale = max_side as f64 / longest as f64;
    let target = |side: u32| ((side as f64 * scale).round() as u32).max(1);
    // BGRA is resampled channel by channel, so the order is kept.
    let resized = image::imageops::resize(
        &pixels,
        target(width),
        target(height),
        image::imageops::FilterType::Triangle,
    );
    Arc::new(RenderImage::new(vec![image::Frame::new(resized)]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32) -> Arc<RenderImage> {
        Arc::new(RenderImage::new(vec![image::Frame::new(
            image::RgbaImage::from_pixel(width, height, image::Rgba([1, 2, 3, 255])),
        )]))
    }

    #[test]
    fn fit_shrinks_only_oversized_stills_and_keeps_the_aspect_ratio() {
        let small = solid(100, 50);
        assert!(Arc::ptr_eq(&fit(small.clone(), 128), &small));
        let large = fit(solid(1254, 627), 128);
        let size = large.size(0);
        assert_eq!((size.width.0, size.height.0), (128, 64));
        assert_eq!(&large.as_bytes(0).unwrap()[..4], &[1, 2, 3, 255]);
        assert_eq!(decoded_bytes(&large), 128 * 64 * 4);
    }

    #[gpui::test]
    fn eviction_past_the_budget_frees_the_least_recently_drawn(cx: &mut gpui::TestAppContext) {
        let image = 64 * 64 * 4;
        let cache = cx.update(|cx| BoundedImages::new(4096, 2 * image, cx));
        let view = cx.update(|cx| cx.new(|_| ()).entity_id());
        let request = |key: u64, cx: &mut gpui::TestAppContext| {
            cx.update(|cx| {
                cache.update(cx, |cache, cx| {
                    cache.request(key, view, |_| async { Ok(solid(64, 64)) }, cx)
                })
            })
        };
        for key in 0..2 {
            assert!(request(key, cx).is_none());
            cx.run_until_parked();
            assert!(request(key, cx).is_some_and(|r| r.is_ok()));
        }
        // Age both past the redraw guard; key 0 was drawn longest ago.
        cache.update(cx, |cache, _| {
            for (key, entry) in &mut cache.entries {
                if let Entry::Ready { used, .. } = entry {
                    *used -= KEEP_RECENT * (5 - *key as u32);
                }
            }
        });
        assert!(request(2, cx).is_none());
        cx.run_until_parked();
        cache.update(cx, |cache, _| {
            assert_eq!(cache.bytes, 2 * image);
            assert!(!cache.entries.contains_key(&0));
            assert!(cache.entries.contains_key(&1) && cache.entries.contains_key(&2));
        });
    }
}
