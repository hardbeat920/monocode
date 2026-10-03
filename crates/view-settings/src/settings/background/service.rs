//! Port of the request half of src/features/settings/model/newThreadBackgroundEffects.ts
//! and the caches of its worker.
//!
//! The web worker becomes GPUI's background executor: decoding and every
//! effect run off the UI thread. `BackgroundEffects` keeps the two caches
//! the TypeScript kept (loaded sources and rendered effects) as shared
//! tasks, so callers asking for the same image wait on one job. A rejected
//! job leaves its cache so the next request retries, and only the three
//! newest source revisions stay cached.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use futures::FutureExt as _;
use futures::future::Shared;
use gpui::{App, AppContext as _, Global, RenderImage, Task};
use image::RgbaImage;
use monocode_core::appearance::NewThreadBackgroundEffect;

use super::effects::{self, HazeVariant, Source};

/// A cached job: every caller awaits the same result.
pub type EffectTask<T> = Shared<Task<Result<T, String>>>;

/// Reads an image file. Tests swap in a fake through
/// [`BackgroundEffects::set_reader`].
pub type SourceReader = Arc<dyn Fn(&str) -> Result<Vec<u8>, String> + Send + Sync>;

/// `MAX_CACHED_REVISIONS`.
const MAX_CACHED_REVISIONS: usize = 3;

/// The chat background caches, a GPUI global.
pub struct BackgroundEffects {
    reader: SourceReader,
    /// `loadedSources`, keyed by source key (`path?v=revision`).
    sources: HashMap<String, EffectTask<Arc<Source>>>,
    /// `effectCache`, keyed by `source:effect:themeKey`.
    effects: HashMap<String, EffectTask<Arc<RenderImage>>>,
}

impl Global for BackgroundEffects {}

impl Default for BackgroundEffects {
    fn default() -> Self {
        Self {
            reader: Arc::new(|path| {
                std::fs::read(path).map_err(|_| "Unable to read the background image.".to_string())
            }),
            sources: HashMap::new(),
            effects: HashMap::new(),
        }
    }
}

