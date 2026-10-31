// Copyright 2022 - 2025 Zed Industries, Inc.
// SPDX-License-Identifier: Apache-2.0
// Modified by Monocode for alpha blending, backdrop blur, edge fades, text
// reveal, rounded clipping and project-background compositing. See NOTICE.
use crate::metal_atlas::MetalAtlas;
use anyhow::{Context as _, Result};
use block2::RcBlock;
use core_graphics::geometry::CGSize;
use gpui::{
    AtlasTextureId, Background, Bounds, ContentMask, DevicePixels, Hsla, PaintSurface, Path, Point,
    PrimitiveBatch, Quad, ScaledPixels, Scene, Size, point, size,
};
#[cfg(any(test, feature = "bench-support", feature = "test-support"))]
use image::RgbaImage;
use objc2::{
    msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject},
};

// MPSImageGaussianBlur for frosted popovers.
#[link(name = "MetalPerformanceShaders", kind = "framework")]
unsafe extern "C" {}

use foreign_types::{ForeignType, ForeignTypeRef};
use metal::{
    CAMetalLayer, CommandQueue, MTLGPUFamily, MTLPixelFormat, MTLResourceOptions, NSRange,
    NSUInteger,
};
use objc2_core_foundation::CFRetained;
use objc2_core_video::{
    CVMetalTexture, CVMetalTextureCache, CVMetalTextureGetTexture, CVPixelBufferGetHeight,
    CVPixelBufferGetHeightOfPlane, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth,
    CVPixelBufferGetWidthOfPlane, kCVPixelFormatType_420YpCbCr8BiPlanarFullRange, kCVReturnSuccess,
};
use parking_lot::Mutex;

