struct Globals {
    view_projection: mat4x4<f32>,
    camera_time: vec4<f32>,
    sun_fog: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> globals: Globals;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) albedo_roughness: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) albedo_roughness: vec4<f32>,
};

@vertex
fn world_vertex(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = globals.view_projection * vec4<f32>(input.position, 1.0);
    output.world_position = input.position;
    output.normal = input.normal;
    output.albedo_roughness = input.albedo_roughness;
    return output;
}

fn hash(position: vec3<f32>) -> f32 {
    return fract(sin(dot(position, vec3<f32>(12.9898, 78.233, 37.719))) * 43758.5453);
}

@fragment
fn world_fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let normal = normalize(input.normal);
    let light_direction = normalize(-globals.sun_fog.xyz);
    let view_direction = normalize(globals.camera_time.xyz - input.world_position);
    let half_direction = normalize(light_direction + view_direction);
    let roughness = input.albedo_roughness.w;
    let albedo_variation = 0.90 + 0.16 * hash(floor(input.world_position * 2.0));
    let albedo = input.albedo_roughness.rgb * albedo_variation;

    let diffuse = max(dot(normal, light_direction), 0.0);
    let sky_ambient = mix(0.10, 0.30, normal.y * 0.5 + 0.5);
    let specular_power = mix(96.0, 5.0, roughness);
    let specular = pow(max(dot(normal, half_direction), 0.0), specular_power)
        * mix(0.38, 0.025, roughness);
    let rim = pow(1.0 - max(dot(normal, view_direction), 0.0), 3.0) * 0.055;
    var color = albedo * (sky_ambient + diffuse * vec3<f32>(1.08, 1.00, 0.88))
        + vec3<f32>(specular + rim);

    let distance_from_camera = distance(globals.camera_time.xyz, input.world_position);
    let fog = 1.0 - exp(-distance_from_camera * globals.sun_fog.w);
    let sky = vec3<f32>(0.39, 0.56, 0.76);
    color = mix(color, sky, clamp(fog, 0.0, 0.90));

    // Compact filmic curve and display transfer.
    color = (color * (2.51 * color + vec3<f32>(0.03)))
        / (color * (2.43 * color + vec3<f32>(0.59)) + vec3<f32>(0.14));
    color = pow(clamp(color, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(1.0 / 2.2));
    return vec4<f32>(color, 1.0);
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
