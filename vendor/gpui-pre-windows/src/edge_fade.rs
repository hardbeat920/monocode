//! Fade a composite group against its actual backdrop, including its alpha.
use super::directx_renderer::{
    create_constant_buffer, create_fragment_shader, create_vertex_shader,
    shader_resources::{RawShaderBytes, ShaderModule, ShaderTarget},
    update_buffer,
};
use anyhow::{Context, Result};
use gpui::{Bounds, EdgeFade, Quad, ScaledPixels};
use windows::Win32::Graphics::{Direct3D::*, Direct3D11::*, Dxgi::Common::*};

#[repr(C)]
struct Params {
    region: [f32; 4],
    bounds: [f32; 4],
    viewport: [f32; 2],
    ramps: [f32; 2],
}

struct Snapshot {
    texture: ID3D11Texture2D,
    view: Option<ID3D11ShaderResourceView>,
}
struct Capture {
    size: [u32; 2],
    before: Snapshot,
    after: Snapshot,
}
struct Layer {
    bounds: Bounds<ScaledPixels>,
    capture: Option<(usize, [u32; 4])>,
}
pub(crate) struct EdgeFades {
    vertex: ID3D11VertexShader,
    fragment: ID3D11PixelShader,
    params: Option<ID3D11Buffer>,
    replace: ID3D11BlendState,
    captures: Vec<Capture>,
    layers: Vec<Layer>,
}

pub(crate) struct FadeTarget<'a> {
    pub device: &'a ID3D11Device,
    pub context: &'a ID3D11DeviceContext,
    pub texture: &'a ID3D11Texture2D,
    pub view: &'a Option<ID3D11RenderTargetView>,
    pub viewport: [f32; 2],
}

impl EdgeFades {
    pub fn new(device: &ID3D11Device) -> Result<Self> {
        let vertex = RawShaderBytes::new(ShaderModule::EdgeFade, ShaderTarget::Vertex)?;
        let fragment = RawShaderBytes::new(ShaderModule::EdgeFade, ShaderTarget::Fragment)?;
        let mut replace = None;
        let mut desc = D3D11_BLEND_DESC::default();
        desc.RenderTarget[0].RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
        unsafe { device.CreateBlendState(&desc, Some(&mut replace))? };
        Ok(Self {
            vertex: create_vertex_shader(device, vertex.as_bytes())?,
            fragment: create_fragment_shader(device, fragment.as_bytes())?,
            params: create_constant_buffer::<Params>(device)?,
            replace: replace.context("edge fade blend state")?,
            captures: Vec::new(),
            layers: Vec::new(),
        })
    }

    pub fn prepare(&mut self) {
        self.layers.clear();
    }

