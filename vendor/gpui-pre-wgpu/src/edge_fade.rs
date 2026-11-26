//! Composite a group's drawing against its captured backdrop, as Metal does.
use bytemuck::{Pod, Zeroable};
use gpui::{Bounds, Quad, ScaledPixels};
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    region: [u32; 4],
    bounds: [f32; 4],
    viewport: [f32; 2],
    ramps: [f32; 2],
}

struct Capture {
    before: wgpu::Texture,
    after: wgpu::Texture,
}

struct Layer {
    bounds: Bounds<ScaledPixels>,
    capture: Option<(usize, [u32; 4])>,
}

pub(crate) struct EdgeFades {
    frame: wgpu::Texture,
    pub view: wgpu::TextureView,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    captures: Vec<Capture>,
    layers: Vec<Layer>,
}

impl EdgeFades {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, size: [u32; 2]) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("edge_fade_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture_entry(1),
                texture_entry(2),
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("edge_fade_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("edge_fade.wgsl").into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("edge_fade_pipeline"),
            layout: Some(
                &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("edge_fade_pipeline_layout"),
                    bind_group_layouts: &[Some(&layout)],
                    immediate_size: 0,
                }),
            ),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let frame = texture(device, format, size);
        let view = frame.create_view(&Default::default());
        Self {
            frame,
            view,
            pipeline,
            layout,
            captures: Vec::new(),
            layers: Vec::new(),
        }
    }

    pub fn prepare(&mut self, device: &wgpu::Device, size: [u32; 2]) {
        self.layers.clear();
        if [self.frame.width(), self.frame.height()] != size {
            self.frame = texture(device, self.frame.format(), size);
            self.view = self.frame.create_view(&Default::default());
            self.captures.clear();
        }
    }

    pub fn begin(
        &mut self,
        device: &wgpu::Device,
        quad: &Quad,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        let clipped = quad.bounds.intersect(&quad.content_mask.bounds);
        // Include pixels whose centers lie inside the clipped bounds.
        let pixel_edge =
            |bound: f32, limit: u32| (bound - 0.5).ceil().clamp(0., limit as f32) as u32;
        let left = pixel_edge(clipped.left().0, self.frame.width());
        let top = pixel_edge(clipped.top().0, self.frame.height());
        let right = pixel_edge(clipped.right().0, self.frame.width());
        let bottom = pixel_edge(clipped.bottom().0, self.frame.height());
        let region = [
            left,
            top,
            right.saturating_sub(left),
            bottom.saturating_sub(top),
        ];
        let capture = if region[2] > 0 && region[3] > 0 {
            // Disjoint groups can be batched begin/begin/end/end. Pair by bounds
            // and reuse only slots that do not belong to an active group.
            let mut slot = 0;
            while self
                .layers
                .iter()
                .any(|layer| layer.capture.is_some_and(|(used, _)| used == slot))
            {
                slot += 1;
            }
            let size = [region[2], region[3]];
            if self
                .captures
                .get(slot)
                .is_none_or(|capture| [capture.before.width(), capture.before.height()] != size)
            {
                let capture = Capture {
                    before: texture(device, self.frame.format(), size),
                    after: texture(device, self.frame.format(), size),
                };
                if slot == self.captures.len() {
                    self.captures.push(capture);
                } else {
                    self.captures[slot] = capture;
                }
            }
            copy_region(encoder, &self.frame, &self.captures[slot].before, region);
            Some((slot, region))
        } else {
            None
        };
        self.layers.push(Layer {
            bounds: quad.bounds,
            capture,
        });
    }

    pub fn end(
        &mut self,
        device: &wgpu::Device,
        quad: &Quad,
        ramps: [f32; 2],
        encoder: &mut wgpu::CommandEncoder,
    ) {
        let Some(index) = self
            .layers
            .iter()
            .rposition(|layer| layer.bounds == quad.bounds)
        else {
            return;
        };
        let Some((slot, region)) = self.layers.remove(index).capture else {
            return;
        };
        let capture = &self.captures[slot];
        copy_region(encoder, &self.frame, &capture.after, region);
        self.composite(
            device,
            encoder,
            &self.view,
            [&capture.before, &capture.after],
            Params {
                region,
                bounds: [
                    quad.bounds.left().0,
                    quad.bounds.top().0,
                    quad.bounds.size.width.0,
                    quad.bounds.size.height.0,
                ],
                viewport: [self.frame.width() as f32, self.frame.height() as f32],
                ramps,
            },
        );
    }

    pub fn present(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
    ) {
        let width = self.frame.width();
        let height = self.frame.height();
        self.composite(
            device,
            encoder,
            target,
            [&self.frame, &self.frame],
            Params {
                region: [0, 0, width, height],
                bounds: [0., 0., width as f32, height as f32],
                viewport: [width as f32, height as f32],
                ramps: [0., 0.],
            },
        );
    }

    fn composite(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        sources: [&wgpu::Texture; 2],
        params: Params,
    ) {
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("edge_fade_params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let before = sources[0].create_view(&Default::default());
        let after = sources[1].create_view(&Default::default());
        let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("edge_fade_bindings"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&before),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&after),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("edge_fade_composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.draw(0..6, 0..1);
    }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn texture(device: &wgpu::Device, format: wgpu::TextureFormat, size: [u32; 2]) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("edge_fade_texture"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn copy_region(
    encoder: &mut wgpu::CommandEncoder,
    from: &wgpu::Texture,
    to: &wgpu::Texture,
    region: [u32; 4],
) {
    encoder.copy_texture_to_texture(
        wgpu::TexelCopyTextureInfo {
            texture: from,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: region[0],
                y: region[1],
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyTextureInfo {
            texture: to,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::Extent3d {
            width: region[2],
            height: region[3],
            depth_or_array_layers: 1,
        },
    );
}
