//! Real-GPU proof of HDR resolve, display transfer, HUD composition and finite radiance.
use super::*;

const PATTERN: &str = r"
@vertex
fn diagonal(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let points = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0));
    return vec4<f32>(points[index], 0.0, 1.0);
}
@fragment
fn flat() -> @location(0) vec4<f32> { return vec4<f32>(4.0, 4.0, 4.0, 1.0); }

@group(0) @binding(2) var<storage, read> source: array<vec4<u32>>;
@fragment
fn pattern(@builtin(position) position: vec4<f32>, @builtin(sample_index) sample: u32)
    -> @location(0) vec4<f32> {
    var value = bitcast<vec3<f32>>(source[u32(position.x)].xyz);
    if sample % 2u == 1u { value = vec3<f32>(0.0); }
    return vec4<f32>(finite_hdr(value), 1.0);
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
        .expect("actual Vulkan GPU required");
    let info = adapter.get_info();
    assert_ne!(
        info.device_type,
        wgpu::DeviceType::Cpu,
        "software device is not hardware evidence"
    );
    println!("HDR/MSAA: {} ({:?})", info.name, info.backend);
    validate_formats(
        4,
        adapter.get_texture_format_features(HDR_FORMAT),
        adapter.get_texture_format_features(super::super::DEPTH_FORMAT),
    )
    .unwrap();
    adapter
        .request_device(&wgpu::DeviceDescriptor::default())
        .await
        .unwrap()
}

fn pattern_pipeline(device: &wgpu::Device, samples: u32, geometry: bool) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("explicit per-sample HDR fixture"),
        source: wgpu::ShaderSource::Wgsl(format!("{SHADER}\n{PATTERN}").into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some(if geometry { "diagonal" } else { "fullscreen" }),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: super::super::DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: samples,
            ..Default::default()
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some(if geometry { "flat" } else { "pattern" }),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: HDR_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    display: &DisplayPass,
    output: &wgpu::Texture,
    pipeline: &wgpu::RenderPipeline,
    values: &[f32],
) {
    let bits: Vec<_> = values.iter().map(|value| [value.to_bits(); 4]).collect();
    let source = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("HDR fixture finite and nonfinite input bits"),
        contents: bytemuck::cast_slice(&bits),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 2,
            resource: source.as_entire_binding(),
        }],
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("actual HDR spatial resolve fixture"),
            color_attachments: &[Some(display.color_attachment())],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &display.depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
    let view = output.create_view(&wgpu::TextureViewDescriptor::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("production HDR display fixture"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::RED),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&display.pipeline);
        pass.set_bind_group(0, &display.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
    queue.submit([encoder.finish()]);
}

fn read_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    bytes_per_pixel: u32,
) -> Vec<u8> {
    let row_bytes = texture.width() * bytes_per_pixel;
    let stride = row_bytes.div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(stride) * u64::from(texture.height()),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(texture.height()),
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    buffer.map_async(wgpu::MapMode::Read, .., move |status| {
        sender.send(status).unwrap();
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(10)),
        })
        .unwrap();
    receiver
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap()
        .unwrap();
    let mapped = buffer.get_mapped_range(..).unwrap();
    let result = mapped
        .chunks_exact(stride as usize)
        .flat_map(|row| row[..row_bytes as usize].iter().copied())
        .collect();
    drop(mapped);
    buffer.unmap();
    result
}

fn half_value(bytes: &[u8]) -> f32 {
    let bits = u16::from_le_bytes([bytes[0], bytes[1]]);
    let exponent = (bits >> 10) & 31;
    assert!(exponent < 31, "resolved HDR contains a non-finite value");
    let fraction = f32::from(bits & 1023);
    let unsigned = if exponent == 0 {
        fraction / 16_777_216.0
    } else {
        (1.0 + fraction / 1024.0) * 2_f32.powi(i32::from(exponent) - 15)
    };
    if bits & 0x8000 == 0 {
        unsigned
    } else {
        -unsigned
    }
}

#[test]
#[ignore = "requires an actual Vulkan GPU; execute explicitly with --ignored"]
fn spatial_msaa_resolves_actual_polygon_edge_coverage_without_sample_shading() {
    let (device, queue) = pollster::block_on(gpu());
    for samples in [1, 4] {
        let plan =
            FramePlan::new(65, 65, samples, device.limits().max_texture_dimension_2d).unwrap();
        let display = DisplayPass::new(&device, plan, wgpu::TextureFormat::Rgba8UnormSrgb);
        let pipeline = pattern_pipeline(&device, samples, true);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("polygon edge spatial coverage fixture"),
                color_attachments: &[Some(display.color_attachment())],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &display.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(&pipeline);
            pass.draw(0..3, 0..1);
        }
        queue.submit([encoder.finish()]);
        let bytes = read_texture(&device, &queue, &display.hdr, 8);
        let values: Vec<_> = bytes.chunks_exact(8).map(half_value).collect();
        assert!(values.contains(&0.0) && values.contains(&4.0));
        let partial = values
            .iter()
            .filter(|&&value| value > 0.1 && value < 3.9)
            .count();
        if samples == 1 {
            assert_eq!(partial, 0);
        } else {
            assert!(
                partial >= 32,
                "4x must produce fractional polygon edge coverage: {partial}"
            );
        }
    }
}