    pub fn paint(&mut self, target: FadeTarget<'_>, quad: &Quad, fade: EdgeFade) -> Result<()> {
        let FadeTarget {
            device,
            context,
            texture: target,
            view: target_view,
            viewport,
        } = target;
        match fade {
            EdgeFade::Begin { top, bottom } => {
                let clipped = quad.bounds.intersect(&quad.content_mask.bounds);
                let left = clipped.left().0.ceil().clamp(0., viewport[0]) as u32;
                let top_y = clipped.top().0.ceil().clamp(0., viewport[1]) as u32;
                let right = clipped.right().0.floor().clamp(0., viewport[0]) as u32;
                let bottom_y = clipped.bottom().0.floor().clamp(0., viewport[1]) as u32;
                let region = [
                    left,
                    top_y,
                    right.saturating_sub(left),
                    bottom_y.saturating_sub(top_y),
                ];
                let capture = if (top > 0. || bottom > 0.) && region[2] > 0 && region[3] > 0 {
                    let slot = (0..)
                        .find(|slot| {
                            !self
                                .layers
                                .iter()
                                .any(|layer| layer.capture.is_some_and(|(used, _)| used == *slot))
                        })
                        .unwrap();
                    let size = [region[2], region[3]];
                    if slot == self.captures.len() {
                        self.captures.push(Capture::new(device, size)?);
                    } else if self.captures[slot].size != size {
                        self.captures[slot] = Capture::new(device, size)?;
                    }
                    copy_region(context, &self.captures[slot].before.texture, target, region);
                    Some((slot, region))
                } else {
                    None
                };
                self.layers.push(Layer {
                    bounds: quad.bounds,
                    capture,
                });
            }
            EdgeFade::End { top, bottom } => {
                // Disjoint groups may batch as begin/begin/end/end; match by bounds.
                let Some(index) = self
                    .layers
                    .iter()
                    .rposition(|layer| layer.bounds == quad.bounds)
                else {
                    return Ok(());
                };
                let layer = self.layers.remove(index);
                let Some((slot, region)) = layer.capture else {
                    return Ok(());
                };
                let capture = &self.captures[slot];
                copy_region(context, &capture.after.texture, target, region);
                update_buffer(
                    context,
                    self.params.as_ref().context("edge fade constants")?,
                    &[Params {
                        region: region.map(|n| n as f32),
                        bounds: [
                            quad.bounds.left().0,
                            quad.bounds.top().0,
                            quad.bounds.size.width.0,
                            quad.bounds.size.height.0,
                        ],
                        viewport,
                        ramps: [top, bottom],
                    }],
                )?;
                unsafe {
                    context.OMSetRenderTargets(Some(std::slice::from_ref(target_view)), None);
                    context.VSSetShader(&self.vertex, None);
                    context.PSSetShader(&self.fragment, None);
                    context.VSSetConstantBuffers(2, Some(std::slice::from_ref(&self.params)));
                    context.PSSetConstantBuffers(2, Some(std::slice::from_ref(&self.params)));
                    context.PSSetShaderResources(
                        2,
                        Some(&[capture.before.view.clone(), capture.after.view.clone()]),
                    );
                    context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
                    // Snapshots are already composited. Replace all four channels once.
                    context.OMSetBlendState(&self.replace, None, u32::MAX);
                    context.Draw(6, 0);
                    context.PSSetShaderResources(2, Some(&[None, None]));
                }
            }
        }
        // Copying temporarily unbinds the render target, including at Begin.
        unsafe { context.OMSetRenderTargets(Some(std::slice::from_ref(target_view)), None) };
        Ok(())
    }
}

