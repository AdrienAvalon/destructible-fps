struct Globals {
    view_projection: mat4x4<f32>,
    inverse_view_projection: mat4x4<f32>,
    light_view_projection: mat4x4<f32>,
    camera_time: vec4<f32>,
    sun_fog: vec4<f32>,
    display: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> globals: Globals;

@group(1) @binding(0)
var shadow_map: texture_depth_2d;

@group(1) @binding(1)
var shadow_sampler: sampler_comparison;

@group(2) @binding(0)
var material_color: texture_2d_array<f32>;
@group(2) @binding(1)
var material_normal: texture_2d_array<f32>;
@group(2) @binding(2)
var material_sampler: sampler;
struct MaterialScales {
    tiles: array<vec4<f32>, 5>,
};
@group(2) @binding(3)
var<uniform> material_scales: MaterialScales;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) albedo_roughness: vec4<f32>,
    @location(3) ambient_occlusion: f32,
    @location(4) metallic: f32,
    @location(5) material: u32,
    @location(10) damage: f32,
    @location(11) fracture_depth: f32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) albedo_roughness: vec4<f32>,
    @location(3) ambient_occlusion: f32,
    @location(4) light_clip_position: vec4<f32>,
    @location(5) @interpolate(flat) metallic: f32,
    @location(6) @interpolate(flat) material: u32,
    @location(7) material_position: vec3<f32>,
    @location(8) normal_transform_x: vec3<f32>,
    @location(9) normal_transform_y: vec3<f32>,
    @location(10) @interpolate(flat) damage: f32,
    @location(11) fracture_depth: f32,
    @location(12) material_normal: vec3<f32>,
    @location(13) normal_transform_z: vec3<f32>,
};

struct BodyInstanceInput {
    @location(6) model_0: vec4<f32>,
    @location(7) model_1: vec4<f32>,
    @location(8) model_2: vec4<f32>,
    @location(9) model_3: vec4<f32>,
};

struct SurfaceSample {
    albedo: vec3<f32>,
    roughness: f32,
    metallic: f32,
    local_normal: vec3<f32>,
};

struct SkyOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

fn body_model(input: BodyInstanceInput) -> mat4x4<f32> {
    return mat4x4<f32>(input.model_0, input.model_1, input.model_2, input.model_3);
}

fn normal_transform(model: mat4x4<f32>) -> mat3x3<f32> {
    // Cofactors implement inverse-transpose normals for rigid bodies and scaled player instances.
    let normal_x = cross(model[1].xyz, model[2].xyz);
    let normal_y = cross(model[2].xyz, model[0].xyz);
    let normal_z = cross(model[0].xyz, model[1].xyz);
    let determinant = dot(model[0].xyz, normal_x);
    let inverse_determinant = 1.0 / determinant;
    return mat3x3<f32>(normal_x * inverse_determinant, normal_y * inverse_determinant,
        normal_z * inverse_determinant);
}

fn fill_vertex_output(
    input: VertexInput,
    world_position: vec4<f32>,
    model: mat4x4<f32>,
) -> VertexOutput {
    let transform = normal_transform(model);
    var output: VertexOutput;
    output.clip_position = globals.view_projection * world_position;
    output.world_position = world_position.xyz;
    output.normal = normalize(transform * input.normal);
    output.albedo_roughness = input.albedo_roughness;
    output.ambient_occlusion = input.ambient_occlusion;
    output.light_clip_position = globals.light_view_projection * world_position;
    output.metallic = input.metallic;
    output.material = input.material;
    output.material_position = input.position;
    output.material_normal = input.normal;
    output.normal_transform_x = transform[0];
    output.normal_transform_y = transform[1];
    output.normal_transform_z = transform[2];
    output.damage = input.damage;
    output.fracture_depth = input.fracture_depth;
    return output;
}

@vertex
fn world_vertex(input: VertexInput) -> VertexOutput {
    return fill_vertex_output(input, vec4<f32>(input.position, 1.0), mat4x4<f32>(
        vec4<f32>(1.0, 0.0, 0.0, 0.0),
        vec4<f32>(0.0, 1.0, 0.0, 0.0),
        vec4<f32>(0.0, 0.0, 1.0, 0.0),
        vec4<f32>(0.0, 0.0, 0.0, 1.0),
    ));
}

@vertex
fn body_vertex(input: VertexInput, instance: BodyInstanceInput) -> VertexOutput {
    let model = body_model(instance);
    return fill_vertex_output(input, model * vec4<f32>(input.position, 1.0), model);
}