#[allow(clippy::cast_possible_truncation)]
fn tone(value: f32) -> f32 {
    let c = f64::from(value) * f64::from(super::super::SCENE_EXPOSURE);
    ((c * 2.51_f64.mul_add(c, 0.03)) / c.mul_add(2.43_f64.mul_add(c, 0.59), 0.14)).clamp(0.0, 1.0)
        as f32
}

fn srgb(value: f32) -> f32 {
    if value <= 0.003_130_8 {
        12.92 * value
    } else {
        1.055_f32.mul_add(value.powf(1.0 / 2.4), -0.055)
    }
}

fn output_texture(
    device: &wgpu::Device,
    plan: FramePlan,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: plan.width,
            height: plan.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

#[test]
#[ignore = "requires an actual Vulkan GPU; execute explicitly with --ignored"]
fn linear_hdr_resolves_before_display_with_correct_transfer_hud_and_no_history() {
    let (device, queue) = pollster::block_on(gpu());
    for (width, height) in [(257, 65), (129, 33)] {
        for samples in [1, 4] {
            let mut reference = None;
            for format in [
                wgpu::TextureFormat::Rgba8UnormSrgb,
                wgpu::TextureFormat::Rgba8Unorm,
            ] {
                let plan = FramePlan::new(
                    width,
                    height,
                    samples,
                    device.limits().max_texture_dimension_2d,
                )
                .unwrap();
                let display = DisplayPass::new(&device, plan, format);
                let output = output_texture(&device, plan, format);
                let pipeline = pattern_pipeline(&device, samples, false);
                let mut values = vec![4.0; width as usize];
                values[..9].copy_from_slice(&[
                    4.0,
                    0.001,
                    0.18,
                    64.0,
                    65504.0,
                    f32::NAN,
                    f32::INFINITY,
                    f32::NEG_INFINITY,
                    -0.5,
                ]);
                values[(width / 2) as usize] = 0.0;
                render(&device, &queue, &display, &output, &pipeline, &values);
                let hdr = read_texture(&device, &queue, &display.hdr, 8);
                let bytes = read_texture(&device, &queue, &output, 4);
                for (index, &value) in values[..9].iter().enumerate() {
                    let radiance = if value.is_finite() {
                        value.clamp(0.0, 65504.0)
                    } else {
                        0.0
                    } * if samples == 4 { 0.5 } else { 1.0 };
                    let actual = half_value(&hdr[index * 8..]);
                    assert!(
                        (actual - radiance).abs() <= radiance * 0.001 + 1e-6,
                        "HDR {actual} != {radiance}, {samples}x, {format:?}"
                    );
                    let expected = srgb(tone(actual)) * 255.0;
                    assert!(
                        (f32::from(bytes[index * 4]) - expected).abs() <= 2.0,
                        "display {} != {expected}, {samples}x, {format:?}",
                        bytes[index * 4]
                    );
                }
                if samples == 4 {
                    assert!((half_value(&hdr) - 2.0).abs() < 0.001);
                    let incorrect = srgb(0.5 * tone(4.0)) * 255.0;
                    assert!(
                        (f32::from(bytes[0]) - incorrect).abs() > 30.0,
                        "must tone-map average radiance, not average tone-mapped samples"
                    );
                }
                let center = (((height / 2) * width + width / 2) * 4) as usize;
                for (channel, value) in [0.96, 0.98, 1.0].into_iter().enumerate() {
                    assert!(
                        (f32::from(bytes[center + channel]) - srgb(value * 0.92) * 255.0).abs()
                            <= 2.0
                    );
                }
                if let Some(previous) = reference.take() {
                    let previous: Vec<u8> = previous;
                    assert!(
                        previous
                            .iter()
                            .zip(&bytes)
                            .all(|(a, b)| a.abs_diff(*b) <= 1),
                        "hardware sRGB and manual transfer differ"
                    );
                } else {
                    reference = Some(bytes);
                }
                // Reuse identical targets with an entirely changed scene: no stale resolved frame.
                render(
                    &device,
                    &queue,
                    &display,
                    &output,
                    &pipeline,
                    &vec![0.0; width as usize],
                );
                let cleared = read_texture(&device, &queue, &output, 4);
                assert_eq!(&cleared[..4], &[0, 0, 0, 255]);
            }
        }
    }
}