use std::{
    cell::Cell,
    ffi::c_void,
    mem,
    mem::MaybeUninit,
    ops::Range,
    ptr, slice,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

// Exported to metal
pub(crate) type PointF = gpui::Point<f32>;

#[cfg(not(feature = "runtime_shaders"))]
const SHADERS_METALLIB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/shaders.metallib"));
#[cfg(feature = "runtime_shaders")]
const SHADERS_SOURCE_FILE: &str = include_str!(concat!(env!("OUT_DIR"), "/stitched_shaders.metal"));
// Use 4x MSAA, all devices support it.
// https://developer.apple.com/documentation/metal/mtldevice/1433355-supportstexturesamplecount
const PATH_SAMPLE_COUNT: u32 = 4;
/// Metal requires the offset a buffer is bound at to be 256-byte aligned.
const INSTANCE_BUFFER_ALIGNMENT: usize = 256;
const MAX_INSTANCE_BUFFER_SIZE: usize = 256 * 1024 * 1024;
const DEFAULT_INSTANCE_BUFFER_SIZE: usize = 2 * 1024 * 1024;
/// A frame that grew the instance buffers raises it for good otherwise; this
/// long under a quarter of it halves it again.
const INSTANCE_BUFFER_SHRINK_AFTER: Duration = Duration::from_secs(5);
/// Effect textures (edge fades, haze, frosted popovers) unused this long are
/// dropped at the next frame and made again when the effect returns.
const SCRATCH_IDLE: Duration = Duration::from_secs(2);

static SCRATCH_BYTES: AtomicU64 = AtomicU64::new(0);
static INSTANCE_BUFFER_BYTES: AtomicU64 = AtomicU64::new(DEFAULT_INSTANCE_BUFFER_SIZE as u64);
/// Bumped to make each renderer give back its spare GPU memory on its next
/// frame.
static TRIM_GENERATION: AtomicU64 = AtomicU64::new(0);
static RENDERERS: AtomicU64 = AtomicU64::new(0);
static DRAWABLE_PIXELS: AtomicU64 = AtomicU64::new(0);

/// GPU memory, for diagnostics.
#[derive(Clone, Copy, Debug, Default)]
pub struct GpuMemory {
    /// Everything this process has allocated on the system GPU.
    pub allocated: u64,
    /// The instance buffer size each frame starts from.
    pub instance_buffer: u64,
    /// Effect textures the renderers keep between frames.
    pub scratch: u64,
    /// Live renderers, one per open window.
    pub renderers: u64,
    /// Drawable pixels summed over windows; each window keeps a path texture
    /// and up to three drawables of this size at 4 bytes a pixel.
    pub drawable_pixels: u64,
    pub atlas: crate::metal_atlas::AtlasMemory,
    /// How many times [`trim`] ran.
    pub trims: u64,
}

pub fn gpu_memory() -> GpuMemory {
    GpuMemory {
        allocated: metal::Device::system_default()
            .map_or(0, |device| device.current_allocated_size()),
        instance_buffer: INSTANCE_BUFFER_BYTES.load(Ordering::Relaxed),
        scratch: SCRATCH_BYTES.load(Ordering::Relaxed),
        renderers: RENDERERS.load(Ordering::Relaxed),
        drawable_pixels: DRAWABLE_PIXELS.load(Ordering::Relaxed),
        atlas: crate::metal_atlas::atlas_memory(),
        trims: TRIM_GENERATION.load(Ordering::Relaxed),
    }
}

/// Ask every renderer to drop its instance buffers back to the default size on
/// its next frame. Effect and path textures already go once unused.
pub fn trim() {
    TRIM_GENERATION.fetch_add(1, Ordering::Relaxed);
}

pub type Context = Arc<Mutex<InstanceBufferPool>>;
pub type Renderer = MetalRenderer;

pub unsafe fn new_renderer(
    context: self::Context,
    _native_window: *mut c_void,
    _native_view: *mut c_void,
    _bounds: gpui::Size<f32>,
    transparent: bool,
) -> Renderer {
    MetalRenderer::new(context, transparent)
}

pub struct InstanceBufferPool {
    buffer_size: usize,
    buffers: Vec<metal::Buffer>,
    light_since: Option<Instant>,
}

impl Default for InstanceBufferPool {
    fn default() -> Self {
        Self {
            buffer_size: DEFAULT_INSTANCE_BUFFER_SIZE,
            buffers: Vec::new(),
            light_since: None,
        }
    }
}

pub(crate) struct InstanceBuffer {
    metal_buffer: metal::Buffer,
    size: usize,
}

impl InstanceBufferPool {
    pub(crate) fn reset(&mut self, buffer_size: usize) {
        self.buffer_size = buffer_size;
        self.buffers.clear();
        self.light_since = None;
        INSTANCE_BUFFER_BYTES.store(buffer_size as u64, Ordering::Relaxed);
    }

    /// Halve the buffers after a quiet spell. Buffers still in flight at the
    /// old size are dropped when they come back.
    fn note_frame(&mut self, used: usize) {
        if self.buffer_size <= DEFAULT_INSTANCE_BUFFER_SIZE
            || used.saturating_mul(4) > self.buffer_size
        {
            self.light_since = None;
            return;
        }
        let since = *self.light_since.get_or_insert_with(Instant::now);
        if since.elapsed() >= INSTANCE_BUFFER_SHRINK_AFTER {
            self.reset((self.buffer_size / 2).max(DEFAULT_INSTANCE_BUFFER_SIZE));
        }
    }

    pub(crate) fn acquire(
        &mut self,
        device: &metal::Device,
        unified_memory: bool,
    ) -> InstanceBuffer {
        let buffer = self.buffers.pop().unwrap_or_else(|| {
            let options = if unified_memory {
                MTLResourceOptions::StorageModeShared
                    // Buffers are write only which can benefit from the combined cache
                    // https://developer.apple.com/documentation/metal/mtlresourceoptions/cpucachemodewritecombined
                    | MTLResourceOptions::CPUCacheModeWriteCombined
            } else {
                MTLResourceOptions::StorageModeManaged
            };

            device.new_buffer(self.buffer_size as u64, options)
        });
        InstanceBuffer {
            metal_buffer: buffer,
            size: self.buffer_size,
        }
    }

    pub(crate) fn release(&mut self, buffer: InstanceBuffer) {
        if buffer.size == self.buffer_size {
            self.buffers.push(buffer.metal_buffer)
        }
    }
}

impl Drop for MetalRenderer {
    fn drop(&mut self) {
        RENDERERS.fetch_sub(1, Ordering::Relaxed);
        DRAWABLE_PIXELS.fetch_sub(self.drawable_pixels, Ordering::Relaxed);
    }
}

pub struct MetalRenderer {
    device: metal::Device,
    layer: Option<metal::MetalLayer>,
    is_apple_gpu: bool,
    is_unified_memory: bool,
    presents_with_transaction: bool,
    /// For headless rendering, tracks whether output should be opaque
    opaque: bool,
    command_queue: CommandQueue,
    paths_rasterization_pipeline_state: metal::RenderPipelineState,
    path_sprites_pipeline_state: metal::RenderPipelineState,
    shadows_pipeline_state: metal::RenderPipelineState,
    quads_pipeline_state: metal::RenderPipelineState,
    underlines_pipeline_state: metal::RenderPipelineState,
    monochrome_sprites_pipeline_state: metal::RenderPipelineState,
    polychrome_sprites_pipeline_state: metal::RenderPipelineState,
    surfaces_pipeline_state: metal::RenderPipelineState,
    backdrop_composite_pipeline_state: metal::RenderPipelineState,
    /// `MPSImageGaussianBlur` kernels by sigma, most recently used last.
    backdrop_kernels: Vec<(f32, Retained<AnyObject>)>,
    /// The snapshot and blurred copy of the last backdrop region, reused
    /// while a popover keeps its size.
    backdrop_scratch: Option<(metal::Texture, metal::Texture)>,
    backdrop_used: Option<Instant>,
    edge_fade_pipeline_state: metal::RenderPipelineState,
    text_reveal_pipeline_state: metal::RenderPipelineState,
    edge_fade_scratch: Vec<(metal::Texture, metal::Texture)>,
    edge_fade_used: Vec<Instant>,
    background_pipeline_state: metal::RenderPipelineState,
    background_blurs: Vec<(metal::Texture, metal::Texture)>,
    background_blur_used: Vec<Instant>,
    unit_vertices: metal::Buffer,
    #[allow(clippy::arc_with_non_send_sync)]
    instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>,
    sprite_atlas: Arc<MetalAtlas>,
    core_video_texture_cache: CFRetained<CVMetalTextureCache>,
    path_intermediate_texture: Option<metal::Texture>,
    path_intermediate_msaa_texture: Option<metal::Texture>,
    /// Paths need a window-sized texture; it is made by the first frame that
    /// draws one and dropped once none has for a while.
    paths_used: Option<Instant>,
    trim_generation: u64,
    path_sample_count: u32,
    drawable_pixels: u64,
    /// Offscreen render target reused across `render_scene` calls when
    /// rendering headlessly without reading pixels back.
    #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
    headless_render_target: Option<metal::Texture>,
}

#[repr(C)]
pub struct PathRasterizationVertex {
    pub xy_position: Point<ScaledPixels>,
    pub st_position: Point<f32>,
    pub color: Background,
    pub bounds: Bounds<ScaledPixels>,
}

impl MetalRenderer {
    /// Creates a new MetalRenderer with a CAMetalLayer for window-based rendering.
    pub fn new(instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>, transparent: bool) -> Self {
        let device = Self::create_device();
        let layer = metal::MetalLayer::new();
        Self::configure_layer(&layer, &device, transparent);
        Self::new_internal(device, Some(layer), !transparent, instance_buffer_pool)
    }

    /// Creates a renderer for a CAMetalLayer owned by a platform view, such as
    /// the backing layer UIKit creates for a view whose `layerClass` is
    /// `CAMetalLayer`. The renderer retains the layer.
    pub fn from_layer(
        instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>,
        layer: &objc2_quartz_core::CAMetalLayer,
        transparent: bool,
    ) -> Self {
        let device = Self::create_device();
        // Both types bind the same Objective-C class, so this only changes which
        // Rust wrapper views the live layer. `to_owned` retains it.
        let layer = unsafe {
            metal::MetalLayerRef::from_ptr(ptr::from_ref(layer).cast_mut().cast::<CAMetalLayer>())
        }
        .to_owned();
        Self::configure_layer(&layer, &device, transparent);
        Self::new_internal(device, Some(layer), !transparent, instance_buffer_pool)
    }

    fn configure_layer(layer: &metal::MetalLayerRef, device: &metal::DeviceRef, transparent: bool) {
        layer.set_device(device);
        layer.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
        // Support direct-to-display rendering if the window is not transparent
        // https://developer.apple.com/documentation/metal/managing-your-game-window-for-metal-in-macos
        layer.set_opaque(!transparent);
        // Each drawable is a full window of pixels; a third one costs as much
        // as the path texture and only helps a frame that misses its deadline.
        layer.set_maximum_drawable_count(2);
        // Monocode: backdrop blur copies already drawn pixels out of the
        // drawable, so it must stay readable in every build, not only for
        // visual tests and UI automation screenshots.
        layer.set_framebuffer_only(false);
        // metal-rs doesn't bind these setters, so view the same object through
        // objc2's typed CAMetalLayer binding.
        let objc2_layer: &objc2_quartz_core::CAMetalLayer = unsafe { &*layer.as_ptr().cast() };
        objc2_layer.setAllowsNextDrawableTimeout(false);
        objc2_layer.setNeedsDisplayOnBoundsChange(true);
        // UIKit sizes a view's backing layer itself; only AppKit-hosted
        // layers need to track their superlayer's bounds.
        #[cfg(target_os = "macos")]
        objc2_layer.setAutoresizingMask(
            objc2_quartz_core::CAAutoresizingMask::LayerWidthSizable
                | objc2_quartz_core::CAAutoresizingMask::LayerHeightSizable,
        );
    }

    /// Creates a new headless MetalRenderer for offscreen rendering without a window.
    ///
    /// This renderer can render scenes to images without requiring a CAMetalLayer,
    /// window, or AppKit. Use `render_scene_to_image()` to render scenes.
    #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
    pub fn new_headless(instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>) -> Self {
        let device = Self::create_device();
        Self::new_internal(device, None, true, instance_buffer_pool)
    }

    #[cfg(target_os = "macos")]
    fn create_device() -> metal::Device {
        // Prefer low‐power integrated GPUs on Intel Mac. On Apple
        // Silicon, there is only ever one GPU, so this is equivalent to
        // `metal::Device::system_default()`.
        if let Some(d) = metal::Device::all()
            .into_iter()
            .min_by_key(|d| (d.is_removable(), !d.is_low_power()))
        {
            d
        } else {
            // For some reason `all()` can return an empty list, see https://github.com/zed-industries/zed/issues/37689
            // In that case, we fall back to the system default device.
            log::error!(
                "Unable to enumerate Metal devices; attempting to use system default device"
            );
            Self::system_default_device()
        }
    }

    #[cfg(target_os = "ios")]
    fn create_device() -> metal::Device {
        Self::system_default_device()
    }

    fn system_default_device() -> metal::Device {
        metal::Device::system_default().unwrap_or_else(|| {
            log::error!("unable to access a compatible graphics device");
            std::process::exit(1);
        })
    }

    fn new_internal(
        device: metal::Device,
        layer: Option<metal::MetalLayer>,
        opaque: bool,
        instance_buffer_pool: Arc<Mutex<InstanceBufferPool>>,
    ) -> Self {
        #[cfg(feature = "runtime_shaders")]
        let library = device
            .new_library_with_source(&SHADERS_SOURCE_FILE, &metal::CompileOptions::new())
            .expect("error building metal library");
        #[cfg(not(feature = "runtime_shaders"))]
        let library = device
            .new_library_with_data(SHADERS_METALLIB)
            .expect("error building metal library");

        fn to_float2_bits(point: PointF) -> u64 {
            let mut output = point.y.to_bits() as u64;
            output <<= 32;
            output |= point.x.to_bits() as u64;
            output
        }

        // Shared memory can be used only if CPU and GPU share the same memory space.
        // https://developer.apple.com/documentation/metal/setting-resource-storage-modes
        // iOS does not support managed resources. Its simulator may report a
        // non-unified host GPU even though resources must still use shared storage.
        let is_unified_memory = cfg!(target_os = "ios") || device.has_unified_memory();
        // Apple GPU families support memoryless textures, which can significantly reduce
        // memory usage by keeping render targets in on-chip tile memory instead of
        // allocating backing store in system memory.
        // https://developer.apple.com/documentation/metal/mtlgpufamily
        let is_apple_gpu = device.supports_family(MTLGPUFamily::Apple1);

        let unit_vertices = [
            to_float2_bits(point(0., 0.)),
            to_float2_bits(point(1., 0.)),
            to_float2_bits(point(0., 1.)),
            to_float2_bits(point(0., 1.)),
            to_float2_bits(point(1., 0.)),
            to_float2_bits(point(1., 1.)),
        ];
        let unit_vertices = device.new_buffer_with_data(
            unit_vertices.as_ptr() as *const c_void,
            mem::size_of_val(&unit_vertices) as u64,
            if is_unified_memory {
                MTLResourceOptions::StorageModeShared
                    | MTLResourceOptions::CPUCacheModeWriteCombined
            } else {
                MTLResourceOptions::StorageModeManaged
            },
        );

        let paths_rasterization_pipeline_state = build_path_rasterization_pipeline_state(
            &device,
            &library,
            "paths_rasterization",
            "path_rasterization_vertex",
            "path_rasterization_fragment",
            MTLPixelFormat::BGRA8Unorm,
            PATH_SAMPLE_COUNT,
        );
        let path_sprites_pipeline_state = build_path_sprite_pipeline_state(
            &device,
            &library,
            "path_sprites",
            "path_sprite_vertex",
            "path_sprite_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let shadows_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "shadows",
            "shadow_vertex",
            "shadow_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let quads_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "quads",
            "quad_vertex",
            "quad_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let underlines_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "underlines",
            "underline_vertex",
            "underline_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let monochrome_sprites_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "monochrome_sprites",
            "monochrome_sprite_vertex",
            "monochrome_sprite_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let polychrome_sprites_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "polychrome_sprites",
            "polychrome_sprite_vertex",
            "polychrome_sprite_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let surfaces_pipeline_state = build_pipeline_state(
            &device,
            &library,
            "surfaces",
            "surface_vertex",
            "surface_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );

        let backdrop_composite_pipeline_state = build_backdrop_pipeline_state(
            &device,
            &library,
            "backdrop_composite",
            "backdrop_composite_vertex",
            "backdrop_composite_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let edge_fade_pipeline_state = build_backdrop_pipeline_state(
            &device,
            &library,
            "edge_fade",
            "backdrop_composite_vertex",
            "edge_fade_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let text_reveal_pipeline_state = build_backdrop_pipeline_state(
            &device,
            &library,
            "text_reveal",
            "backdrop_composite_vertex",
            "text_reveal_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );
        let background_pipeline_state = build_backdrop_pipeline_state(
            &device,
            &library,
            "project_background",
            "backdrop_composite_vertex",
            "project_background_fragment",
            MTLPixelFormat::BGRA8Unorm,
        );

        let command_queue = device.new_command_queue();
        let supports_shared_storage = cfg!(target_os = "ios") || is_apple_gpu;
        let sprite_atlas = Arc::new(MetalAtlas::new(device.clone(), supports_shared_storage));
        // metal and objc2-metal expose the same Objective-C device; the borrowed
        // protocol object remains valid while `device` owns it.
        let core_video_texture_cache = unsafe {
            let mut cache = ptr::null_mut();
            let status = CVMetalTextureCache::create(
                None,
                None,
                &*device
                    .as_ptr()
                    .cast::<objc2::runtime::ProtocolObject<dyn objc2_metal::MTLDevice>>(),
                None,
                ptr::NonNull::from(&mut cache),
            );
            assert_eq!(
                status, kCVReturnSuccess,
                "failed to create CoreVideo Metal texture cache"
            );
            CFRetained::from_raw(ptr::NonNull::new(cache).expect("CoreVideo returned a null cache"))
        };

        RENDERERS.fetch_add(1, Ordering::Relaxed);
        Self {
            device,
            layer,
            presents_with_transaction: false,
            is_apple_gpu,
            is_unified_memory,
            opaque,
            command_queue,
            paths_rasterization_pipeline_state,
            path_sprites_pipeline_state,
            shadows_pipeline_state,
            quads_pipeline_state,
            underlines_pipeline_state,
            monochrome_sprites_pipeline_state,
            polychrome_sprites_pipeline_state,
            surfaces_pipeline_state,
            backdrop_composite_pipeline_state,
            backdrop_kernels: Vec::new(),
            backdrop_scratch: None,
            backdrop_used: None,
            edge_fade_pipeline_state,
            text_reveal_pipeline_state,
            edge_fade_scratch: Vec::new(),
            edge_fade_used: Vec::new(),
            background_pipeline_state,
            background_blurs: Vec::new(),
            background_blur_used: Vec::new(),
            unit_vertices,
            instance_buffer_pool,
            sprite_atlas,
            core_video_texture_cache,
            path_intermediate_texture: None,
            path_intermediate_msaa_texture: None,
            paths_used: None,
            trim_generation: 0,
            path_sample_count: PATH_SAMPLE_COUNT,
            drawable_pixels: 0,
            #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
            headless_render_target: None,
        }
    }

    pub fn layer(&self) -> Option<&metal::MetalLayerRef> {
        self.layer.as_ref().map(|l| l.as_ref())
    }

    pub fn layer_ptr(&self) -> *mut CAMetalLayer {
        self.layer
            .as_ref()
            .map(|l| l.as_ptr())
            .unwrap_or(ptr::null_mut())
    }

    pub fn sprite_atlas(&self) -> &Arc<MetalAtlas> {
        &self.sprite_atlas
    }

    pub fn set_presents_with_transaction(&mut self, presents_with_transaction: bool) {
        self.presents_with_transaction = presents_with_transaction;
        if let Some(layer) = &self.layer {
            layer.set_presents_with_transaction(presents_with_transaction);
        }
    }

    pub fn update_drawable_size(&mut self, size: Size<DevicePixels>) {
        if let Some(layer) = &self.layer {
            layer.set_drawable_size(CGSize::new(size.width.0 as f64, size.height.0 as f64));
        }
        let pixels = (size.width.0.max(0) as u64) * (size.height.0.max(0) as u64);
        DRAWABLE_PIXELS.fetch_sub(self.drawable_pixels, Ordering::Relaxed);
        DRAWABLE_PIXELS.fetch_add(pixels, Ordering::Relaxed);
        self.drawable_pixels = pixels;
    }

    /// Make the path textures for a frame of `size` that draws paths.
    fn update_path_intermediate_textures(&mut self, size: Size<DevicePixels>) {
        self.paths_used = Some(Instant::now());
        if self
            .path_intermediate_texture
            .as_ref()
            .is_some_and(|texture| {
                texture.width() == size.width.0 as u64 && texture.height() == size.height.0 as u64
            })
        {
            return;
        }
        // We are uncertain when this happens, but sometimes size can be 0 here. Most likely before
        // the layout pass on window creation. Zero-sized texture creation causes SIGABRT.
        // https://github.com/zed-industries/zed/issues/36229
        if size.width.0 <= 0 || size.height.0 <= 0 {
            self.path_intermediate_texture = None;
            self.path_intermediate_msaa_texture = None;
            return;
        }

        let texture_descriptor = metal::TextureDescriptor::new();
        texture_descriptor.set_width(size.width.0 as u64);
        texture_descriptor.set_height(size.height.0 as u64);
        texture_descriptor.set_pixel_format(metal::MTLPixelFormat::BGRA8Unorm);
        texture_descriptor.set_storage_mode(metal::MTLStorageMode::Private);
        texture_descriptor
            .set_usage(metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead);
        self.path_intermediate_texture = Some(self.device.new_texture(&texture_descriptor));

        if self.path_sample_count > 1 {
            // https://developer.apple.com/documentation/metal/choosing-a-resource-storage-mode-for-apple-gpus
            // Rendering MSAA textures are done in a single pass, so we can use memory-less storage on Apple Silicon
            let storage_mode = if self.is_apple_gpu {
                metal::MTLStorageMode::Memoryless
            } else {
                metal::MTLStorageMode::Private
            };

            let msaa_descriptor = texture_descriptor;
            msaa_descriptor.set_texture_type(metal::MTLTextureType::D2Multisample);
            msaa_descriptor.set_storage_mode(storage_mode);
            msaa_descriptor.set_sample_count(self.path_sample_count as _);
            self.path_intermediate_msaa_texture = Some(self.device.new_texture(&msaa_descriptor));
        } else {
            self.path_intermediate_msaa_texture = None;
        }
    }

    pub fn update_transparency(&mut self, transparent: bool) {
        self.opaque = !transparent;
        if let Some(layer) = &self.layer {
            layer.set_opaque(!transparent);
        }
    }

    pub fn destroy(&self) {
        // nothing to do
    }

    pub fn draw(&mut self, scene: &Scene) {
        let layer = match &self.layer {
            Some(l) => l.clone(),
            None => {
                log::error!(
                    "draw() called on headless renderer - use render_scene_to_image() instead"
                );
                return;
            }
        };
        let viewport_size = layer.drawable_size();
        let viewport_size: Size<DevicePixels> = size(
            (viewport_size.width.ceil() as i32).into(),
            (viewport_size.height.ceil() as i32).into(),
        );
        let drawable = if let Some(drawable) = layer.next_drawable() {
            drawable
        } else {
            log::error!(
                "failed to retrieve next drawable, drawable size: {:?}",
                viewport_size
            );
            return;
        };

        let command_buffer = match self.render_frame(scene, drawable.texture(), viewport_size) {
            Ok(command_buffer) => command_buffer,
            Err(error) => {
                log::error!("failed to render: {error:#}");
                return;
            }
        };

        if self.presents_with_transaction {
            command_buffer.commit();
            command_buffer.wait_until_scheduled();
            drawable.present();
        } else {
            command_buffer.present_drawable(drawable);
            command_buffer.commit();
        }
    }

    fn render_frame(
        &mut self,
        scene: &Scene,
        texture: &metal::TextureRef,
        viewport_size: Size<DevicePixels>,
    ) -> Result<metal::CommandBuffer> {
        let mut writer = InstanceBufferWriter::new(
            &self.device,
            &self.instance_buffer_pool,
            self.is_unified_memory,
        );
        let instance_bindings = write_instances(scene, &mut writer).with_context(|| {
            format!(
                "scene too large: {} paths, {} shadows, {} quads, {} underlines, {} mono, {} poly, {} surfaces",
                scene.paths.len(),
                scene.shadows.len(),
                scene.quads.len(),
                scene.underlines.len(),
                scene.monochrome_sprites.len(),
                scene.polychrome_sprites.len(),
                scene.surfaces.len(),
            )
        })?;
        if !scene.paths.is_empty() {
            self.update_path_intermediate_textures(viewport_size);
        }
        let command_buffer = self.draw_primitives_to_texture(
            scene,
            &instance_bindings,
            &mut writer,
            texture,
            viewport_size,
        )?;

        let used = writer.used();
        let trim_generation = TRIM_GENERATION.load(Ordering::Relaxed);
        if self.trim_generation != trim_generation {
            self.trim_generation = trim_generation;
            self.instance_buffer_pool
                .lock()
                .reset(DEFAULT_INSTANCE_BUFFER_SIZE);
        }
        self.instance_buffer_pool.lock().note_frame(used);
        self.release_idle_scratch();

        let instance_buffer_pool = self.instance_buffer_pool.clone();
        let instance_buffer = Cell::new(Some(writer.finish()));
        let block = RcBlock::new(move |_: ptr::NonNull<AnyObject>| {
            if let Some(instance_buffer) = instance_buffer.take() {
                instance_buffer_pool.lock().release(instance_buffer);
            }
        });
        // SAFETY: Both pointee types are opaque views of the same Objective-C block pointer ABI.
        unsafe {
            command_buffer.add_completed_handler(&*RcBlock::as_ptr(&block).cast());
        }

        Ok(command_buffer)
    }

    /// Renders the scene to a texture and returns the pixel data as an RGBA image.
    /// This does not present the frame to screen - useful for visual testing
    /// where we want to capture what would be rendered without displaying it.
    ///
    /// Note: This requires a layer-backed renderer. For headless rendering,
    /// use `render_scene_to_image()` instead.
    #[cfg(any(test, feature = "test-support"))]
    pub fn render_to_image(&mut self, scene: &Scene) -> Result<RgbaImage> {
        let layer = self
            .layer
            .clone()
            .ok_or_else(|| anyhow::anyhow!("render_to_image requires a layer-backed renderer"))?;
        let viewport_size = layer.drawable_size();
        let viewport_size: Size<DevicePixels> = size(
            (viewport_size.width.ceil() as i32).into(),
            (viewport_size.height.ceil() as i32).into(),
        );
        let drawable = layer
            .next_drawable()
            .ok_or_else(|| anyhow::anyhow!("Failed to get drawable for render_to_image"))?;

        let command_buffer = self.render_frame(scene, drawable.texture(), viewport_size)?;

        // Commit and wait for completion without presenting
        command_buffer.commit();
        command_buffer.wait_until_completed();

        read_texture_to_image(drawable.texture())
    }

    /// Renders a scene to an image without requiring a window or CAMetalLayer.
    ///
    /// This is the primary method for headless rendering. It creates an offscreen
    /// texture, renders the scene to it, and returns the pixel data as an RGBA image.
    #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
    pub fn render_scene_to_image(
        &mut self,
        scene: &Scene,
        size: Size<DevicePixels>,
    ) -> Result<RgbaImage> {
        if size.width.0 <= 0 || size.height.0 <= 0 {
            anyhow::bail!("Invalid size for render_scene_to_image: {:?}", size);
        }

        // Headless callers do not have a Cocoa event-loop pool to release
        // autoreleased command buffers and render-pass descriptors.
        objc2::rc::autoreleasepool(|_| {
            // Update path intermediate textures for this size
            self.update_path_intermediate_textures(size);

            // Create an offscreen texture as render target
            let texture_descriptor = metal::TextureDescriptor::new();
            texture_descriptor.set_width(size.width.0 as u64);
            texture_descriptor.set_height(size.height.0 as u64);
            texture_descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
            texture_descriptor.set_usage(
                metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead,
            );
            // Like the atlas, only Apple GPUs can create shared textures on macOS;
            // Intel Macs cannot, even with unified memory. iOS has no managed storage.
            let uses_shared_storage = cfg!(target_os = "ios") || self.is_apple_gpu;
            texture_descriptor.set_storage_mode(if uses_shared_storage {
                metal::MTLStorageMode::Shared
            } else {
                metal::MTLStorageMode::Managed
            });
            let target_texture = self.device.new_texture(&texture_descriptor);

            let command_buffer = self.render_frame(scene, &target_texture, size)?;

            // Managed textures require an explicit blit synchronize before the CPU
            // can read back the rendered data. Without this, get_bytes returns
            // stale zeros.
            if !uses_shared_storage {
                let blit = command_buffer.new_blit_command_encoder();
                blit.synchronize_resource(&target_texture);
                blit.end_encoding();
            }

            // Commit and wait for completion
            command_buffer.commit();
            command_buffer.wait_until_completed();

            read_texture_to_image(&target_texture)
        })
    }

    /// Renders a scene to a reused offscreen texture without reading pixels
    /// back or blocking on GPU completion.
    ///
    /// This mirrors the CPU cost of presenting a frame to a window (scene
    /// encoding, instance buffer writes, command submission) and is used by
    /// headless benchmark rendering, where the produced pixels are never
    /// inspected.
    #[cfg(any(test, feature = "bench-support", feature = "test-support"))]
    pub fn render_scene(&mut self, scene: &Scene, size: Size<DevicePixels>) -> Result<()> {
        if size.width.0 <= 0 || size.height.0 <= 0 {
            anyhow::bail!("Invalid size for render_scene: {:?}", size);
        }

        objc2::rc::autoreleasepool(|_| {
            self.update_path_intermediate_textures(size);

            let needs_new_target = self.headless_render_target.as_ref().is_none_or(|texture| {
                texture.width() != size.width.0 as u64 || texture.height() != size.height.0 as u64
            });
            if needs_new_target {
                let texture_descriptor = metal::TextureDescriptor::new();
                texture_descriptor.set_width(size.width.0 as u64);
                texture_descriptor.set_height(size.height.0 as u64);
                texture_descriptor.set_pixel_format(MTLPixelFormat::BGRA8Unorm);
                texture_descriptor.set_usage(
                    metal::MTLTextureUsage::RenderTarget | metal::MTLTextureUsage::ShaderRead,
                );
                texture_descriptor.set_storage_mode(metal::MTLStorageMode::Private);
                self.headless_render_target = Some(self.device.new_texture(&texture_descriptor));
            }
            let target_texture = self
                .headless_render_target
                .clone()
                .expect("just ensured the render target exists");

            let command_buffer = self.render_frame(scene, &target_texture, size)?;

            // Commit without waiting, mirroring presentation to a real window where
            // the CPU doesn't block on the GPU.
            command_buffer.commit();
            Ok(())
        })
    }

    /// Drop effect textures no recent frame drew with. Encoded frames keep
    /// their own references until the GPU finishes with them.
    fn release_idle_scratch(&mut self) {
        let now = Instant::now();
        let idle = |used: &Instant| now.duration_since(*used) >= SCRATCH_IDLE;
        // Nested effects use every shallower depth, so only the deepest go.
        while self.edge_fade_used.last().is_some_and(idle) {
            self.edge_fade_used.pop();
            self.edge_fade_scratch.pop();
        }
        self.edge_fade_scratch.truncate(self.edge_fade_used.len());
        while self.background_blur_used.last().is_some_and(idle) {
            self.background_blur_used.pop();
            self.background_blurs.pop();
        }
        self.background_blurs
            .truncate(self.background_blur_used.len());
        if self.backdrop_used.as_ref().is_none_or(idle) {
            self.backdrop_scratch = None;
            self.backdrop_used = None;
        }
        if self.paths_used.as_ref().is_none_or(idle) {
            self.path_intermediate_texture = None;
            self.path_intermediate_msaa_texture = None;
            self.paths_used = None;
        }
        let bytes = self
            .edge_fade_scratch
            .iter()
            .chain(&self.background_blurs)
            .chain(&self.backdrop_scratch)
            .map(|(a, b)| texture_bytes(a) + texture_bytes(b))
            .sum::<u64>()
            + self
                .path_intermediate_texture
                .as_ref()
                .map_or(0, |texture| texture_bytes(texture));
        // Several windows share the total; the last frame's renderer reports.
        SCRATCH_BYTES.store(bytes, Ordering::Relaxed);
    }

    fn draw_primitives_to_texture(
        &mut self,
        scene: &Scene,
        instance_bindings: &InstanceBindings,
        writer: &mut InstanceBufferWriter,
        texture: &metal::TextureRef,
        viewport_size: Size<DevicePixels>,
    ) -> Result<metal::CommandBuffer> {
        let command_queue = self.command_queue.clone();
        let command_buffer = command_queue.new_command_buffer();
        let alpha = if self.opaque { 1. } else { 0. };

        let mut command_encoder = new_command_encoder_for_texture(
            command_buffer,
            texture,
            viewport_size,
            Some(metal::MTLClearColor::new(0., 0., 0., alpha)),
        );

        let mut fade_layers: Vec<(Bounds<ScaledPixels>, Option<Bounds<ScaledPixels>>, usize)> =
            Vec::new();
        for batch in scene.batches() {
            match batch {
                PrimitiveBatch::Shadows(range) => {
                    self.draw_shadows(range, instance_bindings, viewport_size, command_encoder)
                }
                PrimitiveBatch::Quads(range) => {
                    // A marked quad first frosts what is already drawn behind it.
                    let mut start = range.start;
                    for ix in range.clone() {
                        let quad = &scene.quads[ix];
                        let fade = edge_fade_kind(quad);
                        let sigma = backdrop_sigma(quad);
                        if fade.is_none() && sigma.is_none() {
                            continue;
                        }
                        self.draw_quads(
                            start..ix,
                            instance_bindings,
                            viewport_size,
                            command_encoder,
                        );
                        command_encoder.end_encoding();
                        if let Some(begin) = fade {
                            if begin {
                                // GPUI can batch disjoint labels' begin markers
                                // together, followed by their text and then their
                                // end markers. Keep each snapshot paired with its
                                // own bounds instead of assuming LIFO ordering.
                                let mut depth = 0;
                                while fade_layers
                                    .iter()
                                    .any(|(_, region, slot)| region.is_some() && *slot == depth)
                                {
                                    depth += 1;
                                }
                                let region = self.begin_edge_fade(
                                    quad,
                                    depth,
                                    texture,
                                    viewport_size,
                                    command_buffer,
                                );
                                if is_background_marker(quad) {
                                    if let Some(region) = region {
                                        self.composite_background(
                                            quad,
                                            region,
                                            depth,
                                            texture,
                                            viewport_size,
                                            command_buffer,
                                            true,
                                        );
                                    }
                                }
                                fade_layers.push((quad.bounds, region, depth));
                            } else if let Some(ix) = fade_layers
                                .iter()
                                .rposition(|(bounds, _, _)| *bounds == quad.bounds)
                            {
                                let (_, region, depth) = fade_layers.remove(ix);
                                if let Some(region) = region {
                                    if is_background_marker(quad) {
                                        self.composite_background(
                                            quad,
                                            region,
                                            depth,
                                            texture,
                                            viewport_size,
                                            command_buffer,
                                            false,
                                        );
                                    } else {
                                        self.end_edge_fade(
                                            quad,
                                            region,
                                            depth,
                                            texture,
                                            viewport_size,
                                            command_buffer,
                                        );
                                    }
                                }
                            }
                        } else if let Some(sigma) = sigma {
                            self.blur_backdrop(quad, sigma, texture, viewport_size, command_buffer);
                        }
                        command_encoder = new_command_encoder_for_texture(
                            command_buffer,
                            texture,
                            viewport_size,
                            None,
                        );
                        start = if fade.is_some() { ix + 1 } else { ix };
                    }
                    self.draw_quads(
                        start..range.end,
                        instance_bindings,
                        viewport_size,
                        command_encoder,
                    )
                }
                PrimitiveBatch::Paths(range) => {
                    let paths = &scene.paths[range];
                    command_encoder.end_encoding();

                    let did_draw = self.draw_paths_to_intermediate(
                        paths,
                        writer,
                        viewport_size,
                        command_buffer,
                    )?;

                    command_encoder = new_command_encoder_for_texture(
                        command_buffer,
                        texture,
                        viewport_size,
                        None,
                    );

                    if did_draw {
                        if let Err(error) = self.draw_paths_from_intermediate(
                            paths,
                            writer,
                            viewport_size,
                            command_encoder,
                        ) {
                            command_encoder.end_encoding();
                            return Err(error);
                        }
                    }
                }
                PrimitiveBatch::Underlines(range) => {
                    self.draw_underlines(range, instance_bindings, viewport_size, command_encoder)
                }
                PrimitiveBatch::MonochromeSprites { texture_id, range } => self
                    .draw_monochrome_sprites(
                        texture_id,
                        range,
                        instance_bindings,
                        viewport_size,
                        command_encoder,
                    ),
                PrimitiveBatch::PolychromeSprites { texture_id, range } => self
                    .draw_polychrome_sprites(
                        texture_id,
                        range,
                        instance_bindings,
                        viewport_size,
                        command_encoder,
                    ),
                PrimitiveBatch::Surfaces(range) => self.draw_surfaces(
                    &scene.surfaces[range.clone()],
                    range.start,
                    instance_bindings,
                    viewport_size,
                    command_encoder,
                ),
                PrimitiveBatch::SubpixelSprites { .. } => unreachable!(),
            }
        }

        command_encoder.end_encoding();

        Ok(command_buffer.to_owned())
    }

    fn draw_paths_to_intermediate(
        &self,
        paths: &[Path<ScaledPixels>],
        writer: &mut InstanceBufferWriter,
        viewport_size: Size<DevicePixels>,
        command_buffer: &metal::CommandBufferRef,
    ) -> Result<bool> {
        if paths.is_empty() {
            return Ok(false);
        }
        let intermediate_texture = self
            .path_intermediate_texture
            .as_ref()
            .context("missing path intermediate texture")?;

        let mut vertices = Vec::new();
        for path in paths {
            vertices.extend(path.vertices.iter().map(|v| PathRasterizationVertex {
                xy_position: v.xy_position,
                st_position: v.st_position,
                color: path.color,
                bounds: path.bounds.intersect(&path.content_mask.bounds),
            }));
        }
        let vertex_instance_bindings = writer.write(&vertices)?;

        let render_pass_descriptor = metal::RenderPassDescriptor::new();
        let color_attachment = render_pass_descriptor
            .color_attachments()
            .object_at(0)
            .unwrap();
        color_attachment.set_load_action(metal::MTLLoadAction::Clear);
        color_attachment.set_clear_color(metal::MTLClearColor::new(0., 0., 0., 0.));

        if let Some(msaa_texture) = &self.path_intermediate_msaa_texture {
            color_attachment.set_texture(Some(msaa_texture));
            color_attachment.set_resolve_texture(Some(intermediate_texture));
            color_attachment.set_store_action(metal::MTLStoreAction::MultisampleResolve);
        } else {
            color_attachment.set_texture(Some(intermediate_texture));
            color_attachment.set_store_action(metal::MTLStoreAction::Store);
        }

        let command_encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
        command_encoder.set_render_pipeline_state(&self.paths_rasterization_pipeline_state);
        command_encoder.set_vertex_buffer(
            PathRasterizationInputIndex::Vertices as u64,
            Some(&vertex_instance_bindings.buffer),
            vertex_instance_bindings.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            PathRasterizationInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_buffer(
            PathRasterizationInputIndex::Vertices as u64,
            Some(&vertex_instance_bindings.buffer),
            vertex_instance_bindings.offset as u64,
        );
        command_encoder.draw_primitives(
            metal::MTLPrimitiveType::Triangle,
            0,
            vertices.len() as u64,
        );

        command_encoder.end_encoding();
        Ok(true)
    }

    fn draw_shadows(
        &self,
        shadows: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if shadows.is_empty() {
            return;
        }

        command_encoder.set_render_pipeline_state(&self.shadows_pipeline_state);
        command_encoder.set_vertex_buffer(
            ShadowInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            ShadowInputIndex::Shadows as u64,
            Some(&instance_bindings.shadows.buffer),
            instance_bindings.shadows.offset as u64,
        );
        command_encoder.set_fragment_buffer(
            ShadowInputIndex::Shadows as u64,
            Some(&instance_bindings.shadows.buffer),
            instance_bindings.shadows.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            ShadowInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            shadows.len() as u64,
            shadows.start as u64,
        );
    }

    // Capture the background before a masked group. Mixing that snapshot back
    // after drawing the group fades its text, images and fills together without
    // adding an opaque gradient over a translucent native window.
    fn begin_edge_fade(
        &mut self,
        quad: &Quad,
        depth: usize,
        target: &metal::TextureRef,
        viewport: Size<DevicePixels>,
        command: &metal::CommandBufferRef,
    ) -> Option<Bounds<ScaledPixels>> {
        let bounds = quad
            .bounds
            .intersect(&quad.content_mask.bounds)
            .intersect(&Bounds::new(
                point(ScaledPixels(0.), ScaledPixels(0.)),
                size(
                    ScaledPixels(i32::from(viewport.width) as f32),
                    ScaledPixels(i32::from(viewport.height) as f32),
                ),
            ));
        let left = bounds.left().0.ceil();
        let top = bounds.top().0.ceil();
        let width = (bounds.right().0.floor() - left).max(0.) as u64;
        let height = (bounds.bottom().0.floor() - top).max(0.) as u64;
        if width == 0 || height == 0 {
            return None;
        }
        let reusable = self
            .edge_fade_scratch
            .get(depth)
            .is_some_and(|(before, _)| {
                before.width() == width
                    && before.height() == height
                    && before.pixel_format() == target.pixel_format()
            });
        if !reusable {
            let make = || {
                let desc = metal::TextureDescriptor::new();
                desc.set_width(width);
                desc.set_height(height);
                desc.set_pixel_format(target.pixel_format());
                desc.set_storage_mode(metal::MTLStorageMode::Private);
                desc.set_usage(metal::MTLTextureUsage::ShaderRead);
                self.device.new_texture(&desc)
            };
            let pair = (make(), make());
            if depth == self.edge_fade_scratch.len() {
                self.edge_fade_scratch.push(pair);
            } else {
                self.edge_fade_scratch[depth] = pair;
            }
        }
        touch(&mut self.edge_fade_used, depth);
        let region = Bounds::new(
            point(ScaledPixels(left), ScaledPixels(top)),
            size(ScaledPixels(width as f32), ScaledPixels(height as f32)),
        );
        copy_fade_region(command, target, &self.edge_fade_scratch[depth].0, region);
        Some(region)
    }

    fn end_edge_fade(
        &self,
        quad: &Quad,
        region: Bounds<ScaledPixels>,
        depth: usize,
        target: &metal::TextureRef,
        viewport: Size<DevicePixels>,
        command: &metal::CommandBufferRef,
    ) {
        let (before, after) = &self.edge_fade_scratch[depth];
        copy_fade_region(command, target, after, region);
        let text_reveal = quad.border_color.h == TEXT_REVEAL_END;
        let corner_clip = quad.border_color.h == CORNER_CLIP_END;
        let params = BackdropCompositeParams {
            origin: [region.left().0, region.top().0],
            size: [region.size.width.0, region.size.height.0],
            region_origin: [quad.bounds.left().0, quad.bounds.top().0],
            region_size: [quad.bounds.size.width.0, quad.bounds.size.height.0],
            viewport_size: [
                i32::from(viewport.width) as f32,
                i32::from(viewport.height) as f32,
            ],
            corner_radius: if corner_clip {
                quad.bounds.size.width.0.min(quad.bounds.size.height.0)
            } else if text_reveal {
                (quad.border_color.s * 2. - 1.) * EDGE_FADE_MAX
            } else {
                quad.border_color.s * EDGE_FADE_MAX
            },
            // A negative padding selects the horizontal mask. Keep it
            // negative without a right ramp, so only the left edge fades.
            padding: if corner_clip {
                // Below the horizontal ramp range: a circular corner mask,
                // followed by its clockwise corner index from the top left.
                -2. * EDGE_FADE_MAX - (quad.border_color.s * 4.).round()
            } else if text_reveal {
                (quad.border_color.l * 2. - 1.) * EDGE_FADE_MAX
            } else if quad.border_color.h == HORIZONTAL_FADE_END {
                -(quad.border_color.l * EDGE_FADE_MAX).max(1e-3)
            } else {
                quad.border_color.l * EDGE_FADE_MAX
            },
        };
        let encoder = new_command_encoder_for_texture(command, target, viewport, None);
        encoder.set_render_pipeline_state(if text_reveal {
            &self.text_reveal_pipeline_state
        } else {
            &self.edge_fade_pipeline_state
        });
        let ptr = &params as *const BackdropCompositeParams as *const c_void;
        let len = mem::size_of_val(&params) as u64;
        encoder.set_vertex_bytes(0, len, ptr);
        encoder.set_fragment_bytes(0, len, ptr);
        encoder.set_fragment_texture(0, Some(before));
        encoder.set_fragment_texture(1, Some(after));
        encoder.draw_primitives(metal::MTLPrimitiveType::Triangle, 0, 6);
        encoder.end_encoding();
    }

    /// Isolate the artwork from the native backdrop, then apply CSS's masks
    /// and Haze layers in view pixels. The source texture survives live resize.
    #[allow(clippy::too_many_arguments)]
    fn composite_background(
        &mut self,
        quad: &Quad,
        region: Bounds<ScaledPixels>,
        depth: usize,
        target: &metal::TextureRef,
        viewport: Size<DevicePixels>,
        command: &metal::CommandBufferRef,
        clear: bool,
    ) {
        let mode = (quad.border_color.s * 8.).round();
        let scale = quad.border_widths.left.0.max(1.);
        let haze = !clear && mode >= 3.;
        let kernels = haze.then(|| {
            let (soft, strong) = if mode == 5. {
                (2., 5.)
            } else if mode == 4. {
                (5., 12.)
            } else {
                (7., 18.)
            };
            (
                self.gaussian_kernel(soft * scale),
                self.gaussian_kernel(strong * scale),
            )
        });
        let (before, artwork) = &self.edge_fade_scratch[depth];
        if !clear {
            copy_fade_region(command, target, artwork, region);
        }
        if haze {
            let reusable = self.background_blurs.get(depth).is_some_and(|(soft, _)| {
                soft.width() == artwork.width()
                    && soft.height() == artwork.height()
                    && soft.pixel_format() == artwork.pixel_format()
            });
            if !reusable {
                let make = || {
                    let descriptor = metal::TextureDescriptor::new();
                    descriptor.set_width(artwork.width());
                    descriptor.set_height(artwork.height());
                    descriptor.set_pixel_format(artwork.pixel_format());
                    descriptor.set_storage_mode(metal::MTLStorageMode::Private);
                    descriptor.set_usage(
                        metal::MTLTextureUsage::ShaderRead | metal::MTLTextureUsage::ShaderWrite,
                    );
                    self.device.new_texture(&descriptor)
                };
                // A background may appear inside another masked group.
                while self.background_blurs.len() <= depth {
                    self.background_blurs.push((make(), make()));
                }
                self.background_blurs[depth] = (make(), make());
            }
            touch(&mut self.background_blur_used, depth);
            let (soft, strong) = &self.background_blurs[depth];
            let (soft_kernel, strong_kernel) = kernels.as_ref().unwrap();
            for (kernel, destination) in [(soft_kernel, soft), (strong_kernel, strong)] {
                // These are live Metal textures and MPSUnaryImageKernel objects.
                unsafe {
                    let _: () = msg_send![
                        &**kernel,
                        encodeToCommandBuffer: command.as_ptr().cast::<AnyObject>(),
                        sourceTexture: artwork.as_ptr().cast::<AnyObject>(),
                        destinationTexture: destination.as_ptr().cast::<AnyObject>()
                    ];
                }
            }
        }
        let radii = quad.corner_radii;
        let params = BackgroundCompositeParams {
            origin: [region.left().0, region.top().0],
            size: [region.size.width.0, region.size.height.0],
            region_origin: [quad.bounds.left().0, quad.bounds.top().0],
            region_size: [quad.bounds.size.width.0, quad.bounds.size.height.0],
            viewport_size: [
                i32::from(viewport.width) as f32,
                i32::from(viewport.height) as f32,
            ],
            radii: [
                radii.top_left.0,
                radii.top_right.0,
                radii.bottom_right.0,
                radii.bottom_left.0,
            ],
            mode: if clear { 0. } else { mode },
            opacity: quad.border_color.l,
            scale,
            padding: 0.,
        };
        let encoder = new_command_encoder_for_texture(command, target, viewport, None);
        encoder.set_render_pipeline_state(&self.background_pipeline_state);
        let ptr = &params as *const BackgroundCompositeParams as *const c_void;
        let len = mem::size_of_val(&params) as u64;
        encoder.set_vertex_bytes(0, len, ptr);
        encoder.set_fragment_bytes(0, len, ptr);
        encoder.set_fragment_texture(0, Some(before));
        encoder.set_fragment_texture(1, Some(artwork));
        let (soft, strong) = if haze {
            let (soft, strong) = &self.background_blurs[depth];
            (soft, strong)
        } else {
            (before, artwork)
        };
        encoder.set_fragment_texture(2, Some(soft));
        encoder.set_fragment_texture(3, Some(strong));
        encoder.draw_primitives(metal::MTLPrimitiveType::Triangle, 0, 6);
        encoder.end_encoding();
    }

    /// Replaces the pixels inside `quad`'s rounded bounds with a Gaussian blur
    /// (`sigma` device pixels) of what is already drawn there and around it,
    /// like CSS `backdrop-filter: blur()`. The quad itself is drawn afterwards.
    ///
    /// The blur uses Apple's `MPSImageGaussianBlur` at
    /// full resolution with clamped edges: downsampled or sparse-tap blurs
    /// ghost text that sits under the glass.
    fn blur_backdrop(
        &mut self,
        quad: &Quad,
        sigma: f32,
        target: &metal::TextureRef,
        viewport_size: Size<DevicePixels>,
        command_buffer: &metal::CommandBufferRef,
    ) {
        let viewport = [
            i32::from(viewport_size.width) as f32,
            i32::from(viewport_size.height) as f32,
        ];
        // Frosting stops where the quad is clipped, like the quad itself: a
        // pinned header pushed under a toolbar must not blur the toolbar.
        let bounds = quad.bounds.intersect(&quad.content_mask.bounds);
        if bounds.size.width.0 <= 0. || bounds.size.height.0 <= 0. {
            return;
        }
        // The gaussian reaches about 3 sigma, so snapshot that much around
        // the glass: the blur near its edges then averages the backdrop
        // beyond them, as the stylesheet's bleed does, instead of fading.
        let bleed = (sigma * 3.).ceil() + 2.;
        let left = (bounds.origin.x.0 - bleed).floor().max(0.);
        let top = (bounds.origin.y.0 - bleed).floor().max(0.);
        let right = (bounds.origin.x.0 + bounds.size.width.0 + bleed)
            .ceil()
            .min(viewport[0]);
        let bottom = (bounds.origin.y.0 + bounds.size.height.0 + bleed)
            .ceil()
            .min(viewport[1]);
        if right - left < 1. || bottom - top < 1. || sigma <= 0. {
            return;
        }
        let region = ((right - left) as u64, (bottom - top) as u64);
        let format = target.pixel_format();
        let reusable = self.backdrop_scratch.as_ref().is_some_and(|(original, _)| {
            original.width() == region.0
                && original.height() == region.1
                && original.pixel_format() == format
        });
        if !reusable {
            let new_texture = || {
                let descriptor = metal::TextureDescriptor::new();
                descriptor.set_width(region.0);
                descriptor.set_height(region.1);
                descriptor.set_pixel_format(format);
                descriptor.set_usage(
                    metal::MTLTextureUsage::ShaderRead | metal::MTLTextureUsage::ShaderWrite,
                );
                descriptor.set_storage_mode(metal::MTLStorageMode::Private);
                self.device.new_texture(&descriptor)
            };
            self.backdrop_scratch = Some((new_texture(), new_texture()));
        }
        self.backdrop_used = Some(Instant::now());
        let kernel = self.gaussian_kernel(sigma);
        let (original, blurred) = self
            .backdrop_scratch
            .as_ref()
            .expect("just ensured the backdrop scratch exists");

        let blit = command_buffer.new_blit_command_encoder();
        blit.copy_from_texture(
            target,
            0,
            0,
            metal::MTLOrigin {
                x: left as u64,
                y: top as u64,
                z: 0,
            },
            metal::MTLSize {
                width: region.0,
                height: region.1,
                depth: 1,
            },
            original,
            0,
            0,
            metal::MTLOrigin { x: 0, y: 0, z: 0 },
        );
        blit.end_encoding();
        // Safety: all three are live Metal objects owned for this frame, and
        // the selector matches MPSUnaryImageKernel's encoding method.
        unsafe {
            let _: () = msg_send![
                &*kernel,
                encodeToCommandBuffer: command_buffer.as_ptr().cast::<AnyObject>(),
                sourceTexture: original.as_ptr().cast::<AnyObject>(),
                destinationTexture: blurred.as_ptr().cast::<AnyObject>()
            ];
        }

        let params = BackdropCompositeParams {
            origin: [bounds.origin.x.0, bounds.origin.y.0],
            size: [bounds.size.width.0, bounds.size.height.0],
            region_origin: [left, top],
            region_size: [region.0 as f32, region.1 as f32],
            viewport_size: viewport,
            corner_radius: quad.corner_radii.top_left.0,
            padding: 0.,
        };
        let encoder = new_command_encoder_for_texture(command_buffer, target, viewport_size, None);
        encoder.set_render_pipeline_state(&self.backdrop_composite_pipeline_state);
        let params_ptr = &params as *const BackdropCompositeParams as *const c_void;
        let params_len = mem::size_of_val(&params) as u64;
        encoder.set_vertex_bytes(0, params_len, params_ptr);
        encoder.set_fragment_bytes(0, params_len, params_ptr);
        encoder.set_fragment_texture(0, Some(blurred));
        encoder.set_fragment_texture(1, Some(original));
        encoder.draw_primitives(metal::MTLPrimitiveType::Triangle, 0, 6);
        encoder.end_encoding();
    }

    /// The cached `MPSImageGaussianBlur` for `sigma` device pixels.
    fn gaussian_kernel(&mut self, sigma: f32) -> Retained<AnyObject> {
        if let Some(ix) = self
            .backdrop_kernels
            .iter()
            .position(|(cached, _)| (cached - sigma).abs() < 0.01)
        {
            let entry = self.backdrop_kernels.remove(ix);
            let kernel = entry.1.clone();
            self.backdrop_kernels.push(entry);
            return kernel;
        }
        if self.backdrop_kernels.len() >= 4 {
            self.backdrop_kernels.remove(0);
        }
        let class = AnyClass::get(c"MPSImageGaussianBlur")
            .expect("MetalPerformanceShaders provides MPSImageGaussianBlur");
        // Safety: alloc/init returns a +1 reference that Retained takes over;
        // the device pointer is this renderer's live MTLDevice.
        let kernel = unsafe {
            let allocated: *mut AnyObject = msg_send![class, alloc];
            let kernel: *mut AnyObject = msg_send![
                allocated,
                initWithDevice: self.device.as_ptr().cast::<AnyObject>(),
                sigma: sigma
            ];
            let kernel = Retained::from_raw(kernel).expect("MPSImageGaussianBlur init failed");
            // MPSImageEdgeModeClamp: the default zero edge bleeds transparent
            // black into blurs near the window border.
            let _: () = msg_send![&*kernel, setEdgeMode: 1usize];
            kernel
        };
        self.backdrop_kernels.push((sigma, kernel.clone()));
        kernel
    }

    fn draw_quads(
        &self,
        quads: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if quads.is_empty() {
            return;
        }

        command_encoder.set_render_pipeline_state(&self.quads_pipeline_state);
        command_encoder.set_vertex_buffer(
            QuadInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            QuadInputIndex::Quads as u64,
            Some(&instance_bindings.quads.buffer),
            instance_bindings.quads.offset as u64,
        );
        command_encoder.set_fragment_buffer(
            QuadInputIndex::Quads as u64,
            Some(&instance_bindings.quads.buffer),
            instance_bindings.quads.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            QuadInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            quads.len() as u64,
            quads.start as u64,
        );
    }

    fn draw_paths_from_intermediate(
        &self,
        paths: &[Path<ScaledPixels>],
        writer: &mut InstanceBufferWriter,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) -> Result<()> {
        let Some(first_path) = paths.first() else {
            return Ok(());
        };
        let intermediate_texture = self
            .path_intermediate_texture
            .as_ref()
            .context("missing path intermediate texture")?;

        command_encoder.set_render_pipeline_state(&self.path_sprites_pipeline_state);
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        command_encoder.set_fragment_texture(
            SpriteInputIndex::AtlasTexture as u64,
            Some(intermediate_texture),
        );

        // When copying paths from the intermediate texture to the drawable,
        // each pixel must only be copied once, in case of transparent paths.
        //
        // If all paths have the same draw order, then their bounds are all
        // disjoint, so we can copy each path's bounds individually. If this
        // batch combines different draw orders, we perform a single copy
        // for a minimal spanning rect.
        let sprites;
        if paths.last().unwrap().order == first_path.order {
            sprites = paths
                .iter()
                .map(|path| PathSprite {
                    bounds: path.clipped_bounds(),
                })
                .collect();
        } else {
            let mut bounds = first_path.clipped_bounds();
            for path in paths.iter().skip(1) {
                bounds = bounds.union(&path.clipped_bounds());
            }
            sprites = vec![PathSprite { bounds }];
        }

        let sprite_instance_bindings = writer.write(&sprites)?;
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&sprite_instance_bindings.buffer),
            sprite_instance_bindings.offset as u64,
        );

        command_encoder.draw_primitives_instanced(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            sprites.len() as u64,
        );
        Ok(())
    }

    fn draw_underlines(
        &self,
        underlines: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if underlines.is_empty() {
            return;
        }

        command_encoder.set_render_pipeline_state(&self.underlines_pipeline_state);
        command_encoder.set_vertex_buffer(
            UnderlineInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            UnderlineInputIndex::Underlines as u64,
            Some(&instance_bindings.underlines.buffer),
            instance_bindings.underlines.offset as u64,
        );
        command_encoder.set_fragment_buffer(
            UnderlineInputIndex::Underlines as u64,
            Some(&instance_bindings.underlines.buffer),
            instance_bindings.underlines.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            UnderlineInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            underlines.len() as u64,
            underlines.start as u64,
        );
    }

    fn draw_monochrome_sprites(
        &self,
        texture_id: AtlasTextureId,
        sprites: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if sprites.is_empty() {
            return;
        }

        let Some(texture) = self.sprite_atlas.metal_texture(texture_id) else {
            return;
        };
        let texture_size = size(
            DevicePixels(texture.width() as i32),
            DevicePixels(texture.height() as i32),
        );
        command_encoder.set_render_pipeline_state(&self.monochrome_sprites_pipeline_state);
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_bindings.monochrome_sprites.buffer),
            instance_bindings.monochrome_sprites.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::AtlasTextureSize as u64,
            mem::size_of_val(&texture_size) as u64,
            &texture_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_bindings.monochrome_sprites.buffer),
            instance_bindings.monochrome_sprites.offset as u64,
        );
        command_encoder.set_fragment_texture(SpriteInputIndex::AtlasTexture as u64, Some(&texture));

        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            sprites.len() as u64,
            sprites.start as u64,
        );
    }

    fn draw_polychrome_sprites(
        &self,
        texture_id: AtlasTextureId,
        sprites: Range<usize>,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if sprites.is_empty() {
            return;
        }

        let Some(texture) = self.sprite_atlas.metal_texture(texture_id) else {
            return;
        };
        let texture_size = size(
            DevicePixels(texture.width() as i32),
            DevicePixels(texture.height() as i32),
        );
        command_encoder.set_render_pipeline_state(&self.polychrome_sprites_pipeline_state);
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_bindings.polychrome_sprites.buffer),
            instance_bindings.polychrome_sprites.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_vertex_bytes(
            SpriteInputIndex::AtlasTextureSize as u64,
            mem::size_of_val(&texture_size) as u64,
            &texture_size as *const Size<DevicePixels> as *const _,
        );
        command_encoder.set_fragment_buffer(
            SpriteInputIndex::Sprites as u64,
            Some(&instance_bindings.polychrome_sprites.buffer),
            instance_bindings.polychrome_sprites.offset as u64,
        );
        command_encoder.set_fragment_texture(SpriteInputIndex::AtlasTexture as u64, Some(&texture));

        command_encoder.draw_primitives_instanced_base_instance(
            metal::MTLPrimitiveType::Triangle,
            0,
            6,
            sprites.len() as u64,
            sprites.start as u64,
        );
    }

    fn draw_surfaces(
        &mut self,
        surfaces: &[PaintSurface],
        first_surface: usize,
        instance_bindings: &InstanceBindings,
        viewport_size: Size<DevicePixels>,
        command_encoder: &metal::RenderCommandEncoderRef,
    ) {
        if surfaces.is_empty() {
            return;
        }

        command_encoder.set_render_pipeline_state(&self.surfaces_pipeline_state);
        command_encoder.set_vertex_buffer(
            SurfaceInputIndex::Vertices as u64,
            Some(&self.unit_vertices),
            0,
        );
        command_encoder.set_vertex_buffer(
            SurfaceInputIndex::Surfaces as u64,
            Some(&instance_bindings.surfaces.buffer),
            instance_bindings.surfaces.offset as u64,
        );
        command_encoder.set_vertex_bytes(
            SurfaceInputIndex::ViewportSize as u64,
            mem::size_of_val(&viewport_size) as u64,
            &viewport_size as *const Size<DevicePixels> as *const _,
        );

        for (index, surface) in surfaces.iter().enumerate() {
            let image_buffer = &surface.image_buffer;
            let texture_size = size(
                DevicePixels::from(CVPixelBufferGetWidth(image_buffer) as i32),
                DevicePixels::from(CVPixelBufferGetHeight(image_buffer) as i32),
            );

            assert_eq!(
                CVPixelBufferGetPixelFormatType(image_buffer),
                kCVPixelFormatType_420YpCbCr8BiPlanarFullRange
            );

            let plane_texture = |plane, format| unsafe {
                let mut texture = ptr::null_mut();
                let status = CVMetalTextureCache::create_texture_from_image(
                    None,
                    &self.core_video_texture_cache,
                    image_buffer,
                    None,
                    format,
                    CVPixelBufferGetWidthOfPlane(image_buffer, plane),
                    CVPixelBufferGetHeightOfPlane(image_buffer, plane),
                    plane,
                    ptr::NonNull::from(&mut texture),
                );
                assert_eq!(
                    status, kCVReturnSuccess,
                    "failed to create CoreVideo Metal texture"
                );
                CFRetained::<CVMetalTexture>::from_raw(
                    ptr::NonNull::new(texture).expect("CoreVideo returned a null texture"),
                )
            };
            let y_texture = plane_texture(0, objc2_metal::MTLPixelFormat::R8Unorm);
            let cb_cr_texture = plane_texture(1, objc2_metal::MTLPixelFormat::RG8Unorm);

            command_encoder.set_vertex_bytes(
                SurfaceInputIndex::TextureSize as u64,
                mem::size_of_val(&texture_size) as u64,
                &texture_size as *const Size<DevicePixels> as *const _,
            );
            command_encoder.set_fragment_texture(SurfaceInputIndex::YTexture as u64, unsafe {
                let texture = CVMetalTextureGetTexture(&y_texture)
                    .expect("CoreVideo returned no Metal texture");
                Some(metal::TextureRef::from_ptr(
                    objc2::rc::Retained::as_ptr(&texture) as *mut _,
                ))
            });
            command_encoder.set_fragment_texture(SurfaceInputIndex::CbCrTexture as u64, unsafe {
                let texture = CVMetalTextureGetTexture(&cb_cr_texture)
                    .expect("CoreVideo returned no Metal texture");
                Some(metal::TextureRef::from_ptr(
                    objc2::rc::Retained::as_ptr(&texture) as *mut _,
                ))
            });

            command_encoder.draw_primitives_instanced_base_instance(
                metal::MTLPrimitiveType::Triangle,
                0,
                6,
                1,
                (first_surface + index) as u64,
            );
        }
    }
}