@vertex
fn shadow_vertex(input: VertexInput) -> @builtin(position) vec4<f32> {
    return globals.light_view_projection * vec4<f32>(input.position, 1.0);
}

@vertex
fn body_shadow_vertex(
    input: VertexInput,
    instance: BodyInstanceInput,
) -> @builtin(position) vec4<f32> {
    return globals.light_view_projection * body_model(instance) * vec4<f32>(input.position, 1.0);
}

fn hash_2d(position: vec2<f32>) -> f32 {
    var value = fract(vec3<f32>(position.x, position.y, position.x) * 0.1031);
    value = value + dot(value, value.yzx + vec3<f32>(33.33));
    return fract((value.x + value.y) * value.z);
}

fn value_noise(position: vec2<f32>) -> f32 {
    let cell = floor(position);
    let fraction = fract(position);
    let curve = fraction * fraction * (vec2<f32>(3.0) - 2.0 * fraction);
    let lower = mix(hash_2d(cell), hash_2d(cell + vec2<f32>(1.0, 0.0)), curve.x);
    let upper = mix(
        hash_2d(cell + vec2<f32>(0.0, 1.0)),
        hash_2d(cell + vec2<f32>(1.0, 1.0)),
        curve.x,
    );
    return mix(lower, upper, curve.y);
}

fn fbm(position: vec2<f32>) -> f32 {
    var sample_position = position;
    var amplitude = 0.5;
    var result = 0.0;
    for (var octave: u32 = 0u; octave < 4u; octave = octave + 1u) {
        result = result + value_noise(sample_position) * amplitude;
        sample_position = sample_position * 2.03 + vec2<f32>(17.1, 9.2);
        amplitude = amplitude * 0.5;
    }
    return result / 0.9375;
}

struct ProjectionFrame {
    tangent: vec3<f32>,
    bitangent: vec3<f32>,
};

fn projection_frame(axis: u32, normal: vec3<f32>) -> ProjectionFrame {
    var frame: ProjectionFrame;
    let side = select(-1.0, 1.0, normal[axis] >= 0.0);
    if axis == 0u {
        frame.tangent = vec3<f32>(0.0, 0.0, -side);
        frame.bitangent = vec3<f32>(0.0, 1.0, 0.0);
    } else if axis == 1u {
        frame.tangent = vec3<f32>(1.0, 0.0, 0.0);
        frame.bitangent = vec3<f32>(0.0, 0.0, -side);
    } else {
        frame.tangent = vec3<f32>(side, 0.0, 0.0);
        frame.bitangent = vec3<f32>(0.0, 1.0, 0.0);
    }
    return frame;
}

fn projection_uv(position: vec3<f32>, frame: ProjectionFrame) -> vec2<f32> {
    // Image rows start at the top; a GL normal's positive green component points along +bitangent.
    return vec2<f32>(dot(position, frame.tangent), -dot(position, frame.bitangent));
}

fn dominant_axis(normal: vec3<f32>) -> u32 {
    let magnitude = abs(normal);
    if magnitude.x >= magnitude.y && magnitude.x >= magnitude.z {
        return 0u;
    }
    if magnitude.y >= magnitude.z {
        return 1u;
    }
    return 2u;
}

fn surface_gradient_normal(normal: vec3<f32>, slope: vec3<f32>) -> vec3<f32> {
    // Preserve the original smooth normal for a flat normal map.
    let tangent_slope = slope - normal * dot(slope, normal);
    return normalize(normal + tangent_slope * 0.75);
}

fn sample_scanned(
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
            let base = textureSampleGrad(material_color, material_sampler, uv, layer, dx, dy);
            let packed_normal = textureSampleGrad(material_normal, material_sampler, uv, layer, dx, dy);
            let tangent_normal = packed_normal.xyz * 2.0 - vec3<f32>(1.0);
            let axis_slope = (frame.tangent * tangent_normal.x + frame.bitangent * tangent_normal.y)
                / max(tangent_normal.z, 0.35);
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
    return surface;
}

