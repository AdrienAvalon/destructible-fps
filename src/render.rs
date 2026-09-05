//! Safe `wgpu` presentation layer for the playable Linux slice.

#![allow(clippy::cast_precision_loss)]

use crate::{
    CHUNK_EDGE, IVec3,
    mesh::{CpuBodyMesh, CpuMesh, Vertex},
    replication::MAX_ACTIVE_BODIES,
};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, mpsc},
};
use wgpu::util::DeviceExt;
use winit::{dpi::PhysicalSize, window::Window};

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SHADOW_MAP_SIZE: u32 = 2_048;
const GPU_TIMESTAMP_COUNT: u32 = 4;
const GPU_TIMESTAMP_BYTES: u64 = 4 * 8;
const GPU_READBACK_SLOTS: usize = 4;
const MAX_COMPLETED_GPU_SAMPLES: usize = 16;
const BODY_INSTANCE_BYTES: u64 = 64;
const SHADER: &str = include_str!("shaders/world.wgsl");

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_projection: [[f32; 4]; 4],
    light_view_projection: [[f32; 4]; 4],
    camera_time: [f32; 4],
    sun_fog: [f32; 4],
    display: [f32; 4],
}

struct GpuMesh {
    vertex: wgpu::Buffer,
    index: wgpu::Buffer,
    index_count: u32,
}

