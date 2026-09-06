//! Hardware-only regression for the production depth passes and WGSL visibility receiver.
use super::super::{BodyInstance, GpuBody, GpuMesh, create_gpu_mesh, create_shadow_sampler};
use super::*;
use crate::{
    Material, Voxel, World,
    mesh::{CpuMesh, mesh_chunk},
};

const HARNESS: &str = r"
@group(0) @binding(7) var<storage, read_write> result: array<vec4<f32>>;
@compute @workgroup_size(1)
fn sample_visibility() {
    let up = vec3<f32>(0.0, 1.0, 0.0);
    result[0] = vec4<f32>(environment_visibility(vec3<f32>(0.5, 1.01, 0.5), up, up, up, 0.0), 0.0, 0.0);
    result[1] = vec4<f32>(environment_visibility(vec3<f32>(64.0, 1.01, 0.5), up, up, up, 1.0), 0.0, 0.0);
    result[2] = vec4<f32>(environment_visibility(vec3<f32>(0.5, 1.01, 0.5), up, up,
        normalize(vec3<f32>(1.0, 0.01, 0.0)), 0.5), 0.0, 0.0);
    result[3] = vec4<f32>(environment_visibility(vec3<f32>(44.5, 1.01, 0.5), up, up, up, 1.0), 0.0, 0.0);
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
        .expect("actual Vulkan adapter required");
    let info = adapter.get_info();
    assert_ne!(
        info.device_type,
        wgpu::DeviceType::Cpu,
        "software GPU is not hardware coverage"
    );
    println!("sky visibility: {} ({:?})", info.name, info.backend);
    adapter
        .request_device(&wgpu::DeviceDescriptor::default())
        .await
        .expect("Vulkan device")
}

struct Fixture {
    device: wgpu::Device,
    queue: wgpu::Queue,
    sky: SkyVisibility,
    pipeline: wgpu::ComputePipeline,
    receiver: wgpu::BindGroup,
    output: wgpu::BindGroup,
    results: wgpu::Buffer,
    readback: wgpu::Buffer,
    chunks: HashMap<IVec3, GpuMesh>,
    bodies: HashMap<BodyId, GpuBody>,
    body_buffer: wgpu::Buffer,
    player_mesh: GpuMesh,
    player_count: usize,
}

impl Fixture {
    fn new() -> Self {
        let (device, queue) = pollster::block_on(gpu());
        let sky = SkyVisibility::new(&device);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("production sky receiver test"),
            source: wgpu::ShaderSource::Wgsl(format!("{}\n{HARNESS}", super::super::SHADER).into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &shader,
            entry_point: Some("sample_visibility"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        let sampler = create_shadow_sampler(&device);
        let receiver = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(1),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&sky.view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: sky.buffer.as_entire_binding(),
                },
            ],
        });
        let results = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let output = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 7,
                resource: results.as_entire_binding(),
            }],
        });
        let body_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::bytes_of(&BodyInstance {
                model: Mat4::IDENTITY.to_cols_array_2d(),
            }),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });
        let mut placeholder = World::default();
        placeholder.set_voxel(IVec3::new(0, 0, 0), Voxel::new(Material::Brick));
        let player_mesh = create_gpu_mesh(
            &device,
            &mesh_chunk(&placeholder, IVec3::new(0, 0, 0)),
            "unused player mesh",
        );
        Self {
            device,
            queue,
            sky,
            pipeline,
            receiver,
            output,
            results,
            readback,
            chunks: HashMap::new(),
            bodies: HashMap::new(),
            body_buffer,
            player_mesh,
            player_count: 0,
        }
    }

    fn install_world(&mut self, world: &World) {
        self.chunks = world
            .chunk_positions()
            .into_iter()
            .map(|position| {
                (
                    position,
                    create_gpu_mesh(&self.device, &mesh_chunk(world, position), "test room"),
                )
            })
            .collect();
        self.sky.invalidate();
    }

    fn sample(&mut self, camera: Vec3) -> Vec<[f32; 4]> {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        self.sky.encode(
            &mut encoder,
            &self.queue,
            camera,
            &self.chunks,
            &self.bodies,
            &self.body_buffer,
            &self.player_mesh,
            &self.body_buffer,
            self.player_count,
            None,
        );
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.output, &[]);
            pass.set_bind_group(1, &self.receiver, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&self.results, 0, &self.readback, 0, 64);
        self.queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        self.readback
            .map_async(wgpu::MapMode::Read, .., move |status| {
                sender.send(status).unwrap();
            });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(10)),
            })
            .unwrap();
        receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap()
            .unwrap();
        let result = bytemuck::cast_slice(&self.readback.get_mapped_range(..).unwrap()).to_vec();
        self.readback.unmap();
        result
    }

    fn install_slab(&mut self) {
        let mut world = World::default();
        world.fill_box(
            IVec3::new(-7, 4, -7),
            IVec3::new(7, 4, 7),
            Voxel::new(Material::Brick),
        );
        let mut mesh = CpuMesh::default();
        for chunk in world.chunk_positions() {
            let part = mesh_chunk(&world, chunk);
            let offset = u32::try_from(mesh.vertices.len()).unwrap();
            mesh.vertices.extend(part.vertices);
            mesh.indices
                .extend(part.indices.into_iter().map(|index| index + offset));
        }
        self.bodies.insert(
            1,
            GpuBody {
                mesh: create_gpu_mesh(&self.device, &mesh, "movable sky occluder"),
                instance_slot: 0,
                extent: Vec3::new(15.0, 1.0, 15.0),
                rotation_pivot: Vec3::ZERO,
                world_minimum: Vec3::new(-7.0, 4.0, -7.0),
                world_maximum: Vec3::new(8.0, 5.0, 8.0),
            },
        );
        self.sky.invalidate();
    }
}