fn sample_material(
    input: VertexOutput,
    position_dx: vec3<f32>,
    position_dy: vec3<f32>,
) -> SurfaceSample {
    let material = input.material;
    let damage = input.damage;
    let local_normal = normalize(input.material_normal);
    let frame = projection_frame(dominant_axis(local_normal), local_normal);
    let uv = projection_uv(input.material_position, frame);
    let footprint = max(length(position_dx), length(position_dy));
    let detail_visibility = 1.0 - smoothstep(0.008, 0.085, footprint);
    var surface: SurfaceSample;
    if material >= 1u && material <= 5u {
        surface = sample_scanned(input, position_dx, position_dy);
    } else {
        surface.albedo = input.albedo_roughness.rgb;
        surface.roughness = input.albedo_roughness.w;
        surface.metallic = input.metallic;
        surface.local_normal = local_normal;
        if material == 6u {
            let rust = smoothstep(0.58, 0.84, fbm(uv * 1.9 + vec2<f32>(4.0, -7.0)));
            surface.albedo = mix(surface.albedo * 0.62, vec3<f32>(0.18, 0.055, 0.018), rust);
            surface.roughness = mix(0.38, 0.88, rust);
            surface.metallic = mix(surface.metallic, 0.05, rust);
        } else if material == 7u {
            let grime = smoothstep(0.60, 0.88, fbm(uv * vec2<f32>(0.7, 3.1)));
            surface.albedo = mix(vec3<f32>(0.028, 0.044, 0.049), vec3<f32>(0.09, 0.10, 0.085), grime);
            surface.roughness = 0.12 + grime * 0.35;
        }
    }

    if (material == 4u || material == 5u) && damage > 0.0 {
        let fracture_noise = fbm(uv * 3.1 + vec2<f32>(7.3, -12.8));
        let aggregate_noise = value_noise(uv * 31.0 + vec2<f32>(-2.7, 9.4));
        let crack_distance = abs(value_noise(uv * 5.7 + fracture_noise * 1.9) - 0.5);
        let cracks = (1.0 - smoothstep(0.018, 0.072, crack_distance)) * detail_visibility;
        let exposed = clamp(damage * (0.38 + fracture_noise * 0.72), 0.0, 0.92);
        var fractured_albedo = mix(vec3<f32>(0.08, 0.045, 0.022), vec3<f32>(0.26, 0.13, 0.07),
            smoothstep(0.54, 0.86, aggregate_noise));
        if material == 5u {
            fractured_albedo = mix(vec3<f32>(0.12, 0.13, 0.13), vec3<f32>(0.35, 0.32, 0.26),
                smoothstep(0.57, 0.83, aggregate_noise));
        }
        surface.albedo = mix(surface.albedo, fractured_albedo, exposed * 0.78);
        surface.albedo = surface.albedo * mix(1.0, 0.24, cracks * clamp(damage * 2.4, 0.0, 1.0));
        surface.roughness = mix(surface.roughness, 0.98, clamp(exposed + cracks * 0.35, 0.0, 1.0));
    }

    if input.fracture_depth >= 0.0 && (material == 4u || material == 5u) {
        let cut_damage = max(damage, 0.32);
        let depth = clamp(input.fracture_depth, 0.0, 1.0);
        let shell = smoothstep(0.085, 0.19, abs(depth - 0.5));
        let coarse = value_noise(uv * 11.0 + vec2<f32>(13.7, -6.4));
        let chips = smoothstep(0.61, 0.87, coarse);
        let layer_noise = fbm(uv * 3.1 + vec2<f32>(7.3, -12.8));
        var core_albedo = mix(vec3<f32>(0.12, 0.095, 0.065), vec3<f32>(0.27, 0.095, 0.04), chips * 0.62);
        if material == 5u {
            core_albedo = mix(vec3<f32>(0.10, 0.11, 0.11), vec3<f32>(0.31, 0.27, 0.20), chips * 0.74);
        }
        let ragged_shell = clamp(shell + (layer_noise - 0.5) * mix(0.16, 0.42, cut_damage), 0.0, 1.0);
        surface.albedo = mix(core_albedo, surface.albedo, ragged_shell);
        surface.roughness = mix(0.98, surface.roughness, ragged_shell);
        surface.local_normal = normalize(mix(local_normal, surface.local_normal, ragged_shell));
        if material == 5u {
            let bar_distance = abs(fract(uv.y * 0.42 + 0.5) - 0.5);
            let rebar = (1.0 - smoothstep(0.018, 0.047, bar_distance))
                * (1.0 - shell) * clamp(cut_damage * 2.2, 0.0, 1.0);
            surface.albedo = mix(surface.albedo, vec3<f32>(0.08, 0.02, 0.007), rebar * 0.92);
            surface.roughness = mix(surface.roughness, 0.62, rebar);
            surface.metallic = mix(surface.metallic, 0.48, rebar);
        }
    }
    return surface;
}

