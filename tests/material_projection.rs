//! Executes the production WGSL helpers, not a CPU reimplementation of the shader.
//! Run explicitly with `cargo test --test material_projection -- --ignored` on a Vulkan GPU.

use destructible_fps::{environment::EnvironmentLibrary, render::create_environment};
use glam::{Mat4, Vec3, Vec4};

const OUTPUT_BYTES: u64 = (43 + 512 * 8 + 128 * 5) * 16;
const WORLD_SHADER: &str = concat!(
    include_str!("../src/shaders/color.wgsl"),
    include_str!("../src/shaders/world.wgsl")
);
const HARNESS: &str = r"
@group(0) @binding(7) var<storage, read_write> results: array<vec4<f32>>;

@compute @workgroup_size(1)
fn validate_material_projection() {
    for (var index = 0u; index < 6u; index = index + 1u) {
        let axis = index / 2u;
        var normal = vec3<f32>(0.0);
        normal[axis] = select(1.0, -1.0, index % 2u == 1u);
        let frame = projection_frame(axis, normal);
        results[index * 3u] = vec4<f32>(cross(frame.tangent, frame.bitangent), 0.0);
        results[index * 3u + 1u] = vec4<f32>(projection_uv(frame.tangent, frame),
            projection_uv(frame.bitangent, frame));
        results[index * 3u + 2u] = vec4<f32>(surface_gradient_normal(normal,
            frame.tangent / 0.75), 0.0);
    }
    let curved = normalize(vec3<f32>(1.0, 2.0, 3.0));
    results[18] = vec4<f32>(surface_gradient_normal(curved, vec3<f32>(0.0)), 0.0);
    results[19] = vec4<f32>(surface_gradient_normal(curved, curved * 2.0), 0.0);
    let model = mat4x4<f32>(vec4<f32>(0.0, 2.0, 0.0, 0.0),
        vec4<f32>(-3.0, 0.0, 0.0, 0.0), vec4<f32>(0.0, 0.0, 4.0, 0.0),
        vec4<f32>(7.0, 11.0, 13.0, 1.0));
    results[20] = vec4<f32>(normalize(normal_transform(model) * curved), 0.0);
    // Deliberately asymmetric off-axis directions catch face flips invisible at face centers.
    let directions = array<vec3<f32>, 6>(vec3<f32>(1.0, -0.25, -0.5), vec3<f32>(-1.0, -0.25, 0.5),
        vec3<f32>(0.5, 1.0, 0.25), vec3<f32>(0.5, -1.0, -0.25),
        vec3<f32>(0.5, -0.25, 1.0), vec3<f32>(-0.5, -0.25, -1.0));
    for (var face = 0u; face < 6u; face = face + 1u) {
        let direction = normalize(directions[face]);
        results[21u + face] = vec4<f32>(atmosphere(direction), 0.0);
        results[27u + face] = vec4<f32>(environment_lighting(direction, direction,
            vec3<f32>(1.0), 0.0, 1.0, 1.0), 0.0);
        results[33u + face] = vec4<f32>(textureSampleLevel(environment_specular, environment_sampler,
            direction, f32(face)).rgb, 0.0);
    }
    results[39] = vec4<f32>(environment_lighting(vec3<f32>(0.0, 1.0, 0.0),
        vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.5), 1.0, 0.0, 0.0), 0.0);
    results[40] = vec4<f32>(environment_lighting(vec3<f32>(0.0, 1.0, 0.0),
        vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.5), 1.0, 1.0, 1.0), 0.0);
    results[41] = vec4<f32>(textureSampleLevel(environment_specular, environment_sampler,
        vec3<f32>(1.0, 0.0, 0.0), f32(textureNumLevels(environment_specular) - 1u)).rgb, 0.0);
    results[42] = vec4<f32>(fog_radiance(vec3<f32>(1.0, 0.0, 0.0)), 0.0);
    for (var index = 0u; index < 128u; index = index + 1u) {
        let material = 4u + index % 2u;
        let p = vec3<f32>(f32(i32(index) - 64) * 16.0, 1.4, f32(index % 9u) * 0.3);
        let n = normalize(vec3<f32>(0.1, 1.0, -0.2));
        let near = sample_cut_core(material, p, n, 0.001);
        let far = sample_cut_core(material, p, n, 1.0);
        let left = sample_cut_core(material, p - vec3<f32>(0.0001, 0.0, 0.0), n, 0.001);
        let right = sample_cut_core(material, p + vec3<f32>(0.0001, 0.0, 0.0), n, 0.001);
        let offset = 4139u + index * 5u;
        results[offset] = vec4<f32>(near.albedo, near.roughness);
        results[offset + 1u] = vec4<f32>(far.albedo, far.roughness);
        results[offset + 2u] = vec4<f32>(near.local_normal - n, near.metallic);
        results[offset + 3u] = vec4<f32>(left.albedo - right.albedo, left.roughness - right.roughness);
        let marker = array<f32, 5>(-2.0, -1.0, 0.0, 0.5, 1.0)[index % 5u];
        results[offset + 4u] = select(vec4<f32>(0.0), vec4<f32>(1.0), vec4<bool>(
            explicit_cut_core(marker, 4u), explicit_cut_core(marker, 5u),
            explicit_cut_core(marker, 1u), explicit_cut_core(marker, 6u)));
    }
    for (var index = 0u; index < 512u; index = index + 1u) {
        let materials = array<u32, 8>(1u, 1u, 4u, 5u, 2u, 3u, 6u, 7u);
        let material = materials[index % 8u];
        let n = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, 0.0, 1.0), index % 8u == 1u
            || material == 4u || material == 5u);
        let p = vec3<f32>(f32(i32(index / 8u) - 32) * 16.0, 2.4, 15.8);
        var source: SurfaceSample;
        source.albedo = vec3<f32>(0.3, 0.2, 0.1);
        source.roughness = 0.7;
        source.metallic = 0.123;
        source.local_normal = normalize(vec3<f32>(0.1, 1.0, 0.2));
        let near = weather_scanned(source, material, p, n, 0.01);
        let left = weather_scanned(source, material, p - vec3<f32>(0.0001, 0.0, 0.0), n, 0.01);
        let right = weather_scanned(source, material, p + vec3<f32>(0.0001, 0.0, 0.0), n, 0.01);
        let far = weather_scanned(source, material, p, n, 10.0);
        let offset = 43u + index * 8u;
        results[offset] = vec4<f32>(near.albedo, near.roughness);
        results[offset + 1u] = vec4<f32>(left.albedo - right.albedo, left.roughness - right.roughness);
        results[offset + 2u] = vec4<f32>(near.local_normal - source.local_normal, near.metallic - source.metallic);
        results[offset + 3u] = vec4<f32>(far.albedo, far.roughness);
        let horizontal = weather_scanned(source, material, p, vec3<f32>(0.0, 1.0, 0.0), 0.01);
        results[offset + 4u] = vec4<f32>(horizontal.albedo, horizontal.roughness);
        for (var axis = 1u; axis < 3u; axis = axis + 1u) {
            var seam = vec3<f32>(2.4, 2.4, 2.4);
            seam[axis] = p.x;
            var epsilon = vec3<f32>(0.0);
            epsilon[axis] = 0.0001;
            let lower = weather_scanned(source, material, seam - epsilon, n, 0.01);
            let upper = weather_scanned(source, material, seam + epsilon, n, 0.01);
            results[offset + 4u + axis] = vec4<f32>(lower.albedo - upper.albedo, lower.roughness - upper.roughness);
        }
        source.albedo = vec3<f32>(1.06);
        let bright = weather_scanned(source, material, p, n, 0.01);
        results[offset + 7u] = vec4<f32>(bright.albedo, bright.roughness);
    }
}
";

