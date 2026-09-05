struct Globals {
    view_projection: mat4x4<f32>,
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
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) albedo_roughness: vec4<f32>,
    @location(3) ambient_occlusion: f32,
    @location(4) light_clip_position: vec4<f32>,
    @location(5) @interpolate(flat) metallic: f32,
};

struct BodyInstanceInput {
    @location(5) model_0: vec4<f32>,
    @location(6) model_1: vec4<f32>,
    @location(7) model_2: vec4<f32>,
    @location(8) model_3: vec4<f32>,
};

fn body_model(input: BodyInstanceInput) -> mat4x4<f32> {
    return mat4x4<f32>(input.model_0, input.model_1, input.model_2, input.model_3);
}

@vertex
fn world_vertex(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = globals.view_projection * vec4<f32>(input.position, 1.0);
    output.world_position = input.position;
    output.normal = input.normal;
    output.albedo_roughness = input.albedo_roughness;
    output.ambient_occlusion = input.ambient_occlusion;
    output.light_clip_position = globals.light_view_projection * vec4<f32>(input.position, 1.0);
    output.metallic = input.metallic;
    return output;
}

@vertex
fn body_vertex(input: VertexInput, instance: BodyInstanceInput) -> VertexOutput {
    let model = body_model(instance);
    let world_position = model * vec4<f32>(input.position, 1.0);
    var output: VertexOutput;
    output.clip_position = globals.view_projection * world_position;
    output.world_position = world_position.xyz;
    output.normal = normalize((model * vec4<f32>(input.normal, 0.0)).xyz);
    output.albedo_roughness = input.albedo_roughness;
    output.ambient_occlusion = input.ambient_occlusion;
    output.light_clip_position = globals.light_view_projection * world_position;
    output.metallic = input.metallic;
    return output;
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

fn hash(position: vec3<f32>) -> f32 {
    return fract(sin(dot(position, vec3<f32>(12.9898, 78.233, 37.719))) * 43758.5453);
}

fn directional_shadow(light_clip_position: vec4<f32>, normal: vec3<f32>) -> f32 {
    let projected = light_clip_position.xyz / light_clip_position.w;
    let uv = projected.xy * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
    if projected.z <= 0.0 || projected.z >= 1.0 || any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return 1.0;
    }
    let light_direction = normalize(-globals.sun_fog.xyz);
    let bias = max(0.00035 * (1.0 - dot(normal, light_direction)), 0.00008);
    return textureSampleCompare(shadow_map, shadow_sampler, uv, projected.z - bias);
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

@fragment
fn world_fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let normal = normalize(input.normal);
    let light_direction = normalize(-globals.sun_fog.xyz);
    let view_direction = normalize(globals.camera_time.xyz - input.world_position);
    let half_direction = normalize(light_direction + view_direction);
    let roughness_variation = hash(floor(input.world_position * 4.0) + input.normal * 17.0);
    let roughness = clamp(input.albedo_roughness.w + (roughness_variation - 0.5) * 0.08, 0.06, 1.0);
    let metallic = clamp(input.metallic, 0.0, 1.0);
    let ambient_occlusion = clamp(input.ambient_occlusion, 0.35, 1.0);
    let albedo_variation = 0.90 + 0.16 * hash(floor(input.world_position * 2.0));
    let albedo = input.albedo_roughness.rgb * albedo_variation;

    let normal_dot_light = max(dot(normal, light_direction), 0.0);
    let normal_dot_view = max(dot(normal, view_direction), 0.0);
    let normal_dot_half = max(dot(normal, half_direction), 0.0);
    let view_dot_half = max(dot(view_direction, half_direction), 0.0);
    let shadow = directional_shadow(input.light_clip_position, normal);
    let sky_ambient = mix(0.055, 0.19, normal.y * 0.5 + 0.5) * ambient_occlusion;
    let reflectance_at_normal = mix(vec3<f32>(0.04), albedo, metallic);
    let fresnel = fresnel_schlick(view_dot_half, reflectance_at_normal);
    let distribution = distribution_ggx(normal_dot_half, roughness);
    let geometry = geometry_smith(normal_dot_view, normal_dot_light, roughness);
    let specular = distribution * geometry * fresnel
        / max(4.0 * normal_dot_view * normal_dot_light, 0.0001);
    let diffuse_weight = (vec3<f32>(1.0) - fresnel) * (1.0 - metallic);
    let direct_occlusion = mix(0.62, 1.0, ambient_occlusion);
    let sun_radiance = vec3<f32>(3.6, 3.15, 2.58);
    let direct = (diffuse_weight * albedo / 3.14159265 + specular)
        * sun_radiance * normal_dot_light * shadow * direct_occlusion;
    let ambient_fresnel = fresnel_schlick(normal_dot_view, reflectance_at_normal);
    let ambient_diffuse = albedo * sky_ambient * (1.0 - metallic);
    let ambient_specular = ambient_fresnel
        * mix(0.018, 0.105, 1.0 - roughness)
        * ambient_occlusion;
    var color = direct + ambient_diffuse + ambient_specular;

    let distance_from_camera = distance(globals.camera_time.xyz, input.world_position);
    let fog = 1.0 - exp(-distance_from_camera * globals.sun_fog.w);
    let sky = vec3<f32>(0.15, 0.32, 0.57);
    color = mix(color, sky, clamp(fog, 0.0, 0.90));

    // Compact filmic curve. An sRGB surface performs display transfer; linear-only surfaces need it here.
    color = (color * (2.51 * color + vec3<f32>(0.03)))
        / (color * (2.43 * color + vec3<f32>(0.59)) + vec3<f32>(0.14));
    if globals.display.x > 0.5 {
        color = pow(max(color, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));
    }
    return vec4<f32>(clamp(color, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
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