fn directional_shadow(light_clip_position: vec4<f32>, normal: vec3<f32>) -> f32 {
    let projected = light_clip_position.xyz / light_clip_position.w;
    let uv = projected.xy * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
    if projected.z <= 0.0 || projected.z >= 1.0 || any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return 1.0;
    }
    let light_direction = normalize(-globals.sun_fog.xyz);
    let bias = max(0.00048 * (1.0 - dot(normal, light_direction)), 0.00010);
    let texel = vec2<f32>(1.0) / vec2<f32>(textureDimensions(shadow_map));
    var visibility = 0.0;
    for (var y: i32 = -1; y <= 1; y = y + 1) {
        for (var x: i32 = -1; x <= 1; x = x + 1) {
            visibility = visibility + textureSampleCompare(
                shadow_map,
                shadow_sampler,
                uv + vec2<f32>(f32(x), f32(y)) * texel * 1.35,
                projected.z - bias,
            );
        }
    }
    return visibility / 9.0;
}

fn distribution_ggx(normal_dot_half: f32, roughness: f32) -> f32 {
    let alpha = roughness * roughness;
    let alpha_squared = alpha * alpha;
    let denominator_term = normal_dot_half * normal_dot_half * (alpha_squared - 1.0) + 1.0;
    return alpha_squared / max(3.14159265 * denominator_term * denominator_term, 0.000001);
}

fn geometry_schlick_ggx(normal_dot_direction: f32, roughness: f32) -> f32 {
    let remapped = roughness + 1.0;
    let k = remapped * remapped * 0.125;
    return normal_dot_direction / max(normal_dot_direction * (1.0 - k) + k, 0.000001);
}

fn geometry_smith(normal_dot_view: f32, normal_dot_light: f32, roughness: f32) -> f32 {
    return geometry_schlick_ggx(normal_dot_view, roughness)
        * geometry_schlick_ggx(normal_dot_light, roughness);
}

fn fresnel_schlick(cosine: f32, reflectance_at_normal: vec3<f32>) -> vec3<f32> {
    let grazing = pow(1.0 - clamp(cosine, 0.0, 1.0), 5.0);
    return reflectance_at_normal + (vec3<f32>(1.0) - reflectance_at_normal) * grazing;
}

fn atmosphere(direction: vec3<f32>) -> vec3<f32> {
    let ray = normalize(direction);
    let sun_direction = normalize(-globals.sun_fog.xyz);
    let elevation = clamp(ray.y * 0.5 + 0.5, 0.0, 1.0);
    let zenith = vec3<f32>(0.105, 0.215, 0.39);
    let horizon = vec3<f32>(0.51, 0.58, 0.64);
    var color = mix(horizon, zenith, pow(elevation, 0.62));
    let sun_alignment = max(dot(ray, sun_direction), 0.0);
    color = color + vec3<f32>(1.0, 0.77, 0.51) * pow(sun_alignment, 480.0) * 7.0;
    color = color + vec3<f32>(0.52, 0.38, 0.24) * pow(sun_alignment, 18.0) * 0.20;

    if ray.y > -0.08 {
        let cloud_projection = ray.xz / max(ray.y + 0.24, 0.16);
        let drift = vec2<f32>(globals.camera_time.w * 0.0025, globals.camera_time.w * 0.0011);
        let cloud_field = fbm(cloud_projection * 0.42 + drift);
        let cloud_mask = smoothstep(0.43, 0.64, cloud_field) * smoothstep(-0.04, 0.18, ray.y);
        let lit_cloud = mix(vec3<f32>(0.34, 0.37, 0.40), vec3<f32>(0.88, 0.86, 0.80), pow(sun_alignment, 2.0));
        color = mix(color, lit_cloud, cloud_mask * 0.72);
    }
    return color;
}

fn display_transform(linear_color: vec3<f32>) -> vec3<f32> {
    var color = max(linear_color, vec3<f32>(0.0));
    color = (color * (2.51 * color + vec3<f32>(0.03)))
        / (color * (2.43 * color + vec3<f32>(0.59)) + vec3<f32>(0.14));
    if globals.display.x > 0.5 {
        color = pow(max(color, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));
    }
    return clamp(color, vec3<f32>(0.0), vec3<f32>(1.0));
}

@vertex
fn sky_vertex(@builtin(vertex_index) index: u32) -> SkyOutput {
    let positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var output: SkyOutput;
    output.ndc = positions[index];
    output.clip_position = vec4<f32>(output.ndc, 1.0, 1.0);
    return output;
}

@fragment
fn sky_fragment(input: SkyOutput) -> @location(0) vec4<f32> {
    let far_point = globals.inverse_view_projection * vec4<f32>(input.ndc, 1.0, 1.0);
    let world_position = far_point.xyz / far_point.w;
    let view_direction = normalize(world_position - globals.camera_time.xyz);
    return vec4<f32>(display_transform(atmosphere(view_direction)), 1.0);
}