struct GpuBody {
    mesh: GpuMesh,
    instance_slot: u32,
    world_minimum: Vec3,
    world_maximum: Vec3,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BodyInstance {
    model: [[f32; 4]; 4],
}

const _: () = assert!(size_of::<BodyInstance>() == 64);

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats {
    pub chunks: usize,
    pub exposed_faces: usize,
    pub visible_chunks: usize,
    pub bodies: usize,
    pub body_faces: usize,
    pub visible_bodies: usize,
    pub world_draw_calls: usize,
    pub shadow_draw_calls: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpuFrameTime {
    pub shadow_ms: f64,
    pub world_hud_ms: f64,
    pub total_ms: f64,
}

enum ReadbackState {
    Idle,
    Scheduled,
    Pending(mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>),
}

struct ReadbackSlot {
    buffer: wgpu::Buffer,
    state: ReadbackState,
}

struct GpuProfiler {
    query_set: wgpu::QuerySet,
    resolve_buffer: wgpu::Buffer,
    slots: Vec<ReadbackSlot>,
    next_slot: usize,
    timestamp_period_ns: f64,
    completed: VecDeque<GpuFrameTime>,
    dropped_samples: u64,
}

impl GpuProfiler {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("frame GPU timestamp queries"),
            ty: wgpu::QueryType::Timestamp,
            count: GPU_TIMESTAMP_COUNT,
        });
        let resolve_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame GPU timestamp resolve buffer"),
            size: GPU_TIMESTAMP_BYTES,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let slots = (0..GPU_READBACK_SLOTS)
            .map(|_| ReadbackSlot {
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("frame GPU timestamp readback"),
                    size: GPU_TIMESTAMP_BYTES,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                state: ReadbackState::Idle,
            })
            .collect();
        Self {
            query_set,
            resolve_buffer,
            slots,
            next_slot: 0,
            timestamp_period_ns: f64::from(queue.get_timestamp_period()),
            completed: VecDeque::with_capacity(MAX_COMPLETED_GPU_SAMPLES),
            dropped_samples: 0,
        }
    }

    fn poll(&mut self, device: &wgpu::Device) {
        if device.poll(wgpu::PollType::Poll).is_err() {
            return;
        }
        for slot in &mut self.slots {
            let result = match &slot.state {
                ReadbackState::Pending(receiver) => receiver.try_recv(),
                ReadbackState::Idle | ReadbackState::Scheduled => continue,
            };
            match result {
                Ok(Ok(())) => {
                    if let Ok(view) = slot.buffer.get_mapped_range(..) {
                        let timestamps = bytemuck::cast_slice::<u8, u64>(&view);
                        if let Some(sample) = gpu_frame_time(timestamps, self.timestamp_period_ns) {
                            if self.completed.len() == MAX_COMPLETED_GPU_SAMPLES {
                                self.completed.pop_front();
                                self.dropped_samples = self.dropped_samples.saturating_add(1);
                            }
                            self.completed.push_back(sample);
                        }
                        drop(view);
                    }
                    slot.buffer.unmap();
                    slot.state = ReadbackState::Idle;
                }
                Ok(Err(_)) | Err(mpsc::TryRecvError::Disconnected) => {
                    slot.state = ReadbackState::Idle;
                    self.dropped_samples = self.dropped_samples.saturating_add(1);
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
    }

    const fn timestamps(&self, begin: u32, end: u32) -> wgpu::RenderPassTimestampWrites<'_> {
        wgpu::RenderPassTimestampWrites {
            query_set: &self.query_set,
            beginning_of_pass_write_index: Some(begin),
            end_of_pass_write_index: Some(end),
        }
    }

    fn encode_readback(&mut self, encoder: &mut wgpu::CommandEncoder) -> Option<usize> {
        let slot_index = (0..self.slots.len())
            .map(|offset| (self.next_slot + offset) % self.slots.len())
            .find(|&index| matches!(self.slots[index].state, ReadbackState::Idle));
        let Some(slot_index) = slot_index else {
            self.dropped_samples = self.dropped_samples.saturating_add(1);
            return None;
        };
        encoder.resolve_query_set(
            &self.query_set,
            0..GPU_TIMESTAMP_COUNT,
            &self.resolve_buffer,
            0,
        );
        encoder.copy_buffer_to_buffer(
            &self.resolve_buffer,
            0,
            &self.slots[slot_index].buffer,
            0,
            GPU_TIMESTAMP_BYTES,
        );
        self.slots[slot_index].state = ReadbackState::Scheduled;
        self.next_slot = (slot_index + 1) % self.slots.len();
        Some(slot_index)
    }

    fn map_after_submit(&mut self, slot_index: usize) {
        let slot = &mut self.slots[slot_index];
        debug_assert!(matches!(slot.state, ReadbackState::Scheduled));
        let (sender, receiver) = mpsc::sync_channel(1);
        slot.buffer
            .map_async(wgpu::MapMode::Read, .., move |result| {
                let _ = sender.try_send(result);
            });
        slot.state = ReadbackState::Pending(receiver);
    }
}

fn gpu_frame_time(timestamps: &[u64], period_ns: f64) -> Option<GpuFrameTime> {
    let [shadow_begin, shadow_end, world_begin, world_end] = *timestamps else {
        return None;
    };
    if shadow_end < shadow_begin || world_begin < shadow_end || world_end < world_begin {
        return None;
    }
    let ticks_to_ms = |ticks: u64| ticks as f64 * period_ns / 1_000_000.0;
    Some(GpuFrameTime {
        shadow_ms: ticks_to_ms(shadow_end - shadow_begin),
        world_hud_ms: ticks_to_ms(world_end - world_begin),
        total_ms: ticks_to_ms(world_end - shadow_begin),
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderOutcome {
    Presented,
    Reconfigure,
    RecreateSurface,
    Skipped,
}

pub struct Renderer {
    instance: wgpu::Instance,
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    depth_view: wgpu::TextureView,
    _shadow_texture: wgpu::Texture,
    shadow_view: wgpu::TextureView,
    shadow_pipeline: wgpu::RenderPipeline,
    body_shadow_pipeline: wgpu::RenderPipeline,
    world_pipeline: wgpu::RenderPipeline,
    body_world_pipeline: wgpu::RenderPipeline,
    crosshair_pipeline: wgpu::RenderPipeline,
    globals_buffer: wgpu::Buffer,
    body_instance_buffer: wgpu::Buffer,
    globals_bind_group: wgpu::BindGroup,
    shadow_sampling_bind_group: wgpu::BindGroup,
    gpu_profiler: Option<GpuProfiler>,
    chunks: HashMap<IVec3, GpuMesh>,
    bodies: HashMap<u128, GpuBody>,
    stats: RenderStats,
}

impl Renderer {
    /// Initializes the Vulkan device without doing CPU meshing on this thread.
    ///
    /// # Errors
    ///
    /// Returns a diagnostic when the window surface or a compatible GPU is unavailable.
    #[allow(clippy::too_many_lines)]
    pub async fn new(window: Arc<Window>) -> Result<Self, String> {
        let size = non_zero_size(window.inner_size());
        let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        instance_descriptor.backends = wgpu::Backends::VULKAN;
        let instance = wgpu::Instance::new(instance_descriptor);
        let surface = instance
            .create_surface(Arc::clone(&window))
            .map_err(|error| format!("creation de la surface Vulkan: {error}"))?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
                apply_limit_buckets: false,
            })
            .await
            .map_err(|error| format!("aucun GPU Vulkan compatible: {error}"))?;
        let info = adapter.get_info();
        println!(
            "GPU: {} ({:?}, backend {:?}, pilote {})",
            info.name, info.device_type, info.backend, info.driver
        );
        let supports_gpu_timestamps = adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
        let required_features = if supports_gpu_timestamps {
            wgpu::Features::TIMESTAMP_QUERY
        } else {
            wgpu::Features::empty()
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("destructible-fps device"),
                required_features,
                ..Default::default()
            })
            .await
            .map_err(|error| format!("creation du device Vulkan: {error}"))?;
        let mut config = surface
            .get_default_config(&adapter, size.width, size.height)
            .ok_or_else(|| "la surface Vulkan ne fournit aucun format utilisable".to_owned())?;
        if let Some(format) = preferred_surface_format(&surface.get_capabilities(&adapter).formats)
        {
            config.format = format;
        }
        surface.configure(&device, &config);
        let gpu_profiler = supports_gpu_timestamps.then(|| GpuProfiler::new(&device, &queue));
        println!(
            "Telemetrie GPU par timestamps: {}",
            if supports_gpu_timestamps {
                "active"
            } else {
                "indisponible sur cet adaptateur"
            }
        );

        let globals = Globals {
            view_projection: Mat4::IDENTITY.to_cols_array_2d(),
            light_view_projection: Mat4::IDENTITY.to_cols_array_2d(),
            camera_time: [0.0; 4],
            sun_fog: [0.35, -0.90, 0.22, 0.010],
            display: [f32::from(!config.format.is_srgb()), 0.0, 0.0, 0.0],
        };
        let globals_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("camera and lighting globals"),
            contents: bytemuck::bytes_of(&globals),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let body_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rigid body instance transforms"),
            size: u64::try_from(MAX_ACTIVE_BODIES).unwrap_or(u64::MAX) * BODY_INSTANCE_BYTES,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (shadow_texture, shadow_view) = create_shadow_map(&device);
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow comparison sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let shadow_sampling_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("shadow sampling layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                        count: None,
                    },
                ],
            });
        let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals bind group"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buffer.as_entire_binding(),
            }],
        });
        let shadow_sampling_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow sampling bind group"),
            layout: &shadow_sampling_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&shadow_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&shadow_sampler),
                },
            ],
        });
        let world_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("world pipeline layout"),
                bind_group_layouts: &[Some(&globals_layout), Some(&shadow_sampling_layout)],
                immediate_size: 0,
            });
        let globals_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("globals-only pipeline layout"),
                bind_group_layouts: &[Some(&globals_layout)],
                immediate_size: 0,
            });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("procedural world shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let vertex_attributes = wgpu::vertex_attr_array![
            0 => Float32x3,
            1 => Float32x3,
            2 => Float32x4,
            3 => Float32
        ];
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &vertex_attributes,
        };
        let body_instance_attributes = wgpu::vertex_attr_array![
            4 => Float32x4,
            5 => Float32x4,
            6 => Float32x4,
            7 => Float32x4
        ];
        let body_instance_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<BodyInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &body_instance_attributes,
        };
        let color_target = wgpu::ColorTargetState {
            format: config.format,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        };
        let world_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("voxel world pipeline"),
            layout: Some(&world_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("world_vertex"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(vertex_layout.clone())],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("world_fragment"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(color_target)],
            }),
            multiview_mask: None,
            cache: None,
        });
        let body_world_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rigid body world pipeline"),
            layout: Some(&world_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("body_vertex"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[
                    Some(vertex_layout.clone()),
                    Some(body_instance_layout.clone()),
                ],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("world_fragment"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("directional shadow pipeline"),
            layout: Some(&globals_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("shadow_vertex"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(vertex_layout.clone())],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.0,
                    clamp: 0.0,
                },
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });
        let body_shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rigid body directional shadow pipeline"),
            layout: Some(&globals_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("body_shadow_vertex"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(vertex_layout), Some(body_instance_layout)],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.0,
                    clamp: 0.0,
                },
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });
        let crosshair_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("crosshair pipeline"),
            layout: Some(&globals_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("crosshair_vertex"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("crosshair_fragment"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let depth_view = create_depth_view(&device, size);
        let renderer = Self {
            instance,
            window,
            surface,
            device,
            queue,
            depth_view,
            _shadow_texture: shadow_texture,
            shadow_view,
            shadow_pipeline,
            body_shadow_pipeline,
            config,
            world_pipeline,
            body_world_pipeline,
            crosshair_pipeline,
            globals_buffer,
            body_instance_buffer,
            globals_bind_group,
            shadow_sampling_bind_group,
            gpu_profiler,
            chunks: HashMap::new(),
            bodies: HashMap::new(),
            stats: RenderStats::default(),
        };
        Ok(renderer)
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.device, &self.config);
        self.depth_view = create_depth_view(&self.device, size);
    }

    /// Recreates a lost presentation surface while retaining device resources.
    ///
    /// # Errors
    ///
    /// Returns a diagnostic if the operating-system window can no longer expose a surface.
    pub fn recreate_surface(&mut self) -> Result<(), String> {
        self.surface = self
            .instance
            .create_surface(Arc::clone(&self.window))
            .map_err(|error| format!("recreation de la surface Vulkan: {error}"))?;
        self.surface.configure(&self.device, &self.config);
        Ok(())
    }

    pub fn upload_chunk_meshes(&mut self, meshes: Vec<(IVec3, CpuMesh)>) {
        for (chunk, mesh) in meshes {
            self.upload_mesh(chunk, &mesh);
        }
        self.refresh_stats();
    }

    fn upload_mesh(&mut self, chunk: IVec3, mesh: &CpuMesh) {
        if mesh.indices.is_empty() {
            self.chunks.remove(&chunk);
            return;
        }
        self.chunks
            .insert(chunk, self.create_gpu_mesh(mesh, "chunk"));
    }

    /// Uploads body meshes already produced by the bounded background worker.
    ///
    /// # Errors
    ///
    /// Rejects more bodies than the fixed GPU instance arena can address.
    pub fn upload_body_meshes(&mut self, meshes: Vec<CpuBodyMesh>) -> Result<(), String> {
        for body in meshes {
            if body.mesh.indices.is_empty() {
                continue;
            }
            let instance_slot = if let Some(existing) = self.bodies.get(&body.body_id) {
                existing.instance_slot
            } else {
                u32::try_from(self.bodies.len()).map_err(|_| "too many rendered bodies")?
            };
            if usize::try_from(instance_slot).unwrap_or(usize::MAX) >= MAX_ACTIVE_BODIES {
                return Err(format!(
                    "rigid-body instance arena exhausted at {MAX_ACTIVE_BODIES} bodies"
                ));
            }
            let origin = Vec3::new(
                body.origin.x as f32,
                body.origin.y as f32,
                body.origin.z as f32,
            );
            let maximum = Vec3::new(
                body.maximum.x.saturating_add(1) as f32,
                body.maximum.y.saturating_add(1) as f32,
                body.maximum.z.saturating_add(1) as f32,
            );
            let instance = BodyInstance {
                model: Mat4::from_translation(origin).to_cols_array_2d(),
            };
            let offset = u64::from(instance_slot) * BODY_INSTANCE_BYTES;
            self.queue.write_buffer(
                &self.body_instance_buffer,
                offset,
                bytemuck::bytes_of(&instance),
            );
            self.bodies.insert(
                body.body_id,
                GpuBody {
                    mesh: self.create_gpu_mesh(&body.mesh, "rigid body"),
                    instance_slot,
                    world_minimum: origin,
                    world_maximum: maximum,
                },
            );
        }
        self.refresh_stats();
        Ok(())
    }

    fn create_gpu_mesh(&self, mesh: &CpuMesh, label: &str) -> GpuMesh {
        let index_count =
            u32::try_from(mesh.indices.len()).expect("bounded mesh index count fits in u32");
        let vertex = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(&mesh.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(&mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        GpuMesh {
            vertex,
            index,
            index_count,
        }
    }

    fn refresh_stats(&mut self) {
        self.stats.chunks = self.chunks.len();
        self.stats.exposed_faces = self
            .chunks
            .values()
            .map(|chunk| chunk.index_count as usize / 6)
            .sum();
        self.stats.bodies = self.bodies.len();
        self.stats.body_faces = self
            .bodies
            .values()
            .map(|body| body.mesh.index_count as usize / 6)
            .sum();
    }

    #[must_use]
    pub const fn stats(&self) -> RenderStats {
        self.stats
    }

    pub fn take_gpu_frame_time(&mut self) -> Option<GpuFrameTime> {
        self.gpu_profiler
            .as_mut()
            .and_then(|profiler| profiler.completed.pop_front())
    }

    #[must_use]
    pub fn gpu_timing_dropped_samples(&self) -> u64 {
        self.gpu_profiler
            .as_ref()
            .map_or(0, |profiler| profiler.dropped_samples)
    }

    #[must_use]
    pub const fn gpu_timing_supported(&self) -> bool {
        self.gpu_profiler.is_some()
    }

    #[allow(clippy::too_many_lines)]
    pub fn render(
        &mut self,
        camera_position: Vec3,
        view_direction: Vec3,
        elapsed_seconds: f32,
    ) -> RenderOutcome {
        if let Some(profiler) = &mut self.gpu_profiler {
            profiler.poll(&self.device);
        }
        let aspect = self.config.width as f32 / self.config.height as f32;
        let projection =
            glam::camera::rh::proj::directx::perspective(70_f32.to_radians(), aspect, 0.05, 420.0);
        let view = glam::camera::rh::view::look_to_mat4(camera_position, view_direction, Vec3::Y);
        let view_projection = projection * view;
        let light_view_projection = light_view_projection();
        let globals = Globals {
            view_projection: view_projection.to_cols_array_2d(),
            light_view_projection: light_view_projection.to_cols_array_2d(),
            camera_time: [
                camera_position.x,
                camera_position.y,
                camera_position.z,
                elapsed_seconds,
            ],
            sun_fog: [0.35, -0.90, 0.22, 0.010],
            display: [f32::from(!self.config.format.is_srgb()), 0.0, 0.0, 0.0],
        };
        self.queue
            .write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));

        let (output, suboptimal) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(output) => (output, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(output) => (output, true),
            wgpu::CurrentSurfaceTexture::Outdated => return RenderOutcome::Reconfigure,
            wgpu::CurrentSurfaceTexture::Lost => return RenderOutcome::RecreateSurface,
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => return RenderOutcome::Skipped,
        };
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame encoder"),
            });
        let visible_chunks: Vec<_> = self
            .chunks
            .iter()
            .filter(|(position, _chunk)| chunk_intersects_frustum(**position, view_projection))
            .map(|(_position, chunk)| chunk)
            .collect();
        let visible_bodies: Vec<_> = self
            .bodies
            .values()
            .filter(|body| {
                aabb_intersects_frustum(body.world_minimum, body.world_maximum, view_projection)
            })
            .collect();
        self.stats.visible_chunks = visible_chunks.len();
        self.stats.visible_bodies = visible_bodies.len();
        self.stats.world_draw_calls = visible_chunks.len() + visible_bodies.len();
        self.stats.shadow_draw_calls = self.chunks.len() + self.bodies.len();
        let shadow_timestamp_writes = self
            .gpu_profiler
            .as_ref()
            .map(|profiler| profiler.timestamps(0, 1));
        {
            let mut shadow_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("directional shadow pass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: shadow_timestamp_writes,
                ..Default::default()
            });
            shadow_pass.set_pipeline(&self.shadow_pipeline);
            shadow_pass.set_bind_group(0, &self.globals_bind_group, &[]);
            for chunk in self.chunks.values() {
                shadow_pass.set_vertex_buffer(0, chunk.vertex.slice(..));
                shadow_pass.set_index_buffer(chunk.index.slice(..), wgpu::IndexFormat::Uint32);
                shadow_pass.draw_indexed(0..chunk.index_count, 0, 0..1);
            }
            shadow_pass.set_pipeline(&self.body_shadow_pipeline);
            for body in self.bodies.values() {
                shadow_pass.set_vertex_buffer(0, body.mesh.vertex.slice(..));
                shadow_pass.set_vertex_buffer(
                    1,
                    body_instance_slice(&self.body_instance_buffer, body.instance_slot),
                );
                shadow_pass.set_index_buffer(body.mesh.index.slice(..), wgpu::IndexFormat::Uint32);
                shadow_pass.draw_indexed(0..body.mesh.index_count, 0, 0..1);
            }
        }
        let world_timestamp_writes = self
            .gpu_profiler
            .as_ref()
            .map(|profiler| profiler.timestamps(2, 3));
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world and HUD pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.075,
                            g: 0.19,
                            b: 0.39,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: world_timestamp_writes,
                ..Default::default()
            });
            pass.set_pipeline(&self.world_pipeline);
            pass.set_bind_group(0, &self.globals_bind_group, &[]);
            pass.set_bind_group(1, &self.shadow_sampling_bind_group, &[]);
            for chunk in &visible_chunks {
                pass.set_vertex_buffer(0, chunk.vertex.slice(..));
                pass.set_index_buffer(chunk.index.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..chunk.index_count, 0, 0..1);
            }
            pass.set_pipeline(&self.body_world_pipeline);
            for body in &visible_bodies {
                pass.set_vertex_buffer(0, body.mesh.vertex.slice(..));
                pass.set_vertex_buffer(
                    1,
                    body_instance_slice(&self.body_instance_buffer, body.instance_slot),
                );
                pass.set_index_buffer(body.mesh.index.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..body.mesh.index_count, 0, 0..1);
            }
            pass.set_pipeline(&self.crosshair_pipeline);
            pass.draw(0..12, 0..1);
        }
        let readback_slot = self
            .gpu_profiler
            .as_mut()
            .and_then(|profiler| profiler.encode_readback(&mut encoder));
        self.queue.submit([encoder.finish()]);
        if let (Some(profiler), Some(slot_index)) = (&mut self.gpu_profiler, readback_slot) {
            profiler.map_after_submit(slot_index);
        }
        self.queue.present(output);
        if suboptimal {
            RenderOutcome::Reconfigure
        } else {
            RenderOutcome::Presented
        }
    }
}