/// `revisionOf`: the number after `?v=` or `&v=`, else 0.
pub fn revision_of(key: &str) -> i64 {
    for marker in ["?v=", "&v="] {
        if let Some(at) = key.find(marker) {
            let digits: String = key[at + 3..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if !digits.is_empty() {
                return digits.parse().unwrap_or(0);
            }
        }
    }
    0
}

/// `pruneByRevision`: keep the entries of the newest three revisions.
pub fn prune_by_revision<V>(map: &mut HashMap<String, V>) {
    let revisions: BTreeSet<i64> = map.keys().map(|key| revision_of(key)).collect();
    if revisions.len() <= MAX_CACHED_REVISIONS {
        return;
    }
    let keep: BTreeSet<i64> = revisions
        .into_iter()
        .rev()
        .take(MAX_CACHED_REVISIONS)
        .collect();
    map.retain(|key, _| keep.contains(&revision_of(key)));
}

/// RGBA pixels to the BGRA frame GPUI draws.
pub fn render_image(mut image: RgbaImage) -> Arc<RenderImage> {
    for pixel in image.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Arc::new(RenderImage::new(vec![image::Frame::new(image)]))
}

impl BackgroundEffects {
    fn global_mut(cx: &mut App) -> &mut Self {
        if !cx.has_global::<Self>() {
            cx.set_global(Self::default());
        }
        cx.global_mut::<Self>()
    }

    /// Replaces how image files are read, for tests.
    pub fn set_reader(reader: SourceReader, cx: &mut App) {
        let this = Self::global_mut(cx);
        this.reader = reader;
        this.sources.clear();
        this.effects.clear();
    }

    /// How many sources and rendered effects are cached.
    pub fn cached(cx: &App) -> (usize, usize) {
        cx.try_global::<Self>()
            .map(|this| (this.sources.len(), this.effects.len()))
            .unwrap_or((0, 0))
    }

    /// `ensureSource`: load and decode `path` once per source key.
    pub fn ensure_source(source_key: &str, path: &str, cx: &mut App) -> EffectTask<Arc<Source>> {
        if let Some(loading) = Self::global_mut(cx).sources.get(source_key) {
            return loading.clone();
        }
        let reader = Self::global_mut(cx).reader.clone();
        let path = path.to_string();
        let loading = cx
            .background_spawn(async move {
                let bytes = reader(&path)?;
                effects::load_source(&bytes).map(Arc::new)
            })
            .shared();
        let this = Self::global_mut(cx);
        this.sources.insert(source_key.to_string(), loading.clone());
        prune_by_revision(&mut this.sources);
        forget_on_reject(source_key.to_string(), loading.clone(), cx, |this| {
            &mut this.sources
        });
        loading
    }

    fn cached_or_insert(
        cache_key: String,
        cx: &mut App,
        start: impl FnOnce(&mut App) -> Task<Result<Arc<RenderImage>, String>>,
    ) -> EffectTask<Arc<RenderImage>> {
        if let Some(prepared) = Self::global_mut(cx).effects.get(&cache_key) {
            return prepared.clone();
        }
        let prepared = start(cx).shared();
        let this = Self::global_mut(cx);
        this.effects.insert(cache_key.clone(), prepared.clone());
        prune_by_revision(&mut this.effects);
        forget_on_reject(cache_key, prepared.clone(), cx, |this| &mut this.effects);
        prepared
    }

    /// `prepareNewThreadBackgroundEffect`: the image for `effect`, rendered
    /// on a background thread and cached per source, effect, and scheme.
    pub fn prepare(
        source_key: &str,
        path: &str,
        effect: NewThreadBackgroundEffect,
        light: bool,
        cx: &mut App,
    ) -> EffectTask<Arc<RenderImage>> {
        let theme_key = effects::theme_key(effect, light);
        let cache_key = format!("{source_key}:{}:{theme_key}", effect.as_str());
        let (source_key, path) = (source_key.to_string(), path.to_string());
        Self::cached_or_insert(cache_key, cx, move |cx| {
            let source = Self::ensure_source(&source_key, &path, cx);
            let executor = cx.background_executor().clone();
            cx.spawn(async move |_| {
                let source = source.await?;
                executor
                    .spawn(async move {
                        let pixels = effects::render_pixels(&source, effect, theme_key);
                        Ok(render_image(effects::to_image(&source, pixels)))
                    })
                    .await
            })
        })
    }

    /// Haze for one place it is drawn, tinted with the theme's background
    /// and cropped to the `display` box (CSS px).
    pub fn prepare_haze(
        source_key: &str,
        path: &str,
        variant: HazeVariant,
        background: [u8; 3],
        display: (f64, f64),
        cx: &mut App,
    ) -> EffectTask<Arc<RenderImage>> {
        let [r, g, b] = background;
        let (width, height) = (display.0.round(), display.1.round());
        let cache_key = format!(
            "{source_key}:gradient-blur:{variant:?}:{r:02x}{g:02x}{b:02x}:{width}x{height}"
        );
        let (source_key, path) = (source_key.to_string(), path.to_string());
        Self::cached_or_insert(cache_key, cx, move |cx| {
            let source = Self::ensure_source(&source_key, &path, cx);
            let executor = cx.background_executor().clone();
            cx.spawn(async move |_| {
                let source = source.await?;
                executor
                    .spawn(async move {
                        Ok(render_image(effects::haze_pixels(
                            &source, variant, background, width, height,
                        )))
                    })
                    .await
            })
        })
    }
}

/// `forgetOnReject`: when `task` fails, drop its cache entry unless a newer
/// job replaced it.
fn forget_on_reject<T: Clone + 'static>(
    key: String,
    task: EffectTask<T>,
    cx: &mut App,
    cache: fn(&mut BackgroundEffects) -> &mut HashMap<String, EffectTask<T>>,
) {
    cx.spawn(async move |cx| {
        if task.clone().await.is_ok() {
            return;
        }
        cx.update(|cx| {
            let map = cache(BackgroundEffects::global_mut(cx));
            if map.get(&key).is_some_and(|entry| entry.ptr_eq(&task)) {
                map.remove(&key);
            }
        });
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_revision_from_a_source_key() {
        assert_eq!(revision_of("/a.png?v=101"), 101);
        assert_eq!(revision_of("/a.png?x=1&v=7:dither:false"), 7);
        assert_eq!(revision_of("/a.png"), 0);
    }

    #[test]
    fn keeps_only_the_newest_three_revisions() {
        let mut map: HashMap<String, ()> = HashMap::new();
        for revision in 1..=5 {
            map.insert(format!("/a.png?v={revision}"), ());
            map.insert(format!("/a.png?v={revision}:dither:false"), ());
        }
        prune_by_revision(&mut map);
        let mut kept: Vec<i64> = map.keys().map(|key| revision_of(key)).collect();
        kept.sort();
        kept.dedup();
        assert_eq!(kept, vec![3, 4, 5]);
        assert_eq!(map.len(), 6);
    }

    #[test]
    fn swaps_pixels_to_bgra() {
        let image = RgbaImage::from_pixel(1, 1, image::Rgba([1, 2, 3, 4]));
        let render = render_image(image);
        assert_eq!(render.as_bytes(0), Some(&[3u8, 2, 1, 4][..]));
    }
}