@fragment
fn world_fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let position_dx = dpdx(input.material_position);
    let position_dy = dpdy(input.material_position);
    let sampled = sample_material(input, position_dx, position_dy);
    let normal = normalize(input.normal_transform_x * sampled.local_normal.x
        + input.normal_transform_y * sampled.local_normal.y
        + input.normal_transform_z * sampled.local_normal.z);
    let light_direction = normalize(-globals.sun_fog.xyz);
    let view_direction = normalize(globals.camera_time.xyz - input.world_position);
    let half_direction = normalize(light_direction + view_direction);
    let roughness = clamp(sampled.roughness, 0.055, 1.0);
    let metallic = clamp(sampled.metallic, 0.0, 1.0);
    let ambient_occlusion = clamp(input.ambient_occlusion, 0.32, 1.0);
    let albedo = max(sampled.albedo, vec3<f32>(0.003));

    let normal_dot_light = max(dot(normal, light_direction), 0.0);
    let normal_dot_view = max(dot(normal, view_direction), 0.0);
    let normal_dot_half = max(dot(normal, half_direction), 0.0);
    let view_dot_half = max(dot(view_direction, half_direction), 0.0);
    let shadow = directional_shadow(input.light_clip_position, normal);
    let reflectance_at_normal = mix(vec3<f32>(0.04), albedo, metallic);
    let fresnel = fresnel_schlick(view_dot_half, reflectance_at_normal);
    let distribution = distribution_ggx(normal_dot_half, roughness);
    let geometry = geometry_smith(normal_dot_view, normal_dot_light, roughness);
    let specular = distribution * geometry * fresnel
        / max(4.0 * normal_dot_view * normal_dot_light, 0.0001);
    let diffuse_weight = (vec3<f32>(1.0) - fresnel) * (1.0 - metallic);
    let direct_occlusion = mix(0.58, 1.0, ambient_occlusion);
    let sun_radiance = vec3<f32>(3.75, 3.28, 2.66);
    let direct = (diffuse_weight * albedo / 3.14159265 + specular)
        * sun_radiance * normal_dot_light * shadow * direct_occlusion;

    let sky_factor = normal.y * 0.5 + 0.5;
    let sky_irradiance = mix(vec3<f32>(0.045, 0.040, 0.035), vec3<f32>(0.22, 0.28, 0.36), sky_factor);
    let ambient_fresnel = fresnel_schlick(normal_dot_view, reflectance_at_normal);
    let ambient_diffuse = albedo * sky_irradiance * ambient_occlusion * (1.0 - metallic);
    let ambient_specular = ambient_fresnel
        * mix(vec3<f32>(0.025, 0.028, 0.032), vec3<f32>(0.15, 0.18, 0.22), 1.0 - roughness)
        * ambient_occlusion;
    var color = direct + ambient_diffuse + ambient_specular;

    let camera_to_surface = input.world_position - globals.camera_time.xyz;
    let distance_from_camera = length(camera_to_surface);
    let altitude_relief = clamp((input.world_position.y + 4.0) / 42.0, 0.0, 1.0);
    let fog_density = globals.sun_fog.w * mix(1.18, 0.48, altitude_relief);
    let fog = 1.0 - exp(-distance_from_camera * fog_density);
    color = mix(color, atmosphere(normalize(camera_to_surface)), clamp(fog, 0.0, 0.92));

    return vec4<f32>(display_transform(color), 1.0);
}

struct CrosshairOutput {
    @builtin(position) clip_position: vec4<f32>,
};

@vertex
fn crosshair_vertex(@builtin(vertex_index) index: u32) -> CrosshairOutput {
    let points = array<vec2<f32>, 12>(
        vec2<f32>(-0.0012, -0.018), vec2<f32>(0.0012, -0.018), vec2<f32>(0.0012, 0.018),
        vec2<f32>(-0.0012, -0.018), vec2<f32>(0.0012, 0.018), vec2<f32>(-0.0012, 0.018),
        vec2<f32>(-0.010, -0.0021), vec2<f32>(0.010, -0.0021), vec2<f32>(0.010, 0.0021),
        vec2<f32>(-0.010, -0.0021), vec2<f32>(0.010, 0.0021), vec2<f32>(-0.010, 0.0021),
    );
    var output: CrosshairOutput;
    output.clip_position = vec4<f32>(points[index], 0.0, 1.0);
    return output;
}

@fragment
fn crosshair_fragment() -> @location(0) vec4<f32> {
    return vec4<f32>(0.96, 0.98, 1.0, 0.92);
}