fn preferred_surface_format(formats: &[wgpu::TextureFormat]) -> Option<wgpu::TextureFormat> {
    formats
        .iter()
        .copied()
        .find(wgpu::TextureFormat::is_srgb)
        .or_else(|| formats.first().copied())
}

fn light_view_projection() -> Mat4 {
    let sun_direction = Vec3::new(0.35, -0.90, 0.22).normalize();
    let target = Vec3::new(0.0, 3.0, 0.0);
    let position = target - sun_direction * 150.0;
    let view = glam::camera::rh::view::look_to_mat4(position, sun_direction, Vec3::Z);
    let projection =
        glam::camera::rh::proj::directx::orthographic(-105.0, 105.0, -105.0, 105.0, 0.1, 300.0);
    projection * view
}

fn chunk_intersects_frustum(chunk: IVec3, view_projection: Mat4) -> bool {
    let edge = CHUNK_EDGE as f32;
    let minimum = Vec3::new(chunk.x as f32, chunk.y as f32, chunk.z as f32) * edge;
    let maximum = minimum + Vec3::splat(edge);
    aabb_intersects_frustum(minimum, maximum, view_projection)
}

fn aabb_intersects_frustum(minimum: Vec3, maximum: Vec3, view_projection: Mat4) -> bool {
    let corners = [
        Vec3::new(minimum.x, minimum.y, minimum.z),
        Vec3::new(maximum.x, minimum.y, minimum.z),
        Vec3::new(minimum.x, maximum.y, minimum.z),
        Vec3::new(maximum.x, maximum.y, minimum.z),
        Vec3::new(minimum.x, minimum.y, maximum.z),
        Vec3::new(maximum.x, minimum.y, maximum.z),
        Vec3::new(minimum.x, maximum.y, maximum.z),
        Vec3::new(maximum.x, maximum.y, maximum.z),
    ]
    .map(|corner| view_projection * corner.extend(1.0));

    !(corners.iter().all(|corner| corner.x < -corner.w)
        || corners.iter().all(|corner| corner.x > corner.w)
        || corners.iter().all(|corner| corner.y < -corner.w)
        || corners.iter().all(|corner| corner.y > corner.w)
        || corners.iter().all(|corner| corner.z < 0.0)
        || corners.iter().all(|corner| corner.z > corner.w))
}

