//! Real raster interpolation of production vertex outputs and the cut-core predicate.
//! No texture library, screenshot, generated image or CPU shader substitute is involved.
use destructible_fps::{Material, mesh::Vertex};
use glam::{Mat4, Vec4};
use std::time::Duration;
use wgpu::util::DeviceExt;

const WORLD_SHADER: &str = concat!(
    include_str!("../src/shaders/color.wgsl"),
    include_str!("../src/shaders/world.wgsl")
);
const HARNESS: &str = r"
@fragment
fn finish_raster_probe(input: VertexOutput) -> @location(0) vec4<u32> {
    let old_predicate = select(0u, 1u, explicit_cut_core(input.fracture_depth, input.material));
    return vec4<u32>(bitcast<u32>(input.fracture_depth), input.material,
        input.authored_cut_core | (old_predicate << 1u), u32(input.damage));
}
";
const TILE: u32 = 64;
const WIDTH: u32 = 12 * TILE;
const HEIGHT: u32 = 4 * TILE;
const ROW_BYTES: u32 = WIDTH * 16;
const READBACK_BYTES: u64 = (ROW_BYTES * HEIGHT) as u64;
const W_PROFILES: [[f32; 3]; 4] = [
    [1.0, 1.0, 1.0],
    [1.0, 17.0, 113.0],
    [0.03125, 4096.0, 7.3],
    [0.11, 1.7, 39.37],
];

#[derive(Clone, Copy)]
struct Case {
    material: Material,
    markers: [f32; 3],
    cut: bool,
}

fn cases() -> [Case; 12] {
    [
        (Material::Brick, [-2.0; 3], true),
        (Material::Concrete, [-2.0; 3], true),
        (Material::Brick, [-1.0; 3], false),
        (Material::Brick, [0.0; 3], false),
        (Material::Concrete, [0.5; 3], false),
        (Material::Brick, [1.0; 3], false),
        (Material::Soil, [-2.0; 3], false),
        (Material::Stone, [-2.0; 3], false),
        (Material::Steel, [-2.0; 3], false),
        (Material::Glass, [-2.0; 3], false),
        (Material::Brick, [0.0, 0.5, 1.0], false),
        (Material::Concrete, [-1.0; 3], false),
    ]
    .map(|(material, markers, cut)| Case {
        material,
        markers,
        cut,
    })
}

fn bounded_float(value: u32) -> f32 {
    f32::from(u16::try_from(value).unwrap())
}

fn vertices() -> Vec<Vertex> {
    let mut vertices = Vec::with_capacity(12 * 4 * 3);
    for (row, depths) in W_PROFILES.into_iter().enumerate() {
        for (column, case) in cases().into_iter().enumerate() {
            let row = u32::try_from(row).unwrap();
            let column = u32::try_from(column).unwrap();
            for (index, [x, y]) in [[8, 8], [56, 16], [24, 56]].into_iter().enumerate() {
                let x = bounded_float(column * TILE + x);
                let y = bounded_float(row * TILE + y);
                let w = depths[index];
                vertices.push(Vertex {
                    // Projection below produces clip W = source Z, without clipping the triangle.
                    position: [
                        (2.0 * x / bounded_float(WIDTH) - 1.0) * w,
                        (1.0 - 2.0 * y / bounded_float(HEIGHT)) * w,
                        w,
                    ],
                    normal: [0.0, 0.0, 1.0],
                    albedo_roughness: [0.2, 0.3, 0.1, 0.8],
                    ambient_occlusion: 1.0,
                    metallic: 0.0,
                    material: u32::from(case.material as u8),
                    // The existing flat damage varying labels the primitive independently.
                    damage: bounded_float(row * 12 + column + 1),
                    fracture_depth: case.markers[index],
                });
            }
        }
    }
    vertices
}

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
        .expect("explicit raster validation requires a Vulkan adapter");
    let info = adapter.get_info();
    assert_ne!(
        info.device_type,
        wgpu::DeviceType::Cpu,
        "software GPU is not coverage"
    );
    println!(
        "FINISH_RASTER adapter={} backend={:?} driver={}",
        info.name, info.backend, info.driver_info
    );
    adapter
        .request_device(&wgpu::DeviceDescriptor::default())
        .await
        .expect("Vulkan device")
}

fn pipeline(device: &wgpu::Device) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production world vertex and cut predicate raster probe"),
        source: wgpu::ShaderSource::Wgsl(format!("{WORLD_SHADER}\n{HARNESS}").into()),
    });
    let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3,
        2 => Float32x4, 3 => Float32, 4 => Float32, 5 => Uint32, 10 => Float32, 11 => Float32];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("production interpolated finish classification"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            // The production entry point calls fill_vertex_output; no marker is a shader constant.
            entry_point: Some("world_vertex"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &attributes,
            })],
        },
        primitive: wgpu::PrimitiveState {
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("finish_raster_probe"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba32Uint,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn raster(device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<[u32; 4]> {
    let pipeline = pipeline(device);
    // Same 224-byte Globals contract as production; only the camera matrix differs.
    let mut globals = [0.0_f32; 56];
    globals[..16].copy_from_slice(
        &Mat4::from_cols(Vec4::X, Vec4::Y, Vec4::new(0.0, 0.0, 0.5, 1.0), Vec4::ZERO)
            .to_cols_array(),
    );
    globals[16..32].copy_from_slice(&Mat4::IDENTITY.to_cols_array());
    globals[32..48].copy_from_slice(&Mat4::IDENTITY.to_cols_array());
    let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("bounded raster projection"),
        contents: bytemuck::cast_slice(&globals),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform.as_entire_binding(),
        }],
    });
    let source = vertices();
    let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("48 isolated triangles with buffer-sourced finish markers"),
        contents: bytemuck::cast_slice(&source),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("bounded integer raster evidence"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Uint,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("3 MiB bounded raster readback"),
        size: READBACK_BYTES,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("48 finish oracle triangles"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.draw(0..u32::try_from(source.len()).unwrap(), 0..1);
    }
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(ROW_BYTES),
                rows_per_image: Some(HEIGHT),
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    readback.map_async(wgpu::MapMode::Read, .., move |status| {
        sender.send(status).unwrap();
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(10)),
        })
        .expect("bounded GPU completion");
    receiver
        .recv_timeout(Duration::from_secs(1))
        .expect("readback callback")
        .expect("readback map");
    let pixels =
        bytemuck::cast_slice(&readback.get_mapped_range(..).expect("mapped pixels")).to_vec();
    readback.unmap();
    pixels
}

