// The only scene exposure/tonemap/transfer stage. HUD is composed afterwards.
struct Parameters { exposure: f32, manual_srgb: f32, hud: f32, padding: f32 };
@group(0) @binding(0) var hdr: texture_2d<f32>;
@group(0) @binding(1) var<uniform> parameters: Parameters;

@vertex
fn fullscreen(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let points = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    return vec4<f32>(points[index], 0.0, 1.0);
}

fn display_transform(linear_color: vec3<f32>) -> vec3<f32> {
    let c = finite_hdr(linear_color) * parameters.exposure;
    return clamp((c * (2.51 * c + vec3<f32>(0.03)))
        / (c * (2.43 * c + vec3<f32>(0.59)) + vec3<f32>(0.14)), vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn display(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    // Pixel-exact, no filtering or implicit upscaling, no history surviving destruction.
    var color = display_transform(textureLoad(hdr, vec2<i32>(position.xy), 0).rgb);
    let ndc = abs(position.xy / vec2<f32>(textureDimensions(hdr)) * 2.0 - vec2<f32>(1.0));
    if parameters.hud > 0.5 && ((ndc.x < 0.0012 && ndc.y < 0.018)
        || (ndc.x < 0.010 && ndc.y < 0.0021)) {
        // Composite once in display-linear space BEFORE the transfer function. Fixed-function
        // alpha blending into a non-sRGB surface would incorrectly blend encoded values.
        color = mix(color, vec3<f32>(0.96, 0.98, 1.0), 0.92);
    }
    if parameters.manual_srgb > 0.5 { color = linear_to_srgb(color); }
    return vec4<f32>(color, 1.0);
}
