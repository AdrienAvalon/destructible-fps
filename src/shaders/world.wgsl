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
    @location(7) material_uv: vec2<f32>,
    @location(8) world_tangent: vec3<f32>,
    @location(9) world_bitangent: vec3<f32>,
    @location(10) @interpolate(flat) damage: f32,
    @location(11) fracture_depth: f32,
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
    height: f32,
};

struct SkyOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

fn body_model(input: BodyInstanceInput) -> mat4x4<f32> {
    return mat4x4<f32>(input.model_0, input.model_1, input.model_2, input.model_3);
}

fn face_tangent(normal: vec3<f32>) -> vec3<f32> {
    if normal.z < -0.999999 {
        return vec3<f32>(0.0, -1.0, 0.0);
    }
    let inverse = 1.0 / (1.0 + normal.z);
    return vec3<f32>(
        1.0 - normal.x * normal.x * inverse,
        -normal.x * normal.y * inverse,
        -normal.x,
    );
}

fn fill_vertex_output(
    input: VertexInput,
    world_position: vec4<f32>,
    model: mat4x4<f32>,
) -> VertexOutput {
    let tangent = face_tangent(input.normal);
    let bitangent = normalize(cross(input.normal, tangent));
    var output: VertexOutput;
    output.clip_position = globals.view_projection * world_position;
    output.world_position = world_position.xyz;
    output.normal = normalize((model * vec4<f32>(input.normal, 0.0)).xyz);
    output.albedo_roughness = input.albedo_roughness;
    output.ambient_occlusion = input.ambient_occlusion;
    output.light_clip_position = globals.light_view_projection * world_position;
    output.metallic = input.metallic;
    output.material = input.material;
    output.material_uv = vec2<f32>(dot(input.position, tangent), dot(input.position, bitangent));
    output.world_tangent = normalize((model * vec4<f32>(tangent, 0.0)).xyz);
    output.world_bitangent = normalize((model * vec4<f32>(bitangent, 0.0)).xyz);
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

fn brick_interior(uv: vec2<f32>) -> f32 {
    var brick = uv * vec2<f32>(1.75, 3.5);
    let row_offset = step(0.5, fract(floor(brick.y) * 0.5)) * 0.5;
    brick.x = brick.x + row_offset;
    let cell = fract(brick);
    let edge = min(min(cell.x, 1.0 - cell.x), min(cell.y, 1.0 - cell.y));
    return smoothstep(0.035, 0.085, edge);
}

fn material_height(material: u32, uv: vec2<f32>, damage: f32) -> f32 {
    if material == 1u {
        return fbm(uv * 5.0) * 0.75 + value_noise(uv * 31.0) * 0.25;
    }
    if material == 2u {
        return fbm(uv * 3.2) * 0.8 + sin((uv.x + uv.y) * 18.0) * 0.05;
    }
    if material == 3u {
        let grain = sin((uv.x + fbm(uv * 0.85) * 0.18) * 38.0) * 0.5 + 0.5;
        return grain * 0.55 + fbm(uv * 7.0) * 0.45;
    }
    if material == 4u {
        let intact = brick_interior(uv) * 0.78 + fbm(uv * 8.0) * 0.22;
        let fracture = fbm(uv * 4.7 + vec2<f32>(11.3, -4.9)) * 0.67
            + value_noise(uv * 29.0) * 0.33;
        return mix(intact, fracture, clamp(damage * 1.35, 0.0, 0.88));
    }
    if material == 5u {
        let intact = fbm(uv * 6.0) * 0.7 + value_noise(uv * 43.0) * 0.3;
        let aggregate = fbm(uv * 3.8 + vec2<f32>(-3.7, 8.1)) * 0.58
            + value_noise(uv * 37.0) * 0.42;
        return mix(intact, aggregate, clamp(damage * 1.30, 0.0, 0.84));
    }
    if material == 6u {
        let brushed = sin(uv.y * 145.0 + value_noise(uv * 3.0) * 5.0) * 0.5 + 0.5;
        return brushed * 0.25 + fbm(uv * 9.0) * 0.75;
    }
    if material == 7u {
        return value_noise(uv * 18.0);
    }
    return 0.5;
}

fn sample_material(
    material: u32,
    uv: vec2<f32>,
    base_albedo: vec3<f32>,
    base_roughness: f32,
    base_metallic: f32,
    damage: f32,
    fracture_depth: f32,
) -> SurfaceSample {
    let broad = fbm(uv * 0.8);
    let footprint = max(length(dpdx(uv)), length(dpdy(uv)));
    let detail_visibility = 1.0 - smoothstep(0.008, 0.085, footprint);
    let fine = mix(0.5, value_noise(uv * 18.0), detail_visibility);
    var surface: SurfaceSample;
    surface.albedo = base_albedo * (0.80 + broad * 0.28 + fine * 0.035);
    surface.roughness = base_roughness;
    surface.metallic = base_metallic;
    surface.height = material_height(material, uv, damage);

    if material == 1u {
        let damp = smoothstep(0.58, 0.92, fbm(uv * 1.7));
        let grit = smoothstep(0.84, 0.97, mix(0.5, value_noise(uv * 24.0), detail_visibility));
        surface.albedo = mix(surface.albedo * vec3<f32>(0.58, 0.50, 0.42), vec3<f32>(0.20, 0.18, 0.15), grit * 0.45);
        surface.albedo = mix(surface.albedo, surface.albedo * 0.48, damp * 0.42);
        surface.roughness = mix(0.98, 0.76, damp);
    } else if material == 2u {
        let strata = sin((uv.y + fbm(uv * 0.7) * 0.32) * 13.0) * 0.5 + 0.5;
        surface.albedo = surface.albedo * mix(vec3<f32>(0.62, 0.66, 0.70), vec3<f32>(1.08, 1.02, 0.92), strata);
        surface.roughness = 0.72 + fine * 0.20;
    } else if material == 3u {
        let grain = sin((uv.x + fbm(uv * 0.8) * 0.18) * 34.0) * 0.5 + 0.5;
        let knot = smoothstep(0.72, 0.94, fbm(uv * 2.6));
        surface.albedo = surface.albedo * mix(vec3<f32>(0.48, 0.36, 0.25), vec3<f32>(1.22, 0.90, 0.54), grain);
        surface.albedo = mix(surface.albedo, surface.albedo * 0.36, knot * 0.52);
        surface.roughness = 0.58 + fine * 0.20;
    } else if material == 4u {
        let interior = brick_interior(uv);
        let brick = base_albedo * (0.68 + broad * 0.32 + fine * 0.11);
        let mortar = vec3<f32>(0.27, 0.25, 0.22) * (0.72 + broad * 0.18);
        let soot = smoothstep(0.72, 0.96, fbm(uv * vec2<f32>(0.55, 2.4)));
        surface.albedo = mix(mortar, brick, interior) * mix(1.0, 0.48, soot * 0.38);
        surface.roughness = mix(0.97, 0.79 + fine * 0.12, interior);
    } else if material == 5u {
        let pores = smoothstep(0.82, 0.97, value_noise(uv * 47.0));
        let stain = smoothstep(0.62, 0.94, fbm(uv * vec2<f32>(0.45, 2.6)));
        surface.albedo = surface.albedo * mix(vec3<f32>(0.74, 0.76, 0.77), vec3<f32>(1.08, 1.04, 0.97), broad);
        surface.albedo = mix(
            surface.albedo,
            surface.albedo * 0.50,
            pores * 0.22 * detail_visibility + stain * 0.16,
        );
        surface.roughness = 0.76 + fine * 0.18;
    } else if material == 6u {
        let rust = smoothstep(0.67, 0.91, fbm(uv * 1.9 + vec2<f32>(4.0, -7.0)));
        let brushed = sin(uv.y * 122.0 + broad * 4.0) * 0.5 + 0.5;
        let steel = base_albedo * (0.72 + brushed * 0.23);
        surface.albedo = mix(steel, vec3<f32>(0.31, 0.105, 0.035), rust * 0.78);
        surface.roughness = mix(0.26 + fine * 0.11, 0.72, rust);
        surface.metallic = mix(base_metallic, 0.18, rust);
    } else if material == 7u {
        let grime = smoothstep(0.68, 0.94, fbm(uv * vec2<f32>(0.7, 3.1)));
        surface.albedo = mix(base_albedo * vec3<f32>(0.62, 0.83, 0.92), vec3<f32>(0.07, 0.085, 0.08), grime * 0.36);
        surface.roughness = 0.08 + fine * 0.10 + grime * 0.22;
    }

    if (material == 4u || material == 5u) && damage > 0.0 {
        let fracture_noise = fbm(uv * 3.1 + vec2<f32>(7.3, -12.8));
        let aggregate_noise = value_noise(uv * 31.0 + vec2<f32>(-2.7, 9.4));
        let crack_distance = abs(value_noise(uv * 5.7 + fracture_noise * 1.9) - 0.5);
        let cracks = (1.0 - smoothstep(0.018, 0.072, crack_distance)) * detail_visibility;
        let exposed = clamp(damage * (0.38 + fracture_noise * 0.72), 0.0, 0.92);
        var fractured_albedo = vec3<f32>(0.30, 0.27, 0.23);
        if material == 5u {
            fractured_albedo = mix(
                vec3<f32>(0.31, 0.32, 0.32),
                vec3<f32>(0.52, 0.49, 0.43),
                smoothstep(0.57, 0.83, aggregate_noise),
            );
        } else {
            fractured_albedo = mix(
                vec3<f32>(0.27, 0.16, 0.10),
                vec3<f32>(0.47, 0.33, 0.24),
                smoothstep(0.54, 0.86, aggregate_noise),
            );
        }
        surface.albedo = mix(surface.albedo, fractured_albedo, exposed * 0.78);
        surface.albedo = surface.albedo * mix(1.0, 0.24, cracks * clamp(damage * 2.4, 0.0, 1.0));
        surface.roughness = max(surface.roughness, mix(surface.roughness, 0.98, exposed + cracks * 0.35));
    }

    if fracture_depth >= 0.0 && (material == 4u || material == 5u) {
        let cut_damage = max(damage, 0.32);
        let depth = clamp(fracture_depth, 0.0, 1.0);
        let distance_from_core = abs(depth - 0.5);
        let shell = smoothstep(0.085, 0.19, distance_from_core);
        let coarse = value_noise(uv * 11.0 + vec2<f32>(13.7, -6.4));
        let chips = smoothstep(0.61, 0.87, coarse);
        let layer_noise = fbm(uv * 3.1 + vec2<f32>(7.3, -12.8));
        var core_albedo = mix(
            vec3<f32>(0.31, 0.28, 0.24),
            vec3<f32>(0.46, 0.25, 0.14),
            chips * 0.62,
        );
        if material == 5u {
            core_albedo = mix(
                vec3<f32>(0.29, 0.30, 0.30),
                vec3<f32>(0.53, 0.49, 0.41),
                chips * 0.74,
            );
        }
        let ragged_shell = clamp(
            shell + (layer_noise - 0.5) * mix(0.16, 0.42, cut_damage),
            0.0,
            1.0,
        );
        surface.albedo = mix(core_albedo, surface.albedo, ragged_shell);
        surface.roughness = mix(0.98, surface.roughness, ragged_shell);

        if material == 5u {
            let bar_distance = abs(fract(uv.y * 0.42 + 0.5) - 0.5);
            let rebar = (1.0 - smoothstep(0.018, 0.047, bar_distance))
                * (1.0 - shell)
                * clamp(cut_damage * 2.2, 0.0, 1.0);
            surface.albedo = mix(surface.albedo, vec3<f32>(0.25, 0.075, 0.022), rebar * 0.92);
            surface.roughness = mix(surface.roughness, 0.62, rebar);
            surface.metallic = mix(surface.metallic, 0.48, rebar);
        }
    }
    return surface;
}

fn material_bump_strength(material: u32) -> f32 {
    if material == 1u {
        return 0.10;
    }
    if material == 2u {
        return 0.14;
    }
    if material == 3u {
        return 0.09;
    }
    if material == 4u {
        return 0.13;
    }
    if material == 5u {
        return 0.10;
    }
    if material == 6u {
        return 0.045;
    }
    return 0.015;
}

fn perturb_normal(input: VertexOutput, center_height: f32) -> vec3<f32> {
    let epsilon = 0.018;
    let tangent_height = material_height(
        input.material,
        input.material_uv + vec2<f32>(epsilon, 0.0),
        input.damage,
    );
    let bitangent_height = material_height(
        input.material,
        input.material_uv + vec2<f32>(0.0, epsilon),
        input.damage,
    );
    let footprint = max(length(dpdx(input.material_uv)), length(dpdy(input.material_uv)));
    let detail_visibility = 1.0 - smoothstep(0.006, 0.075, footprint);
    let strength = material_bump_strength(input.material) * detail_visibility;
    let gradient = vec2<f32>(tangent_height - center_height, bitangent_height - center_height) / epsilon;
    return normalize(
        input.normal
        - input.world_tangent * gradient.x * strength
        - input.world_bitangent * gradient.y * strength
    );
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
    let sampled = sample_material(
        input.material,
        input.material_uv,
        input.albedo_roughness.rgb,
        input.albedo_roughness.w,
        input.metallic,
        input.damage,
        input.fracture_depth,
    );
    let normal = perturb_normal(input, sampled.height);
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
