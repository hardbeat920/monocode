struct Params {
    region: vec4<u32>,
    bounds: vec4<f32>,
    viewport: vec2<f32>,
    ramps: vec2<f32>,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var before: texture_2d<f32>;
@group(0) @binding(2) var after: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let vertices = array<vec2<f32>, 6>(
        vec2(0., 0.), vec2(1., 0.), vec2(0., 1.),
        vec2(0., 1.), vec2(1., 0.), vec2(1., 1.),
    );
    let pixel = vec2<f32>(params.region.xy)
        + vertices[index] * vec2<f32>(params.region.zw);
    return vec4(pixel / params.viewport * vec2(2., -2.) + vec2(-1., 1.), 0., 1.);
}

@fragment
fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(position.xy) - vec2<i32>(params.region.xy);
    let y = position.y - params.bounds.y;
    var top = 1.;
    var bottom = 1.;
    if params.ramps.x > 0. {
        top = clamp(y / params.ramps.x, 0., 1.);
    }
    if params.ramps.y > 0. {
        bottom = clamp((params.bounds.w - y) / params.ramps.y, 0., 1.);
    }
    // Both snapshots contain the render target's premultiplied color and alpha.
    // Replacing the pixel preserves its actual backdrop, including transparency.
    return mix(textureLoad(before, pixel, 0), textureLoad(after, pixel, 0), min(top, bottom));
}