async fn gpu() -> (wgpu::Device, wgpu::Queue) {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::VULKAN;
    let instance = wgpu::Instance::new(descriptor);
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        })
        .await
        .expect("explicit GPU validation requires an actual Vulkan adapter");
    let info = adapter.get_info();
    assert_ne!(
        info.device_type,
        wgpu::DeviceType::Cpu,
        "software GPU is not coverage"
    );
    println!("material projection: {} ({:?})", info.name, info.backend);
    adapter
        .request_device(&wgpu::DeviceDescriptor::default())
        .await
        .expect("Vulkan device")
}

fn execute_shader(device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<[f32; 4]> {
    let source = format!("{WORLD_SHADER}\n{HARNESS}");
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production material shader with compute assertions"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let (environment_layout, environment_group) = create_environment(device, queue).unwrap();
    let output_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 7,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let empty_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[],
    });
    let empty = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &empty_layout,
        entries: &[],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[
            Some(&output_layout),
            Some(&empty_layout),
            Some(&empty_layout),
            Some(&environment_layout),
        ],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("material projection validation"),
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("validate_material_projection"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });
    let result = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("material validation results"),
        size: OUTPUT_BYTES,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("material validation readback"),
        size: OUTPUT_BYTES,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("material validation binding"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 7,
            resource: result.as_entire_binding(),
        }],
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bindings, &[]);
        pass.set_bind_group(1, &empty, &[]);
        pass.set_bind_group(2, &empty, &[]);
        pass.set_bind_group(3, &environment_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&result, 0, &readback, 0, OUTPUT_BYTES);
    queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    readback.map_async(wgpu::MapMode::Read, .., move |status| {
        sender.send(status).expect("readback receiver alive");
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(10)),
        })
        .expect("GPU completed");
    receiver
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("readback callback")
        .expect("readback succeeded");
    let values =
        bytemuck::cast_slice(&readback.get_mapped_range(..).expect("mapped range")).to_vec();
    readback.unmap();
    values
}