fn new_command_encoder_for_texture<'a>(
    command_buffer: &'a metal::CommandBufferRef,
    texture: &'a metal::TextureRef,
    viewport_size: Size<DevicePixels>,
    clear_color: Option<metal::MTLClearColor>,
) -> &'a metal::RenderCommandEncoderRef {
    let render_pass_descriptor = metal::RenderPassDescriptor::new();
    let color_attachment = render_pass_descriptor
        .color_attachments()
        .object_at(0)
        .unwrap();
    color_attachment.set_texture(Some(texture));
    color_attachment.set_store_action(metal::MTLStoreAction::Store);
    if let Some(clear_color) = clear_color {
        color_attachment.set_load_action(metal::MTLLoadAction::Clear);
        color_attachment.set_clear_color(clear_color);
    } else {
        color_attachment.set_load_action(metal::MTLLoadAction::Load);
    }

    let command_encoder = command_buffer.new_render_command_encoder(render_pass_descriptor);
    command_encoder.set_viewport(metal::MTLViewport {
        originX: 0.0,
        originY: 0.0,
        width: i32::from(viewport_size.width) as f64,
        height: i32::from(viewport_size.height) as f64,
        znear: 0.0,
        zfar: 1.0,
    });
    command_encoder
}

#[cfg(any(test, feature = "bench-support", feature = "test-support"))]
fn read_texture_to_image(texture: &metal::TextureRef) -> Result<RgbaImage> {
    let width = texture.width() as u32;
    let height = texture.height() as u32;
    let bytes_per_row = width as usize * 4;
    let mut pixels = vec![0u8; height as usize * bytes_per_row];

    let region = metal::MTLRegion {
        origin: metal::MTLOrigin { x: 0, y: 0, z: 0 },
        size: metal::MTLSize {
            width: width as u64,
            height: height as u64,
            depth: 1,
        },
    };
    texture.get_bytes(
        pixels.as_mut_ptr() as *mut std::ffi::c_void,
        bytes_per_row as u64,
        region,
        0,
    );

    // Convert BGRA to RGBA (swap B and R channels)
    for chunk in pixels.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }

    RgbaImage::from_raw(width, height, pixels).context("failed to create RgbaImage from pixel data")
}