fn body_instance_slice(buffer: &wgpu::Buffer, instance_slot: u32) -> wgpu::BufferSlice<'_> {
    let start = u64::from(instance_slot) * BODY_INSTANCE_BYTES;
    buffer.slice(start..start + BODY_INSTANCE_BYTES)
}

fn non_zero_size(size: PhysicalSize<u32>) -> PhysicalSize<u32> {
    PhysicalSize::new(size.width.max(1), size.height.max(1))
}

fn create_depth_view(device: &wgpu::Device, size: PhysicalSize<u32>) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("main depth texture"),
            size: wgpu::Extent3d {
                width: size.width.max(1),
                height: size.height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

fn create_shadow_map(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("directional shadow map"),
        size: wgpu::Extent3d {
            width: SHADOW_MAP_SIZE,
            height: SHADOW_MAP_SIZE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_surface_is_preferred_over_linear_fallback() {
        let formats = [
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        ];
        assert_eq!(
            preferred_surface_format(&formats),
            Some(wgpu::TextureFormat::Rgba8UnormSrgb)
        );
    }

    #[test]
    fn light_frustum_contains_representative_world_center() {
        let clip = light_view_projection() * Vec3::new(0.0, 3.0, 0.0).extend(1.0);
        let ndc = clip.truncate() / clip.w;

        assert!(ndc.x.abs() <= 1.0);
        assert!(ndc.y.abs() <= 1.0);
        assert!((0.0..=1.0).contains(&ndc.z));
    }

    #[test]
    fn gpu_timestamps_are_split_into_pass_and_total_times() {
        let sample = gpu_frame_time(&[100, 160, 175, 275], 10.0).expect("valid timestamps");

        assert_eq!(
            sample,
            GpuFrameTime {
                shadow_ms: 0.0006,
                world_hud_ms: 0.001,
                total_ms: 0.00175,
            }
        );
        assert!(gpu_frame_time(&[100, 99, 175, 275], 10.0).is_none());
        assert!(gpu_frame_time(&[100, 160, 175], 10.0).is_none());
    }

    #[test]
    fn camera_frustum_rejects_chunks_behind_the_viewer() {
        let projection = glam::camera::rh::proj::directx::perspective(
            70_f32.to_radians(),
            16.0 / 9.0,
            0.05,
            420.0,
        );
        let view = glam::camera::rh::view::look_to_mat4(Vec3::ZERO, -Vec3::Z, Vec3::Y);
        let view_projection = projection * view;

        assert!(chunk_intersects_frustum(
            IVec3::new(0, 0, -1),
            view_projection
        ));
        assert!(chunk_intersects_frustum(
            IVec3::new(-1, -1, -1),
            view_projection
        ));
        assert!(!chunk_intersects_frustum(
            IVec3::new(0, 0, 1),
            view_projection
        ));
        assert!(!chunk_intersects_frustum(
            IVec3::new(30, 0, -1),
            view_projection
        ));
    }
}
