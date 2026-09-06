// Frozen sample_scanned from parent 2224b89, with an explicit soil-plane hook only.
fn reference_scanned(
    input: VertexOutput,
    position_dx: vec3<f32>,
    position_dy: vec3<f32>,
) -> SurfaceSample {
    let local_normal = normalize(input.material_normal);
    let layer = i32(input.material - 1u);
    let tile_scale = material_scales.tiles[u32(layer)].xy;
    var weights = pow(abs(local_normal), vec3<f32>(4.0));
    weights = select(vec3<f32>(0.0), weights, weights >= vec3<f32>(0.002));
    weights = weights / (weights.x + weights.y + weights.z);
    var color = vec4<f32>(0.0);
    var slope = vec3<f32>(0.0);
    var metalness = 0.0;
    for (var axis = 0u; axis < 3u; axis = axis + 1u) {
        if weights[axis] > 0.0 {
            let frame = projection_frame(axis, local_normal);
            let uv = projection_uv(input.material_position, frame) * tile_scale;
            // All derivatives originate at the uniform fragment entry point, before branching.
            let dx = projection_uv(position_dx, frame) * tile_scale;
            let dy = projection_uv(position_dy, frame) * tile_scale;
            var base = textureSampleGrad(material_color, material_sampler, uv, layer, dx, dy);
            var packed_normal = textureSampleGrad(material_normal, material_sampler, uv, layer, dx, dy);
            let tangent_normal = packed_normal.xyz * 2.0 - vec3<f32>(1.0);
            var axis_slope = (frame.tangent * tangent_normal.x + frame.bitangent * tangent_normal.y)
                / max(tangent_normal.z, 0.35);
            // Soil's new three-site plane is composed through the frozen projection below.
            if layer == 0 {
                let plane = scanned_plane(uv, layer, dx, dy);
                base = plane.color;
                axis_slope = frame.tangent * plane.slope.x + frame.bitangent * plane.slope.y;
                packed_normal.w = plane.metallic;
            }
            slope = slope + axis_slope * weights[axis];
            color = color + base * weights[axis];
            metalness = metalness + packed_normal.w * weights[axis];
        }
    }
    var surface: SurfaceSample;
    let macro_variation = value_noise(input.material_position.xz * 0.055
        + vec2<f32>(input.material_position.y * 0.031, 0.0));
    surface.albedo = color.rgb * mix(0.91, 1.06, macro_variation);
    surface.roughness = color.a;
    surface.metallic = metalness;
    surface.local_normal = surface_gradient_normal(local_normal, slope);
    return weather_scanned(surface, input.material, input.material_position, local_normal,
        max(length(position_dx), length(position_dy)));
}