fn build_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::SourceAlpha);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    // Source-over alpha: adding destination alpha makes overlapping translucent
    // shell/pane quads saturate to opaque and produces dark rounded-edge fringes.
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);

    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
}

// Monocode: frosted popovers. The marker lives in gpui so every renderer
// that frosts agrees on it.
pub use gpui::backdrop_blur_marker;
use gpui::backdrop_blur_sigma as backdrop_sigma;
const EDGE_FADE_BEGIN: f32 = 0.271828;
const EDGE_FADE_END: f32 = 0.314159;
const HORIZONTAL_FADE_BEGIN: f32 = 0.161803;
const HORIZONTAL_FADE_END: f32 = 0.141421;
const TEXT_REVEAL_BEGIN: f32 = 0.173205;
const TEXT_REVEAL_END: f32 = 0.264575;
const BACKGROUND_BEGIN: f32 = 0.223607;
const BACKGROUND_END: f32 = 0.244949;
const CORNER_CLIP_BEGIN: f32 = 0.316228;
const CORNER_CLIP_END: f32 = 0.331662;
const EDGE_FADE_MAX: f32 = 4096.;

/// Restore the pixels outside one rounded corner after a group's drawing.
/// The quad covers only the radius-sized corner square. Corners run clockwise
/// from the top left. Only these small tiles are captured, not the whole pane.
pub fn corner_clip_marker(begin: bool, corner: u8) -> Hsla {
    Hsla {
        h: if begin {
            CORNER_CLIP_BEGIN
        } else {
            CORNER_CLIP_END
        },
        s: corner.min(3) as f32 / 4.,
        l: 0.,
        a: 0.,
    }
}