fn assert_vector(actual: [f32; 4], expected: Vec4) {
    assert!(
        Vec4::from_array(actual).abs_diff_eq(expected, 1e-5),
        "{actual:?} != {expected:?}"
    );
}

#[test]
#[ignore = "requires a real Vulkan GPU; execute explicitly with --ignored"]
fn production_shader_preserves_signed_projection_and_transformed_normals() {
    let (device, queue) = pollster::block_on(gpu());
    let values = execute_shader(&device, &queue);
    validate_environment(&values);
    validate_weathering(&values);
    validate_cut_cores(&values);
    let normals = [
        Vec3::X,
        Vec3::NEG_X,
        Vec3::Y,
        Vec3::NEG_Y,
        Vec3::Z,
        Vec3::NEG_Z,
    ];
    let tangents = [Vec3::NEG_Z, Vec3::Z, Vec3::X, Vec3::X, Vec3::X, Vec3::NEG_X];
    for (index, (normal, tangent)) in normals.into_iter().zip(tangents).enumerate() {
        assert_vector(values[index * 3], normal.extend(0.0));
        assert_vector(values[index * 3 + 1], Vec4::new(1.0, 0.0, 0.0, -1.0));
        assert_vector(
            values[index * 3 + 2],
            (normal + tangent).normalize().extend(0.0),
        );
    }
    let curved = Vec3::new(1.0, 2.0, 3.0).normalize();
    assert_vector(values[18], curved.extend(0.0));
    assert_vector(values[19], curved.extend(0.0));
    let model = Mat4::from_cols(
        Vec4::new(0.0, 2.0, 0.0, 0.0),
        Vec4::new(-3.0, 0.0, 0.0, 0.0),
        Vec4::new(0.0, 0.0, 4.0, 0.0),
        Vec4::new(7.0, 11.0, 13.0, 1.0),
    );
    assert_vector(
        values[20],
        model
            .inverse()
            .transpose()
            .transform_vector3(curved)
            .normalize()
            .extend(0.0),
    );
}

fn validate_cut_cores(values: &[[f32; 4]]) {
    let mut range = (f32::MAX, f32::MIN);
    for index in 0..128 {
        let offset = 4139 + index * 5;
        let near = values[offset];
        let far = values[offset + 1];
        assert!(
            near.iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
        );
        assert!((0.87..=0.97).contains(&near[3]));
        assert_vector(values[offset + 2], Vec4::ZERO);
        assert!(
            values[offset + 3].iter().all(|v| v.abs() < 0.01),
            "core seam {index}"
        );
        let expected = if index % 2 == 0 {
            Vec4::new(0.2325, 0.08875, 0.03975, 0.92)
        } else {
            Vec4::new(0.245, 0.2325, 0.1975, 0.92)
        };
        assert_vector(far, expected);
        let selected = f32::from(index % 5 == 0);
        assert_vector(values[offset + 4], Vec4::new(selected, selected, 0.0, 0.0));
        range.0 = range.0.min(near[0]);
        range.1 = range.1.max(near[0]);
    }
    assert!(range.1 - range.0 > 0.05);
}