#[test]
fn material_sampling_consumes_the_vertex_classified_flag_not_interpolated_equality() {
    // A source contract for the consumer complements the real raster oracle below. It is not
    // presented as rendered-material evidence: that also requires the native scene captures.
    let material = WORLD_SHADER
        .split("fn sample_material(")
        .nth(1)
        .unwrap()
        .split("\nfn ")
        .next()
        .unwrap();
    assert!(
        material.contains("if input.authored_cut_core != 0u {\n        return sample_cut_core(")
    );
    assert!(!material.contains("explicit_cut_core(input.fracture_depth"));
}

fn expected_gradient(x: u32, y: u32, row: usize) -> f64 {
    // Analytic perspective interpolation at the pixel centre for the submitted screen triangle.
    let x = f64::from(x % TILE) + 0.5 - 8.0;
    let y = f64::from(y % TILE) + 0.5 - 8.0;
    let b = 16.0_f64.mul_add(-y, 48.0 * x) / 2176.0;
    let c = 48.0_f64.mul_add(y, -8.0 * x) / 2176.0;
    let barycentric = [1.0 - b - c, b, c];
    let weights: [f64; 3] = std::array::from_fn(|i| barycentric[i] / f64::from(W_PROFILES[row][i]));
    weights[1].mul_add(0.5, weights[2]) / weights.iter().sum::<f64>()
}

#[test]
#[ignore = "requires a real Vulkan GPU; execute explicitly with --ignored"]
fn perspective_raster_preserves_cut_classification_and_continuous_depth_witnesses() {
    let (device, queue) = pollster::block_on(gpu());
    let pixels = raster(&device, &queue);
    assert_eq!(pixels.len(), usize::try_from(WIDTH * HEIGHT).unwrap());
    let mut covered = [0_usize; 48];
    let mut missed = [0_usize; 48];
    let mut old_missed = [0_usize; 48];
    let mut changed_bits = [0_usize; 48];
    let mut false_cuts = 0;
    let mut gradient = [f32::INFINITY, f32::NEG_INFINITY];
    let mut gradient_error = 0.0_f64;
    let cases = cases();
    for (index, [bits, material, flags, tag]) in pixels.into_iter().enumerate() {
        if tag == 0 {
            assert_eq!([bits, material, flags], [0; 3]);
            continue;
        }
        let tile = usize::try_from(tag - 1).unwrap();
        assert!(tile < 48);
        let x = u32::try_from(index).unwrap() % WIDTH;
        let y = u32::try_from(index).unwrap() / WIDTH;
        assert_eq!(
            tag - 1,
            y / TILE * 12 + x / TILE,
            "neighboring triangles contaminated a tile"
        );
        assert!(
            (7..=57).contains(&(x % TILE)) && (7..=57).contains(&(y % TILE)),
            "a triangle escaped its isolated footprint"
        );
        covered[tile] += 1;
        let case = cases[tile % 12];
        assert_eq!(material, u32::from(case.material as u8));
        assert!(flags <= 3);
        let cut = flags & 1;
        let old_cut = flags >> 1;
        let marker = f32::from_bits(bits);
        assert!(marker.is_finite());
        if case.cut {
            missed[tile] += usize::from(cut != 1);
            old_missed[tile] += usize::from(old_cut != 1);
            changed_bits[tile] += usize::from(bits != (-2.0_f32).to_bits());
        } else {
            false_cuts += usize::from(cut != 0);
        }
        if tile % 12 == 10 {
            assert!((-1e-5..=1.00001).contains(&marker));
            let error = (f64::from(marker) - expected_gradient(x, y, tile / 12)).abs();
            gradient_error = gradient_error.max(error);
            assert!(
                error < 2e-4,
                "continuous perspective depth changed at ({x}, {y}): error={error}"
            );
            if tile == 10 {
                gradient[0] = gradient[0].min(marker);
                gradient[1] = gradient[1].max(marker);
            }
        }
    }
    assert!(
        covered.into_iter().all(|count| count > 500),
        "every triangle must produce real pixels"
    );
    assert!(
        gradient[1] - gradient[0] > 0.5,
        "continuous 0..1 wall depth must not become flat"
    );
    for (row, depths) in W_PROFILES.into_iter().enumerate() {
        for (column, case) in cases.iter().enumerate().take(2) {
            let tile = row * 12 + column;
            println!(
                "FINISH_RASTER w={depths:?} material={:?} pixels={} changed_marker_bits={} old_missed_cut={} fixed_missed_cut={}",
                case.material, covered[tile], changed_bits[tile], old_missed[tile], missed[tile]
            );
        }
    }
    println!("FINISH_RASTER perspective_gradient_max_error={gradient_error}");
    assert_eq!(
        false_cuts, 0,
        "ordinary depth or non-masonry material classified as cut core"
    );
    assert_eq!(
        missed.into_iter().sum::<usize>(),
        0,
        "production perspective interpolation must preserve an explicitly authored cut finish"
    );
}
