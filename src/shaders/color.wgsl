// IEEE exponent tests avoid a NaN != NaN guard that fast floating-point math may fold.
fn finite_hdr(value: vec3<f32>) -> vec3<f32> {
    let exponent = bitcast<vec3<u32>>(value) & vec3<u32>(0x7f800000u);
    let safe = select(value, vec3<f32>(0.0), exponent == vec3<u32>(0x7f800000u));
    return clamp(safe, vec3<f32>(0.0), vec3<f32>(65504.0));
}

fn linear_to_srgb(value: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(value, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055),
        12.92 * value, value <= vec3<f32>(0.0031308));
}