/// Bracket a group's drawing with quads carrying these markers. Their fills
/// are ignored; use a nontransparent fill so GPUI retains the marker quad.
/// Heights are device pixels. The group fades to the actual background drawn
/// before it, preserving native-window transparency and overlapping contents.
pub fn edge_fade_marker(begin: bool, top: f32, bottom: f32) -> Hsla {
    Hsla {
        h: if begin {
            EDGE_FADE_BEGIN
        } else {
            EDGE_FADE_END
        },
        s: (top / EDGE_FADE_MAX).clamp(0., 1.),
        l: (bottom / EDGE_FADE_MAX).clamp(0., 1.),
        a: 0.,
    }
}
/// The same transparent group mask, fading only the right edge. Width is in
/// device pixels, so the ramp stays anchored to the label when it is clipped.
pub fn horizontal_fade_marker(begin: bool, right: f32) -> Hsla {
    side_fade_marker(begin, 0., right)
}
/// The horizontal mask with both sides: `left` and `right` are ramp widths in
/// device pixels, either of which may be zero.
pub fn side_fade_marker(begin: bool, left: f32, right: f32) -> Hsla {
    Hsla {
        h: if begin {
            HORIZONTAL_FADE_BEGIN
        } else {
            HORIZONTAL_FADE_END
        },
        s: (left / EDGE_FADE_MAX).clamp(0., 1.),
        l: (right / EDGE_FADE_MAX).clamp(0., 1.),
        a: 0.,
    }
}
/// CSS linear alpha mask: opaque at `start`, transparent at `end`. Stops
/// are signed device-pixel offsets from the label's left edge, in either order.
pub fn text_reveal_marker(begin: bool, start: f32, end: f32) -> Hsla {
    Hsla {
        h: if begin {
            TEXT_REVEAL_BEGIN
        } else {
            TEXT_REVEAL_END
        },
        s: (start / EDGE_FADE_MAX + 1.) / 2.,
        l: (end / EDGE_FADE_MAX + 1.) / 2.,
        a: 0.,
    }
}
/// A source-image layer with a view-local fade, four rounded corners and
/// optional Haze. Give the marker a 1px left border to carry the display scale.
pub fn background_mask_marker(
    begin: bool,
    empty: bool,
    haze: bool,
    preview: bool,
    opacity: f32,
) -> Hsla {
    let mode = if !haze {
        if empty { 1. } else { 2. }
    } else if preview {
        5.
    } else if empty {
        3.
    } else {
        4.
    };
    Hsla {
        h: if begin {
            BACKGROUND_BEGIN
        } else {
            BACKGROUND_END
        },
        s: mode / 8.,
        l: opacity.clamp(0., 1.),
        a: 0.,
    }
}
fn is_background_marker(quad: &Quad) -> bool {
    matches!(quad.border_color.h, BACKGROUND_BEGIN | BACKGROUND_END)
}
fn edge_fade_kind(quad: &Quad) -> Option<bool> {
    let c = quad.border_color;
    if c.a != 0. {
        return None;
    }
    match c.h {
        EDGE_FADE_BEGIN
        | HORIZONTAL_FADE_BEGIN
        | TEXT_REVEAL_BEGIN
        | BACKGROUND_BEGIN
        | CORNER_CLIP_BEGIN => Some(true),
        EDGE_FADE_END | HORIZONTAL_FADE_END | TEXT_REVEAL_END | BACKGROUND_END
        | CORNER_CLIP_END => Some(false),
        _ => None,
    }
}
fn copy_fade_region(
    command: &metal::CommandBufferRef,
    from: &metal::TextureRef,
    to: &metal::TextureRef,
    region: Bounds<ScaledPixels>,
) {
    let blit = command.new_blit_command_encoder();
    blit.copy_from_texture(
        from,
        0,
        0,
        metal::MTLOrigin {
            x: region.left().0 as u64,
            y: region.top().0 as u64,
            z: 0,
        },
        metal::MTLSize {
            width: region.size.width.0 as u64,
            height: region.size.height.0 as u64,
            depth: 1,
        },
        to,
        0,
        0,
        metal::MTLOrigin { x: 0, y: 0, z: 0 },
    );
    blit.end_encoding();
}