impl Capture {
    fn new(device: &ID3D11Device, size: [u32; 2]) -> Result<Self> {
        Ok(Self {
            size,
            before: Snapshot::new(device, size)?,
            after: Snapshot::new(device, size)?,
        })
    }
}
impl Snapshot {
    fn new(device: &ID3D11Device, size: [u32; 2]) -> Result<Self> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: size[0],
            Height: size[1],
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            ..Default::default()
        };
        let mut texture = None;
        unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture))? };
        let texture = texture.context("edge fade snapshot")?;
        let mut view = None;
        unsafe { device.CreateShaderResourceView(&texture, None, Some(&mut view))? };
        Ok(Self { texture, view })
    }
}
fn copy_region(
    context: &ID3D11DeviceContext,
    destination: &ID3D11Texture2D,
    source: &ID3D11Texture2D,
    region: [u32; 4],
) {
    let bounds = D3D11_BOX {
        left: region[0],
        top: region[1],
        front: 0,
        right: region[0] + region[2],
        bottom: region[1] + region[3],
        back: 1,
    };
    unsafe {
        context.OMSetRenderTargets(None, None);
        context.CopySubresourceRegion(destination, 0, 0, 0, 0, source, 0, Some(&bounds));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{BorderStyle, ContentMask, Corners, Edges, Hsla, Point, Size};

    fn bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds<ScaledPixels> {
        Bounds::new(
            Point::new(x.into(), y.into()),
            Size::new(width.into(), height.into()),
        )
    }
    fn quad(bounds: Bounds<ScaledPixels>, clip: Bounds<ScaledPixels>) -> Quad {
        Quad {
            order: 0,
            border_style: BorderStyle::Solid,
            bounds,
            content_mask: ContentMask { bounds: clip },
            background: gpui::black().into(),
            border_color: Hsla::default(),
            corner_radii: Corners::default(),
            border_widths: Edges::default(),
        }
    }
    struct Target {
        devices: crate::DirectXDevices,
        texture: ID3D11Texture2D,
        view: Option<ID3D11RenderTargetView>,
        fades: EdgeFades,
    }
    impl Target {
        fn new() -> Self {
            let devices = crate::DirectXDevices::new().unwrap();
            let mut texture = None;
            unsafe {
                devices
                    .device
                    .CreateTexture2D(
                        &D3D11_TEXTURE2D_DESC {
                            Width: 32,
                            Height: 32,
                            MipLevels: 1,
                            ArraySize: 1,
                            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                            SampleDesc: DXGI_SAMPLE_DESC {
                                Count: 1,
                                Quality: 0,
                            },
                            Usage: D3D11_USAGE_DEFAULT,
                            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
                            ..Default::default()
                        },
                        None,
                        Some(&mut texture),
                    )
                    .unwrap()
            };
            let texture = texture.unwrap();
            let mut view = None;
            unsafe {
                devices
                    .device
                    .CreateRenderTargetView(&texture, None, Some(&mut view))
                    .unwrap();
                super::super::directx_renderer::set_rasterizer_state(
                    &devices.device,
                    &devices.device_context,
                )
                .unwrap();
                devices
                    .device_context
                    .RSSetViewports(Some(&[D3D11_VIEWPORT {
                        Width: 32.,
                        Height: 32.,
                        MinDepth: 0.,
                        MaxDepth: 1.,
                        ..Default::default()
                    }]));
            }
            let fades = EdgeFades::new(&devices.device).unwrap();
            Self {
                devices,
                texture,
                view,
                fades,
            }
        }
        fn clear(&self, color: [f32; 4]) {
            unsafe {
                self.devices
                    .device_context
                    .ClearRenderTargetView(self.view.as_ref().unwrap(), &color)
            };
        }
        fn paint(&mut self, quad: &Quad, fade: EdgeFade) {
            self.fades
                .paint(
                    FadeTarget {
                        device: &self.devices.device,
                        context: &self.devices.device_context,
                        texture: &self.texture,
                        view: &self.view,
                        viewport: [32., 32.],
                    },
                    quad,
                    fade,
                )
                .unwrap();
        }
        fn pixels(&self) -> image::RgbaImage {
            let mut staging = None;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            let context = &self.devices.device_context;
            unsafe {
                self.texture.GetDesc(&mut desc);
                desc.Usage = D3D11_USAGE_STAGING;
                desc.BindFlags = 0;
                desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
                self.devices
                    .device
                    .CreateTexture2D(&desc, None, Some(&mut staging))
                    .unwrap();
                let staging = staging.unwrap();
                context.CopyResource(&staging, &self.texture);
                let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                context
                    .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                    .unwrap();
                let result = image::RgbaImage::from_fn(32, 32, |x, y| {
                    let pixel =
                        (mapped.pData as *const u8).add((y * mapped.RowPitch + x * 4) as usize);
                    image::Rgba([*pixel.add(2), *pixel.add(1), *pixel, *pixel.add(3)])
                });
                context.Unmap(&staging, 0);
                result
            }
        }
    }
    fn assert_color(actual: [u8; 4], expected: [f32; 4]) {
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!(
                (actual as f32 - expected * 255.).abs() <= 2.,
                "{actual} != {}",
                expected * 255.
            );
        }
    }

    #[test]
    fn group_alpha_mask_preserves_backdrop_and_has_independent_edge_ramps() {
        let mut target = Target::new();
        let before = [0.12, 0.24, 0.4, 0.55];
        let after = [0.7, 0.6, 0.2, 0.9];
        let quad = quad(bounds(0., 0., 32., 32.), bounds(0., 0., 32., 32.));
        for (top, bottom) in [(8., 12.), (8., 0.), (0., 12.), (0., 0.), (24., 24.)] {
            target.fades.prepare();
            target.clear(before);
            target.paint(&quad, EdgeFade::Begin { top, bottom });
            target.clear(after);
            target.paint(&quad, EdgeFade::End { top, bottom });
            let image = target.pixels();
            for y in 0..32 {
                let y_mid = y as f32 + 0.5;
                let coverage = (if top > 0. {
                    (y_mid / top).clamp(0., 1.)
                } else {
                    1.0_f32
                })
                .min(if bottom > 0. {
                    ((32. - y_mid) / bottom).clamp(0., 1.)
                } else {
                    1.
                });
                assert_color(
                    image.get_pixel(16, y).0,
                    std::array::from_fn(|c| before[c] + (after[c] - before[c]) * coverage),
                );
            }
        }
    }

    #[test]
    fn empty_group_cannot_tint_a_translucent_background() {
        let mut target = Target::new();
        let color = [0.09, 0.16, 0.23, 0.4];
        target.clear(color);
        let quad = quad(bounds(2., -4., 28., 40.), bounds(4., 3., 20., 24.));
        target.paint(
            &quad,
            EdgeFade::Begin {
                top: 16.,
                bottom: 16.,
            },
        );
        target.paint(
            &quad,
            EdgeFade::End {
                top: 16.,
                bottom: 16.,
            },
        );
        for pixel in target.pixels().pixels() {
            assert_color(pixel.0, color);
        }
    }

    #[test]
    fn disjoint_batched_groups_and_nested_groups_keep_separate_snapshots() {
        let mut target = Target::new();
        let full = bounds(0., 0., 32., 32.);
        let left = quad(bounds(0., 0., 16., 32.), full);
        let right = quad(bounds(16., 0., 16., 32.), full);
        let begin = EdgeFade::Begin {
            top: 8.,
            bottom: 8.,
        };
        let end = EdgeFade::End {
            top: 8.,
            bottom: 8.,
        };
        target.clear([0.; 4]);
        target.paint(&left, begin);
        target.paint(&right, begin);
        target.clear([1.; 4]);
        target.paint(&left, end);
        target.paint(&right, end);
        let image = target.pixels();
        for x in [4, 20] {
            assert_color(image.get_pixel(x, 2).0, [2.5 / 8.; 4]);
        }

        target.fades.prepare();
        let outer = quad(full, full);
        let inner = quad(bounds(4., 4., 24., 24.), full);
        target.clear([0.; 4]);
        target.paint(&outer, begin);
        target.clear([0.3; 4]);
        target.paint(&inner, begin);
        target.clear([1.; 4]);
        target.paint(&inner, end);
        target.paint(&outer, end);
        let image = target.pixels();
        for y in 4..28 {
            let outer = ((y as f32 + 0.5) / 8.)
                .min((31.5 - y as f32) / 8.)
                .clamp(0., 1.);
            let inner = ((y as f32 - 3.5) / 8.)
                .min((27.5 - y as f32) / 8.)
                .clamp(0., 1.);
            assert_color(image.get_pixel(16, y).0, [(0.3 + 0.7 * inner) * outer; 4]);
        }
    }

    #[test]
    fn clipped_mask_uses_unclipped_ramp_coordinates_and_leaves_outside_untouched() {
        let mut target = Target::new();
        let quad = quad(bounds(0., -4., 32., 40.), bounds(4., 3., 20., 24.));
        target.clear([0., 0., 0., 0.]);
        target.paint(
            &quad,
            EdgeFade::Begin {
                top: 16.,
                bottom: 16.,
            },
        );
        target.clear([0.8, 0.4, 0.2, 1.]);
        target.paint(
            &quad,
            EdgeFade::End {
                top: 16.,
                bottom: 16.,
            },
        );
        let image = target.pixels();
        for y in 0..32 {
            for x in 0..32 {
                let amount = if (4..24).contains(&x) && (3..27).contains(&y) {
                    ((y as f32 + 4.5) / 16.)
                        .min((35.5 - y as f32) / 16.)
                        .clamp(0., 1.)
                } else {
                    1.
                };
                assert_color(
                    image.get_pixel(x, y).0,
                    [0.8 * amount, 0.4 * amount, 0.2 * amount, amount],
                );
            }
        }
    }
}
