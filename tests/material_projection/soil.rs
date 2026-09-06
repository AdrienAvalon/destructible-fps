//! Actual packed scan sampling with production explicit gradients, plus lattice edge probes.
use destructible_fps::material_library::{
    MATERIAL_TEXTURE_EDGE, MATERIAL_TEXTURE_LAYERS, MATERIAL_TEXTURE_MIPS, MaterialLibrary,
};
use wgpu::util::DeviceExt;

pub const SAMPLES: u64 = 1024;
pub const STRIDE: u64 = 12;
const OFFSET: usize = 4779;
pub const REFERENCE: &str = include_str!("soil_reference.wgsl");

pub const HARNESS: &str = r"
fn validate_soil_projection() {
    let normals = array<vec3<f32>, 8>(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(-1.0, 0.0, 0.0),
        vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(0.0, -1.0, 0.0), vec3<f32>(0.0, 0.0, 1.0),
        vec3<f32>(0.0, 0.0, -1.0), normalize(vec3<f32>(1.0, 2.0, -3.0)),
        normalize(vec3<f32>(-2.0, 3.0, 1.0)));
    for (var i = 0u; i < 40u; i = i + 1u) {
        var input: VertexOutput;
        input.material = i / 8u + 1u;
        input.material_position = vec3<f32>(-4.137, 2.351, 7.753);
        input.material_normal = normals[i % 8u];
        let dx = vec3<f32>(0.003, 0.001, 0.002);
        let dy = vec3<f32>(-0.001, 0.002, 0.003);
        let actual = sample_scanned(input, dx, dy);
        let expected = reference_scanned(input, dx, dy);
        let offset = 17067u + i * 4u;
        results[offset] = vec4<f32>(actual.albedo - expected.albedo, actual.roughness - expected.roughness);
        results[offset + 1u] = vec4<f32>(actual.local_normal - expected.local_normal, actual.metallic - expected.metallic);
        results[offset + 2u] = vec4<f32>(actual.albedo, actual.roughness);
        results[offset + 3u] = vec4<f32>(actual.local_normal, actual.metallic);
    }
}

fn validate_soil() {
    for (var i = 0u; i < 1024u; i = i + 1u) {
        let cell = vec2<f32>(f32(i32(i % 32u) - 16), f32(i32(i / 32u) - 16));
        var grid = cell + vec2<f32>(0.37, 0.63); // Shared diagonal.
        if i % 3u == 1u { grid = cell + vec2<f32>(0.0, 0.37); }
        if i % 3u == 2u { grid = cell + vec2<f32>(0.37, 0.0); }
        let uv = vec2<f32>(grid.x + grid.y * 0.5, grid.y * 0.866025404);
        let dx = vec2<f32>(0.003, 0.0);
        let dy = vec2<f32>(0.0, 0.003);
        let epsilon = vec2<f32>(0.00001, 0.00001);
        let lower = scanned_plane(uv - epsilon, 0, dx, dy);
        let upper = scanned_plane(uv + epsilon, 0, dx, dy);
        // Contrast probes are independent of the repeated edge probes above.
        let p = vec2<f32>(weather_hash(vec2<i32>(i32(i), 113)),
            weather_hash(vec2<i32>(i32(i), -91))) * 128.0 - vec2<f32>(64.0);
        let plain = scan_plane(p, 0, dx, dy);
        let random = scanned_plane(p, 0, dx, dy);
        let plain_shift = scan_plane(p + vec2<f32>(1.0, 0.0), 0, dx, dy);
        let random_shift = scanned_plane(p + vec2<f32>(1.0, 0.0), 0, dx, dy);
        let other = i32(i % 4u) + 1;
        let untouched = scanned_plane(p, other, dx, dy);
        let reference = scan_plane(p, other, dx, dy);
        let far = scanned_plane(p, 0, vec2<f32>(2.0, 0.0), vec2<f32>(0.0, 2.0));
        let far_reference = scan_plane(p, 0, vec2<f32>(2.0, 0.0), vec2<f32>(0.0, 2.0));
        let blend = soil_blend(uv);
        let offset = 4779u + i * 12u;
        results[offset] = vec4<f32>(blend.weights, random.metallic);
        results[offset + 1u] = lower.color - upper.color;
        results[offset + 2u] = vec4<f32>(lower.slope - upper.slope, lower.metallic - upper.metallic, 0.0);
        results[offset + 3u] = plain.color - plain_shift.color;
        results[offset + 4u] = random.color - random_shift.color;
        results[offset + 5u] = random.color;
        results[offset + 6u] = plain.color;
        results[offset + 7u] = untouched.color - reference.color;
        results[offset + 8u] = vec4<f32>(untouched.slope - reference.slope, untouched.metallic - reference.metallic, 0.0);
        results[offset + 9u] = far.color - far_reference.color;
        results[offset + 10u] = vec4<f32>(far.slope - far_reference.slope, far.metallic - far_reference.metallic, 0.0);
        let a = soil_offset(blend.sites[0]);
        let b = soil_offset(blend.sites[1]);
        let c = soil_offset(blend.sites[2]);
        results[offset + 11u] = vec4<f32>(min(a, min(b, c)), max(a, max(b, c)));
    }
}
";