#[repr(C)]
struct BackdropCompositeParams {
    origin: [f32; 2],
    size: [f32; 2],
    region_origin: [f32; 2],
    region_size: [f32; 2],
    viewport_size: [f32; 2],
    corner_radius: f32,
    padding: f32,
}

#[repr(C)]
struct BackgroundCompositeParams {
    origin: [f32; 2],
    size: [f32; 2],
    region_origin: [f32; 2],
    region_size: [f32; 2],
    viewport_size: [f32; 2],
    radii: [f32; 4],
    mode: f32,
    opacity: f32,
    scale: f32,
    padding: f32,
}

/// The backdrop composite overwrites its target outright, mixing the
/// original back in itself for the antialiased rounded edge.
fn build_backdrop_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");
    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(false);
    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
}

fn build_path_sprite_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    // Source-over alpha: adding destination alpha makes overlapping translucent
    // shell/pane quads saturate to opaque and produces dark rounded-edge fringes.
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);

    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
}

fn build_path_rasterization_pipeline_state(
    device: &metal::DeviceRef,
    library: &metal::LibraryRef,
    label: &str,
    vertex_fn_name: &str,
    fragment_fn_name: &str,
    pixel_format: metal::MTLPixelFormat,
    path_sample_count: u32,
) -> metal::RenderPipelineState {
    let vertex_fn = library
        .get_function(vertex_fn_name, None)
        .expect("error locating vertex function");
    let fragment_fn = library
        .get_function(fragment_fn_name, None)
        .expect("error locating fragment function");

    let descriptor = metal::RenderPipelineDescriptor::new();
    descriptor.set_label(label);
    descriptor.set_vertex_function(Some(vertex_fn.as_ref()));
    descriptor.set_fragment_function(Some(fragment_fn.as_ref()));
    if path_sample_count > 1 {
        descriptor.set_raster_sample_count(path_sample_count as _);
        descriptor.set_alpha_to_coverage_enabled(false);
    }
    let color_attachment = descriptor.color_attachments().object_at(0).unwrap();
    color_attachment.set_pixel_format(pixel_format);
    color_attachment.set_blending_enabled(true);
    color_attachment.set_rgb_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_alpha_blend_operation(metal::MTLBlendOperation::Add);
    color_attachment.set_source_rgb_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_source_alpha_blend_factor(metal::MTLBlendFactor::One);
    color_attachment.set_destination_rgb_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);
    color_attachment.set_destination_alpha_blend_factor(metal::MTLBlendFactor::OneMinusSourceAlpha);

    device
        .new_render_pipeline_state(&descriptor)
        .expect("could not create render pipeline state")
}

