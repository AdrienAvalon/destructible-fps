struct LightMatrix { view_projection: mat4x4<f32> };
@group(0) @binding(0) var<uniform> light: LightMatrix;

struct Position { @location(0) position: vec3<f32> };
struct Instance {
    @location(6) x: vec4<f32>,
    @location(7) y: vec4<f32>,
    @location(8) z: vec4<f32>,
    @location(9) w: vec4<f32>,
};

@vertex
fn static_depth(input: Position) -> @builtin(position) vec4<f32> {
    return light.view_projection * vec4<f32>(input.position, 1.0);
}

@vertex
fn instance_depth(input: Position, instance: Instance) -> @builtin(position) vec4<f32> {
    let model = mat4x4<f32>(instance.x, instance.y, instance.z, instance.w);
    return light.view_projection * model * vec4<f32>(input.position, 1.0);
}