fn assert_visibility(value: &[f32; 4], expected: f32) {
    assert!(
        (value[0] - expected).abs() < 0.01 && (value[1] - expected).abs() < 0.01,
        "visibility {value:?}, expected {expected}"
    );
}

#[allow(clippy::cast_possible_truncation)]
fn reference_visibility(world: &World) -> [f32; 4] {
    // Independent bounded CPU ray stepping through the exact one-voxel test geometry.
    // Removing a roof does NOT expose the full hemisphere: low rays still hit side walls.
    let plan = Parameters::at(Vec3::ZERO);
    let origin = Vec3::new(0.5, 1.01 + plan.settings[1], 0.5);
    let mut visible = [0.0, 0.0];
    let mut total = [0.0, 0.0];
    for direction in plan.directions {
        let direction = Vec3::from_slice(&direction);
        let cosine = direction.y.max(0.0);
        let weights = [cosine, cosine.powi(32)];
        let blocked = (1_u16..=2048).any(|step| {
            let p = (origin + direction * (f32::from(step) / 16.0)).floor();
            world
                .voxel(IVec3::new(p.x as i32, p.y as i32, p.z as i32))
                .is_solid()
        });
        for channel in 0..2 {
            total[channel] += weights[channel];
            if !blocked {
                visible[channel] += weights[channel];
            }
        }
    }
    [visible[0] / total[0], visible[1] / total[1], 0.0, 0.0]
}

fn assert_reference(actual: &[f32; 4], expected: &[f32; 4]) {
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(a, b)| (a - b).abs() < 0.01),
        "GPU {actual:?} differs from independent geometry rays {expected:?}"
    );
}

#[test]
#[ignore = "requires an actual Vulkan GPU; execute explicitly with --ignored"]
fn coverage_moves_continuously_even_when_the_cached_anchor_changes() {
    let mut fixture = Fixture::new();
    let mut room = World::default();
    room.fill_box(
        IVec3::new(36, 0, -8),
        IVec3::new(52, 6, 8),
        Voxel::new(Material::Brick),
    );
    room.fill_box(IVec3::new(37, 1, -7), IVec3::new(51, 5, 7), Voxel::AIR);
    fixture.install_world(&room);
    let before = fixture.sample(Vec3::new(7.9, 0.0, 0.0))[3];
    let after = fixture.sample(Vec3::new(8.1, 0.0, 0.0))[3];
    assert!(
        before[0] > 0.1 && before[0] < 0.4,
        "actual bounded fade {before:?}"
    );
    assert!(
        (before[0] - after[0]).abs() < 0.05,
        "anchor popping {before:?} -> {after:?}"
    );
    assert_eq!(fixture.sky.stats.rebuilds, 2);
    let within = fixture.sample(Vec3::new(8.5, 0.0, 0.0))[3];
    assert!(!fixture.sky.stats.refreshed);
    assert!(
        within[0] < after[0],
        "cached receiver coverage must keep following camera"
    );
}