#[derive(Clone)]
struct InstanceBinding {
    buffer: metal::Buffer,
    offset: usize,
}

struct InstanceBindings {
    quads: InstanceBinding,
    shadows: InstanceBinding,
    underlines: InstanceBinding,
    monochrome_sprites: InstanceBinding,
    polychrome_sprites: InstanceBinding,
    surfaces: InstanceBinding,
}

fn write_instances(scene: &Scene, writer: &mut InstanceBufferWriter) -> Result<InstanceBindings> {
    Ok(InstanceBindings {
        quads: writer.write(&scene.quads)?,
        shadows: writer.write(&scene.shadows)?,
        underlines: writer.write(&scene.underlines)?,
        monochrome_sprites: writer.write(&scene.monochrome_sprites)?,
        polychrome_sprites: writer.write(&scene.polychrome_sprites)?,
        surfaces: writer.write_iter(scene.surfaces.iter().map(|surface| SurfaceBounds {
            bounds: surface.bounds,
            content_mask: surface.content_mask,
        }))?,
    })
}

struct InstanceBufferWriter {
    device: metal::Device,
    pool: Arc<Mutex<InstanceBufferPool>>,
    unified_memory: bool,
    filled: Vec<(InstanceBuffer, usize)>,
    current: InstanceBuffer,
    offset: usize,
}