fn validate_weathering(values: &[[f32; 4]]) {
    let unchanged = Vec4::new(0.3, 0.2, 0.1, 0.7);
    let mut soil_range = (f32::MAX, f32::MIN);
    let mut wall_range = (f32::MAX, f32::MIN);
    for index in 0..512 {
        let material = [1, 1, 4, 5, 2, 3, 6, 7][index % 8];
        let offset = 43 + index * 8;
        for sample in [values[offset], values[offset + 3]] {
            assert!(sample.iter().all(|v| v.is_finite()));
            for (value, source) in sample[..3].iter().zip([0.3, 0.2, 0.1]) {
                assert!(*value >= source * 0.33 && *value <= source * 1.13);
            }
            assert!((0.48..=0.93).contains(&sample[3]));
        }
        assert!(
            values[offset + 1].iter().all(|v| v.abs() < 0.001),
            "seam {index}"
        );
        assert_vector(values[offset + 2], Vec4::ZERO);
        for delta in [values[offset + 5], values[offset + 6]] {
            assert!(
                delta.iter().all(|v| v.abs() < 0.001),
                "other-axis seam {index}: {delta:?}"
            );
        }
        if matches!(material, 1 | 4 | 5) {
            assert!(
                values[offset + 7]
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            );
        } else {
            assert_vector(values[offset + 7], Vec4::new(1.06, 1.06, 1.06, 0.7));
        }
        match material {
            1 if index % 8 == 0 => {
                soil_range.0 = soil_range.0.min(values[offset][0]);
                soil_range.1 = soil_range.1.max(values[offset][0]);
            }
            1 => assert!(
                (values[offset][3] - 0.82).abs() < 1e-5,
                "vertical soil cannot look damp"
            ),
            4 | 5 => {
                wall_range.0 = wall_range.0.min(values[offset][0]);
                wall_range.1 = wall_range.1.max(values[offset][0]);
                assert_vector(values[offset + 4], Vec4::new(0.312, 0.204, 0.098, 0.7));
            }
            _ => {
                assert_vector(values[offset], unchanged);
                assert_vector(values[offset + 3], unchanged);
            }
        }
    }
    assert!(
        soil_range.1 - soil_range.0 > 0.1,
        "terrain variation disappeared"
    );
    assert!(
        wall_range.1 - wall_range.0 > 0.1,
        "wall variation disappeared"
    );
}

fn half_value(bytes: &[u8]) -> f32 {
    let bits = u16::from_le_bytes([bytes[0], bytes[1]]);
    let exponent = i32::from((bits >> 10) & 31);
    let fraction = f32::from(bits & 1023);
    if exponent == 0 {
        fraction * 2_f32.powi(-24)
    } else {
        (1024.0 + fraction) * 2_f32.powi(exponent - 25)
    }
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
fn face_sample(data: &[u8], edge: usize, face: usize) -> Vec4 {
    // Independent CPU bilinear sample at u=+0.5,v=+0.25 in the selected WebGPU face.
    let x = 0.75_f32.mul_add(edge as f32, -0.5);
    let y = 0.625_f32.mul_add(edge as f32, -0.5);
    let ix = x.floor() as usize;
    let iy = y.floor() as usize;
    let tx = x.fract();
    let ty = y.fract();
    let pixel = |dx: usize, dy: usize| {
        let start = ((face * edge + (iy + dy).min(edge - 1)) * edge + (ix + dx).min(edge - 1)) * 8;
        Vec4::new(
            half_value(&data[start..]),
            half_value(&data[start + 2..]),
            half_value(&data[start + 4..]),
            0.0,
        )
    };
    pixel(0, 0)
        .lerp(pixel(1, 0), tx)
        .lerp(pixel(0, 1).lerp(pixel(1, 1), tx), ty)
}

fn validate_environment(values: &[[f32; 4]]) {
    let library = EnvironmentLibrary::embedded().unwrap();
    for face in 0..6 {
        let actual = Vec4::from_array(values[21 + face]);
        let expected = face_sample(library.sky(), 256, face);
        assert!(
            actual.abs_diff_eq(expected, 0.002),
            "sky face {face}: {actual:?} != {expected:?}"
        );
        let mirror = Vec4::from_array(values[27 + face]);
        let reflected = face_sample(library.specular(), 64, face);
        assert!(
            mirror.abs_diff_eq(reflected, 0.004),
            "mirror face {face}: {mirror:?} != {reflected:?}"
        );
        // Verify every non-final mip's upload ordering, not only the base mirror image.
        let offset: usize = (0..face).map(|mip| 6 * (64_usize >> mip).pow(2) * 8).sum();
        let filtered = face_sample(&library.specular()[offset..], 64 >> face, face);
        let actual = Vec4::from_array(values[33 + face]);
        assert!(
            actual.abs_diff_eq(filtered, 0.002),
            "prefilter mip {face}: {actual:?} != {filtered:?}"
        );
    }
    assert_vector(values[39], Vec4::ZERO);
    assert_vector(values[42], Vec4::from_array(values[41]));
    let final_mip = &library.specular()[library.specular().len() - 48..];
    assert_vector(
        values[41],
        Vec4::new(
            half_value(final_mip),
            half_value(&final_mip[2..]),
            half_value(&final_mip[4..]),
            0.0,
        ),
    );
    assert!(
        values[40]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0 && *v < 6.0)
    );
}