pub fn bindings(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (wgpu::BindGroupLayout, wgpu::BindGroup) {
    let library = MaterialLibrary::embedded().unwrap();
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../assets/materials/sources.json")).unwrap();
    assert_eq!(destructible_fps::Material::Soil as u8, 1);
    assert_eq!(manifest["materials"][0]["material"], "soil");
    let scales = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&library.scales),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let texture = |format, data| {
        device
            .create_texture_with_data(
                queue,
                &wgpu::TextureDescriptor {
                    label: Some("real production pack for soil validation"),
                    size: wgpu::Extent3d {
                        width: MATERIAL_TEXTURE_EDGE,
                        height: MATERIAL_TEXTURE_EDGE,
                        depth_or_array_layers: MATERIAL_TEXTURE_LAYERS,
                    },
                    mip_level_count: MATERIAL_TEXTURE_MIPS,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                data,
            )
            .create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            })
    };
    let color = texture(
        wgpu::TextureFormat::Rgba8UnormSrgb,
        library.color_roughness(),
    );
    let normal = texture(wgpu::TextureFormat::Rgba8Unorm, library.normal_metalness());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        anisotropy_clamp: 8,
        ..Default::default()
    });
    let layout = layout(device);
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&color),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&normal),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: scales.as_entire_binding(),
            },
        ],
    });
    (layout, group)
}

fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2Array,
            multisampled: false,
        },
        count: None,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            texture_entry(0),
            texture_entry(1),
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    })
}

pub fn validate(values: &[[f32; 4]]) {
    for row in values[17067..].chunks_exact(4) {
        assert!(row.iter().flatten().all(|v| v.is_finite()));
        assert!(
            row[..2].iter().flatten().all(|v| v.abs() < 1e-5),
            "full projection: {row:?}"
        );
        assert!(row[2].iter().all(|v| (0.0..=1.1).contains(v)));
        let length = row[3][..3].iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((length - 1.0).abs() < 1e-5);
    }
    let mut repeat_energy = 0.0;
    let mut samples = [Vec::new(), Vec::new()];
    let mut max_seam = 0.0_f32;
    let stride = usize::try_from(STRIDE).unwrap();
    for i in 0..usize::try_from(SAMPLES).unwrap() {
        let row = &values[OFFSET + i * stride..][..stride];
        assert!(row.iter().flatten().all(|v| v.is_finite()));
        assert!(row[0][..3].iter().all(|v| (0.0..=1.0).contains(v)));
        assert!((row[0][..3].iter().sum::<f32>() - 1.0).abs() < 0.00001);
        assert!(row[0][3].abs() < 1e-7, "soil is nonmetallic");
        assert!(row[11].iter().all(|v| (0.0..1.0).contains(v)));
        max_seam = max_seam.max(row[1].iter().map(|v| v.abs()).fold(0.0, f32::max));
        assert!(
            row[1].iter().all(|v| v.abs() < 0.025),
            "color seam {i}: {:?}",
            row[1]
        );
        assert!(
            row[2].iter().all(|v| v.abs() < 0.15),
            "slope seam {i}: {:?}",
            row[2]
        );
        for field in [3, 7, 8, 9, 10] {
            assert!(
                row[field].iter().all(|v| v.abs() < 0.0001),
                "invariant {i}/{field}: {:?}",
                row[field]
            );
        }
        assert!(row[5].iter().all(|v| (0.0..=1.0).contains(v)));
        repeat_energy += f64::from(row[4][0]).powi(2);
        samples[0].push(f64::from(row[5][0]));
        samples[1].push(f64::from(row[6][0]));
    }
    let variance = |v: &[f64]| {
        let count = f64::from(u32::try_from(v.len()).unwrap());
        let mean = v.iter().sum::<f64>() / count;
        v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / count
    };
    let ratio = variance(&samples[0]) / variance(&samples[1]);
    let rms = (repeat_energy / f64::from(u32::try_from(SAMPLES).unwrap())).sqrt();
    println!(
        "SOIL_GPU old_tile_shift_rms={rms:.6} contrast_variance_ratio={ratio:.6} max_color_seam={max_seam:.6}"
    );
    assert!(
        rms > 0.01,
        "randomized output must break the old one-tile period"
    );
    assert!(
        (0.55..1.2).contains(&ratio),
        "excessive contrast loss/gain: {ratio}"
    );
}