impl InstanceBufferWriter {
    fn used(&self) -> usize {
        self.filled
            .iter()
            .map(|(_, written)| *written)
            .sum::<usize>()
            + self.offset
    }

    fn new(
        device: &metal::Device,
        pool: &Arc<Mutex<InstanceBufferPool>>,
        unified_memory: bool,
    ) -> Self {
        let current = pool.lock().acquire(device, unified_memory);
        Self {
            device: device.clone(),
            pool: pool.clone(),
            unified_memory,
            filled: Vec::new(),
            current,
            offset: 0,
        }
    }

    fn allocate<T>(&mut self, count: usize) -> Result<(InstanceBinding, &mut [MaybeUninit<T>])> {
        let size = mem::size_of::<T>() * count;
        let mut offset = self.offset.next_multiple_of(INSTANCE_BUFFER_ALIGNMENT);
        if offset + size > self.current.size {
            self.grow(size)?;
            offset = 0;
        }
        self.offset = offset + size;

        let binding = InstanceBinding {
            buffer: self.current.metal_buffer.clone(),
            offset,
        };
        // Safety: the reservation lies within a buffer this frame owns
        // exclusively, and never overlaps one handed out earlier.
        let values = unsafe {
            let start = (self.current.metal_buffer.contents() as *mut u8).add(offset);
            slice::from_raw_parts_mut(start.cast::<MaybeUninit<T>>(), count)
        };
        Ok((binding, values))
    }

    fn write<T>(&mut self, values: &[T]) -> Result<InstanceBinding> {
        let (binding, destination) = self.allocate::<T>(values.len())?;
        unsafe {
            ptr::copy_nonoverlapping(
                values.as_ptr(),
                destination.as_mut_ptr().cast::<T>(),
                values.len(),
            );
        }
        Ok(binding)
    }

    fn write_iter<T>(
        &mut self,
        values: impl ExactSizeIterator<Item = T>,
    ) -> Result<InstanceBinding> {
        let (binding, destination) = self.allocate::<T>(values.len())?;
        for (slot, value) in destination.iter_mut().zip(values) {
            slot.write(value);
        }
        Ok(binding)
    }

    fn grow(&mut self, required: usize) -> Result<()> {
        let mut pool = self.pool.lock();
        let buffer_size = (pool.buffer_size * 2)
            .max(required.next_power_of_two())
            .min(MAX_INSTANCE_BUFFER_SIZE);
        anyhow::ensure!(
            buffer_size >= required,
            "instance buffer needs {required} bytes, above the maximum of {MAX_INSTANCE_BUFFER_SIZE}"
        );
        anyhow::ensure!(
            buffer_size > self.current.size,
            "frame instance data exceeds the {MAX_INSTANCE_BUFFER_SIZE}-byte maximum"
        );
        if buffer_size != pool.buffer_size {
            log::info!("increased instance buffer size to {buffer_size}");
            pool.reset(buffer_size);
        }
        let buffer = pool.acquire(&self.device, self.unified_memory);
        drop(pool);

        let filled = mem::replace(&mut self.current, buffer);
        self.filled.push((filled, self.offset));
        self.offset = 0;
        Ok(())
    }

    fn finish(self) -> InstanceBuffer {
        let Self {
            unified_memory,
            filled,
            current,
            offset,
            ..
        } = self;

        if !unified_memory {
            for (buffer, written) in &filled {
                if *written == 0 {
                    continue;
                }
                buffer.metal_buffer.did_modify_range(NSRange {
                    location: 0,
                    length: *written as NSUInteger,
                });
            }
            if offset > 0 {
                current.metal_buffer.did_modify_range(NSRange {
                    location: 0,
                    length: offset as NSUInteger,
                });
            }
        }

        // Metal retains encoded resources until the command buffer completes.
        // Only the final, largest buffer is worth keeping in the pool.
        drop(filled);
        current
    }
}

#[repr(C)]
enum ShadowInputIndex {
    Vertices = 0,
    Shadows = 1,
    ViewportSize = 2,
}

#[repr(C)]
enum QuadInputIndex {
    Vertices = 0,
    Quads = 1,
    ViewportSize = 2,
}

#[repr(C)]
enum UnderlineInputIndex {
    Vertices = 0,
    Underlines = 1,
    ViewportSize = 2,
}

#[repr(C)]
enum SpriteInputIndex {
    Vertices = 0,
    Sprites = 1,
    ViewportSize = 2,
    AtlasTextureSize = 3,
    AtlasTexture = 4,
}

#[repr(C)]
enum SurfaceInputIndex {
    Vertices = 0,
    Surfaces = 1,
    ViewportSize = 2,
    TextureSize = 3,
    YTexture = 4,
    CbCrTexture = 5,
}

#[repr(C)]
enum PathRasterizationInputIndex {
    Vertices = 0,
    ViewportSize = 1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct PathSprite {
    pub bounds: Bounds<ScaledPixels>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct SurfaceBounds {
    pub bounds: Bounds<ScaledPixels>,
    pub content_mask: ContentMask<ScaledPixels>,
}

#[cfg(any(test, feature = "bench-support", feature = "test-support"))]
pub struct MetalHeadlessRenderer {
    renderer: MetalRenderer,
}

#[cfg(any(test, feature = "bench-support", feature = "test-support"))]
impl MetalHeadlessRenderer {
    pub fn new() -> Self {
        let instance_buffer_pool = Arc::new(Mutex::new(InstanceBufferPool::default()));
        let renderer = MetalRenderer::new_headless(instance_buffer_pool);
        Self { renderer }
    }
}

#[cfg(any(test, feature = "bench-support", feature = "test-support"))]
impl gpui::PlatformHeadlessRenderer for MetalHeadlessRenderer {
    fn render_scene_to_image(
        &mut self,
        scene: &Scene,
        size: Size<DevicePixels>,
    ) -> anyhow::Result<image::RgbaImage> {
        self.renderer.render_scene_to_image(scene, size)
    }

    fn render_scene(&mut self, scene: &Scene, size: Size<DevicePixels>) -> anyhow::Result<()> {
        self.renderer.render_scene(scene, size)
    }

    fn sprite_atlas(&self) -> Arc<dyn gpui::PlatformAtlas> {
        self.renderer.sprite_atlas().clone()
    }
}

fn touch(used: &mut Vec<Instant>, depth: usize) {
    let now = Instant::now();
    if used.len() <= depth {
        used.resize(depth + 1, now);
    }
    used[depth] = now;
}

fn texture_bytes(texture: &metal::TextureRef) -> u64 {
    texture.width() * texture.height() * 4
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_core_foundation::{CFDictionary, CFType};
    use objc2_core_video::{
        CVPixelBufferCreate, CVPixelBufferGetBaseAddressOfPlane,
        CVPixelBufferGetBytesPerRowOfPlane, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
        CVPixelBufferUnlockBaseAddress, kCVPixelBufferIOSurfacePropertiesKey,
    };

    #[test]
    fn render_core_video_surface() {
        let attributes = CFDictionary::from_slices(
            &[unsafe { kCVPixelBufferIOSurfacePropertiesKey }],
            &[&*CFDictionary::<CFType, CFType>::empty()],
        );
        let mut image_buffer = ptr::null_mut();
        assert_eq!(
            unsafe {
                CVPixelBufferCreate(
                    None,
                    16,
                    16,
                    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
                    Some(attributes.as_opaque()),
                    ptr::NonNull::from(&mut image_buffer),
                )
            },
            kCVReturnSuccess
        );
        let image_buffer =
            unsafe { CFRetained::from_raw(ptr::NonNull::new(image_buffer).unwrap()) };
        unsafe {
            assert_eq!(
                CVPixelBufferLockBaseAddress(&image_buffer, CVPixelBufferLockFlags::empty()),
                kCVReturnSuccess
            );
            for plane in 0..2 {
                let bytes = CVPixelBufferGetBaseAddressOfPlane(&image_buffer, plane).cast::<u8>();
                let length = CVPixelBufferGetBytesPerRowOfPlane(&image_buffer, plane)
                    * CVPixelBufferGetHeightOfPlane(&image_buffer, plane);
                slice::from_raw_parts_mut(bytes, length).fill(128);
            }
            assert_eq!(
                CVPixelBufferUnlockBaseAddress(&image_buffer, CVPixelBufferLockFlags::empty()),
                kCVReturnSuccess
            );
        }

        let bounds = Bounds {
            origin: Point::default(),
            size: size(ScaledPixels(16.0), ScaledPixels(16.0)),
        };
        let mut scene = Scene::default();
        scene.insert_primitive(PaintSurface {
            order: 0,
            bounds,
            content_mask: ContentMask { bounds },
            image_buffer: image_buffer.clone(),
        });
        drop(image_buffer);
        scene.finish();

        let mut renderer = MetalRenderer::new_headless(Arc::new(Mutex::new(Default::default())));
        let image = renderer
            .render_scene_to_image(&scene, size(DevicePixels(16), DevicePixels(16)))
            .unwrap();
        for image::Rgba([red, green, blue, alpha]) in image.pixels() {
            assert!(red.abs_diff(128) <= 2);
            assert!(green.abs_diff(128) <= 2);
            assert!(blue.abs_diff(128) <= 2);
            assert_eq!(*alpha, 255);
        }
    }
}