#[test]
#[ignore = "requires an actual Vulkan GPU; execute explicitly with --ignored"]
fn depth_views_occlude_sealed_rooms_and_follow_removal_and_moving_bodies() {
    let mut fixture = Fixture::new();
    let mut world = World::default();
    world.fill_box(
        IVec3::new(-8, 0, -8),
        IVec3::new(8, 6, 8),
        Voxel::new(Material::Brick),
    );
    world.fill_box(IVec3::new(-7, 1, -7), IVec3::new(7, 5, 7), Voxel::AIR);
    let fingerprint = world.fingerprint();
    fixture.install_world(&world);
    let sealed = fixture.sample(Vec3::ZERO);
    assert_visibility(&sealed[0], 0.0); // one-voxel walls/roof, no positive light floor
    assert_visibility(&sealed[1], 1.0); // explicitly outside bounded coverage
    assert_eq!(world.fingerprint(), fingerprint); // rendering does not mutate authority
    assert_eq!(fixture.sky.stats.rebuilds, 1);
    let cached = fixture.sample(Vec3::splat(1.0));
    assert_eq!(cached, sealed);
    assert!(!fixture.sky.stats.refreshed);
    assert_eq!(fixture.sky.stats.draw_calls, 0);
    assert_eq!(fixture.sky.stats.cache_hits, 1);
    world.fill_box(IVec3::new(-8, 6, -8), IVec3::new(8, 6, 8), Voxel::AIR);
    fixture.install_world(&world);
    let open = reference_visibility(&world);
    assert!(open[0] > 0.5 && open[0] < 1.0);
    assert_reference(&fixture.sample(Vec3::ZERO)[0], &open);
    assert_eq!(fixture.sky.stats.rebuilds, 2);
    fixture.install_slab();
    assert_visibility(&fixture.sample(Vec3::ZERO)[0], 0.0);
    let translation = Vec3::new(40.0, 0.0, 0.0);
    fixture.queue.write_buffer(
        &fixture.body_buffer,
        0,
        bytemuck::bytes_of(&BodyInstance {
            model: Mat4::from_translation(translation).to_cols_array_2d(),
        }),
    );
    let body = fixture.bodies.get_mut(&1).unwrap();
    body.world_minimum += translation;
    body.world_maximum += translation;
    fixture.sky.invalidate();
    assert_reference(&fixture.sample(Vec3::ZERO)[0], &open);
    // Restore the blocking pose before clearing: removal must actually change visibility.
    fixture.queue.write_buffer(
        &fixture.body_buffer,
        0,
        bytemuck::bytes_of(&BodyInstance {
            model: Mat4::IDENTITY.to_cols_array_2d(),
        }),
    );
    let body = fixture.bodies.get_mut(&1).unwrap();
    body.world_minimum -= translation;
    body.world_maximum -= translation;
    fixture.sky.invalidate();
    assert_visibility(&fixture.sample(Vec3::ZERO)[0], 0.0);
    fixture.bodies.clear();
    fixture.sky.invalidate();
    let no_bodies = fixture.sample(Vec3::ZERO);
    assert_reference(&no_bodies[0], &open);
    assert!(
        no_bodies
            .iter()
            .flatten()
            .all(|x| x.is_finite() && (0.0..=1.0).contains(x))
    );
    let rebuilds = fixture.sky.stats.rebuilds;
    let shifted = fixture.sample(Vec3::new(16.0, 0.0, 0.0));
    assert_reference(&shifted[0], &open);
    assert_eq!(fixture.sky.stats.rebuilds, rebuilds + 1);
    world.fill_box(IVec3::new(-8, 1, -8), IVec3::new(8, 6, 8), Voxel::AIR);
    fixture.install_world(&world);
    assert_visibility(&fixture.sample(Vec3::ZERO)[0], 1.0); // bare floor, no self-occlusion
}

#[test]
#[ignore = "requires an actual Vulkan GPU; execute explicitly with --ignored"]
fn player_instance_depth_tracks_a_finite_occluder_motion_and_removal() {
    let mut fixture = Fixture::new();
    let mut world = World::default();
    world.fill_box(
        IVec3::new(-8, 0, -8),
        IVec3::new(8, 0, 8),
        Voxel::new(Material::Brick),
    );
    fixture.install_world(&world);
    assert_visibility(&fixture.sample(Vec3::ZERO)[0], 1.0);
    // Exercise the distinct player draw/buffer path with a broad synthetic occluder.
    // This is a depth-pipeline fixture, not a claim about player collision dimensions.
    let mut with_slab = world.clone();
    with_slab.fill_box(
        IVec3::new(-7, 4, -7),
        IVec3::new(7, 4, 7),
        Voxel::new(Material::Brick),
    );
    // With side walls removed, grazing sky rays pass around the finite slab edges.
    let slab_reference = reference_visibility(&with_slab);
    assert!(slab_reference[0] > 0.0 && slab_reference[0] < 0.2);
    fixture.install_slab();
    fixture.player_mesh = fixture.bodies.remove(&1).unwrap().mesh;
    fixture.player_count = 1;
    assert_reference(&fixture.sample(Vec3::ZERO)[0], &slab_reference);
    let translation = Vec3::new(40.0, 0.0, 0.0);
    let mut moved_slab = world.clone();
    moved_slab.fill_box(
        IVec3::new(33, 4, -7),
        IVec3::new(47, 4, 7),
        Voxel::new(Material::Brick),
    );
    // Moving a caster sideways may still occlude a grazing direction far away.
    let moved_reference = reference_visibility(&moved_slab);
    assert!(moved_reference[0] > slab_reference[0] + 0.5);
    fixture.queue.write_buffer(
        &fixture.body_buffer,
        0,
        bytemuck::bytes_of(&BodyInstance {
            model: Mat4::from_translation(translation).to_cols_array_2d(),
        }),
    );
    fixture.sky.invalidate();
    assert_reference(&fixture.sample(Vec3::ZERO)[0], &moved_reference);
    fixture.queue.write_buffer(
        &fixture.body_buffer,
        0,
        bytemuck::bytes_of(&BodyInstance {
            model: Mat4::IDENTITY.to_cols_array_2d(),
        }),
    );
    fixture.sky.invalidate();
    assert_reference(&fixture.sample(Vec3::ZERO)[0], &slab_reference);
    fixture.player_count = 0;
    fixture.sky.invalidate();
    assert_visibility(&fixture.sample(Vec3::ZERO)[0], 1.0);
}
