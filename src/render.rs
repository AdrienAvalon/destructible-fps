//! Safe `wgpu` presentation layer for the playable Linux slice.

#![allow(clippy::cast_precision_loss)]

use crate::{
    IVec3, World,
    mesh::{CpuMesh, Vertex, mesh_chunk},
};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use std::{collections::HashMap, sync::Arc};
use wgpu::util::DeviceExt;
use winit::{dpi::PhysicalSize, window::Window};

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SHADOW_MAP_SIZE: u32 = 2_048;
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

struct GpuChunk {
    vertex: wgpu::Buffer,
    index: wgpu::Buffer,
    index_count: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats {
    pub chunks: usize,
    pub exposed_faces: usize,
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
    world_pipeline: wgpu::RenderPipeline,
    crosshair_pipeline: wgpu::RenderPipeline,
    globals_buffer: wgpu::Buffer,
    globals_bind_group: wgpu::BindGroup,
    shadow_sampling_bind_group: wgpu::BindGroup,
    chunks: HashMap<IVec3, GpuChunk>,
    stats: RenderStats,
}

impl Renderer {
    /// Initializes the Vulkan device and uploads the initial chunk meshes.
    ///
    /// # Errors
    ///
    /// Returns a diagnostic when the window surface or a compatible GPU is unavailable.
    #[allow(clippy::too_many_lines)]
    pub async fn new(window: Arc<Window>, world: &World) -> Result<Self, String> {
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
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("destructible-fps device"),
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
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("directional shadow pipeline"),
            layout: Some(&globals_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("shadow_vertex"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(vertex_layout)],
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
        let mut renderer = Self {
            instance,
            window,
            surface,
            device,
            queue,
            depth_view,
            _shadow_texture: shadow_texture,
            shadow_view,
            shadow_pipeline,
            config,
            world_pipeline,
            crosshair_pipeline,
            globals_buffer,
            globals_bind_group,
            shadow_sampling_bind_group,
            chunks: HashMap::new(),
            stats: RenderStats::default(),
        };
        renderer.rebuild_all(world);
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

    pub fn rebuild_all(&mut self, world: &World) {
        self.chunks.clear();
        for chunk in world.chunk_positions() {
            self.upload_chunk(world, chunk);
        }
        self.refresh_stats();
    }

    pub fn rebuild_chunks(&mut self, world: &World, chunks: &[IVec3]) {
        for &chunk in chunks {
            self.upload_chunk(world, chunk);
        }
        self.refresh_stats();
    }

    pub fn upload_chunk_meshes(&mut self, meshes: Vec<(IVec3, CpuMesh)>) {
        for (chunk, mesh) in meshes {
            self.upload_mesh(chunk, &mesh);
        }
        self.refresh_stats();
    }

    fn upload_chunk(&mut self, world: &World, chunk: IVec3) {
        self.upload_mesh(chunk, &mesh_chunk(world, chunk));
    }

    fn upload_mesh(&mut self, chunk: IVec3, mesh: &CpuMesh) {
        if mesh.indices.is_empty() {
            self.chunks.remove(&chunk);
            return;
        }
        let index_count = u32::try_from(mesh.indices.len()).expect("chunk index count fits in u32");
        let vertex = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("chunk vertices"),
                contents: bytemuck::cast_slice(&mesh.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let index = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("chunk indices"),
                contents: bytemuck::cast_slice(&mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        self.chunks.insert(
            chunk,
            GpuChunk {
                vertex,
                index,
                index_count,
            },
        );
    }

    fn refresh_stats(&mut self) {
        self.stats = RenderStats {
            chunks: self.chunks.len(),
            exposed_faces: self
                .chunks
                .values()
                .map(|chunk| chunk.index_count as usize / 6)
                .sum(),
        };
    }

    #[must_use]
    pub const fn stats(&self) -> RenderStats {
        self.stats
    }

    #[allow(clippy::too_many_lines)]
    pub fn render(
        &mut self,
        camera_position: Vec3,
        view_direction: Vec3,
        elapsed_seconds: f32,
    ) -> RenderOutcome {
        let aspect = self.config.width as f32 / self.config.height as f32;
        let projection =
            glam::camera::rh::proj::directx::perspective(70_f32.to_radians(), aspect, 0.05, 420.0);
        let view = glam::camera::rh::view::look_to_mat4(camera_position, view_direction, Vec3::Y);
        let light_view_projection = light_view_projection();
        let globals = Globals {
            view_projection: (projection * view).to_cols_array_2d(),
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
                ..Default::default()
            });
            shadow_pass.set_pipeline(&self.shadow_pipeline);
            shadow_pass.set_bind_group(0, &self.globals_bind_group, &[]);
            for chunk in self.chunks.values() {
                shadow_pass.set_vertex_buffer(0, chunk.vertex.slice(..));
                shadow_pass.set_index_buffer(chunk.index.slice(..), wgpu::IndexFormat::Uint32);
                shadow_pass.draw_indexed(0..chunk.index_count, 0, 0..1);
            }
        }
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
                ..Default::default()
            });
            pass.set_pipeline(&self.world_pipeline);
            pass.set_bind_group(0, &self.globals_bind_group, &[]);
            pass.set_bind_group(1, &self.shadow_sampling_bind_group, &[]);
            for chunk in self.chunks.values() {
                pass.set_vertex_buffer(0, chunk.vertex.slice(..));
                pass.set_index_buffer(chunk.index.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..chunk.index_count, 0, 0..1);
            }
            pass.set_pipeline(&self.crosshair_pipeline);
            pass.draw(0..12, 0..1);
        }
        self.queue.submit([encoder.finish()]);
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
}
