//! Sixteen fixed-size, world-space depth views for dynamic distant-environment visibility.
//! Geometry/pose edits invalidate a cache; no voxel transaction or network field is changed.

use super::{
    BODY_INSTANCE_BYTES, GpuBody, GpuMesh, GpuProfiler, Vertex, aabb_intersects_frustum,
    body_instance_slice, chunk_intersects_frustum,
};
use crate::{BodyId, IVec3};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use std::collections::HashMap;
use wgpu::util::DeviceExt;

pub(super) const VIEWS: usize = 16;
pub(super) const EDGE: u32 = 1_024;
pub(super) const DEPTH_BYTES: usize = VIEWS * 1_024 * 1_024 * 4;
const RADIUS: f32 = 64.0;
const DEPTH: f32 = 256.0;
const ANCHOR_STEP: f32 = 16.0;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Pod, Zeroable)]
pub(super) struct Parameters {
    pub matrices: [[[f32; 4]; 4]; VIEWS],
    pub directions: [[f32; 4]; VIEWS],
    pub focus: [f32; 4],
    pub settings: [f32; 4],
}

const _: () = assert!(size_of::<Parameters>() == 1_312);

impl Parameters {
    fn at(camera: Vec3) -> Self {
        // The camera may move freely within a cached 16m cell; the 64m-radius maps retain
        // a full near-field margin. Snap in light texels as well to avoid subtexel swimming.
        let focus = (camera / ANCHOR_STEP).round() * ANCHOR_STEP;
        let directions = std::array::from_fn(|index| {
            // Paired stratified sphere, including low-elevation window light that equal
            // octants miss. Antipodal pairs retain symmetric coverage for every normal.
            let sample = f32::from(u8::try_from(index % 8).unwrap_or(0));
            let y = (sample + 0.5) / 8.0;
            let radius = (1.0 - y * y).sqrt();
            let azimuth = sample * 2.399_963_1;
            let direction = Vec3::new(radius * azimuth.cos(), y, radius * azimuth.sin());
            (direction * if index < 8 { 1.0 } else { -1.0 })
                .extend(0.0)
                .to_array()
        });
        let matrices = directions.map(|direction| {
            let direction = Vec3::from_array([direction[0], direction[1], direction[2]]);
            let base = glam::camera::rh::view::look_to_mat4(
                direction * (DEPTH * 0.5),
                -direction,
                Vec3::Y,
            );
            let light_focus = base.transform_point3(focus);
            let texel = 2.0 * RADIUS / 1_024.0;
            let offset = Vec3::new(
                -(light_focus.x / texel).round() * texel,
                -(light_focus.y / texel).round() * texel,
                (-DEPTH).mul_add(0.5, -light_focus.z),
            );
            let projection = glam::camera::rh::proj::directx::orthographic(
                -RADIUS, RADIUS, -RADIUS, RADIUS, 0.1, DEPTH,
            );
            (projection * Mat4::from_translation(offset) * base).to_cols_array_2d()
        });
        Self {
            matrices,
            directions,
            // Coverage follows the camera continuously, independently of the cached anchor.
            // 48m + sqrt(3)*8m anchor drift stays inside the 64m maps with a PCF margin.
            focus: camera.extend(32.0).to_array(),
            settings: [48.0, 0.06, 0.03 / DEPTH, 0.0],
        }
    }
}

#[derive(Default, Clone, Copy, Debug)]
pub(super) struct VisibilityStats {
    pub rebuilds: u64,
    pub cache_hits: u64,
    pub draw_calls: usize,
    pub refreshed: bool,
}

pub(super) struct SkyVisibility {
    pub view: wgpu::TextureView,
    pub buffer: wgpu::Buffer,
    layers: [wgpu::TextureView; VIEWS],
    pass_buffers: [wgpu::Buffer; VIEWS],
    pass_groups: [wgpu::BindGroup; VIEWS],
    static_pipeline: wgpu::RenderPipeline,
    instance_pipeline: wgpu::RenderPipeline,
    parameters: Parameters,
    dirty: bool,
    pub stats: VisibilityStats,
}

impl SkyVisibility {
    pub fn new(device: &wgpu::Device) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bounded sky visibility depth array"),
            size: wgpu::Extent3d {
                width: EDGE,
                height: EDGE,
                depth_or_array_layers: 16,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let layers = std::array::from_fn(|index| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: u32::try_from(index).unwrap_or(0),
                array_layer_count: Some(1),
                ..Default::default()
            })
        });
        let parameters = Parameters::at(Vec3::ZERO);
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("sky visibility receiver parameters"),
            contents: bytemuck::bytes_of(&parameters),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky depth matrix layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(64),
                },
                count: None,
            }],
        });
        let pass_buffers = parameters.matrices.map(|matrix| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("sky depth view matrix"),
                contents: bytemuck::bytes_of(&matrix),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            })
        });
        let pass_groups = std::array::from_fn(|index| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sky depth matrix"),
                layout: &layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: pass_buffers[index].as_entire_binding(),
                }],
            })
        });
        let (static_pipeline, instance_pipeline) = pipelines(device, &layout);
        Self {
            view,
            buffer,
            layers,
            pass_buffers,
            pass_groups,
            static_pipeline,
            instance_pipeline,
            parameters,
            dirty: true,
            stats: VisibilityStats::default(),
        }
    }

    pub const fn invalidate(&mut self) {
        self.dirty = true;
    }

    fn prepare(&mut self, queue: &wgpu::Queue, camera: Vec3) -> bool {
        let parameters = Parameters::at(camera);
        self.stats.refreshed = self.dirty || parameters.matrices != self.parameters.matrices;
        self.stats.draw_calls = 0;
        if parameters != self.parameters {
            queue.write_buffer(&self.buffer, 0, bytemuck::bytes_of(&parameters));
        }
        self.parameters = parameters;
        if !self.stats.refreshed {
            self.stats.cache_hits = self.stats.cache_hits.saturating_add(1);
            return false;
        }
        for (buffer, matrix) in self.pass_buffers.iter().zip(&parameters.matrices) {
            queue.write_buffer(buffer, 0, bytemuck::bytes_of(matrix));
        }
        true
    }

    #[allow(clippy::too_many_arguments)]
    pub fn encode(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        queue: &wgpu::Queue,
        camera: Vec3,
        chunks: &HashMap<IVec3, GpuMesh>,
        bodies: &HashMap<BodyId, GpuBody>,
        body_buffer: &wgpu::Buffer,
        player_mesh: &GpuMesh,
        player_buffer: &wgpu::Buffer,
        player_count: usize,
        profiler: Option<&GpuProfiler>,
    ) {
        if !self.prepare(queue, camera) {
            if let Some(profiler) = profiler {
                // Empty compute pass writes current timestamps without touching cached depth.
                let _pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("sky visibility cache hit"),
                    timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                        query_set: &profiler.query_set,
                        beginning_of_pass_write_index: Some(2),
                        end_of_pass_write_index: Some(3),
                    }),
                });
            }
            return;
        }
        for index in 0..VIEWS {
            let matrix = Mat4::from_cols_array_2d(&self.parameters.matrices[index]);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("directional sky visibility depth"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.layers[index],
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: profiler.filter(|_| index == 0 || index == VIEWS - 1).map(
                    |profiler| wgpu::RenderPassTimestampWrites {
                        query_set: &profiler.query_set,
                        beginning_of_pass_write_index: (index == 0).then_some(2),
                        end_of_pass_write_index: (index == VIEWS - 1).then_some(3),
                    },
                ),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.pass_groups[index], &[]);
            pass.set_pipeline(&self.static_pipeline);
            for (position, mesh) in chunks {
                if chunk_intersects_frustum(*position, matrix) {
                    draw_mesh(&mut pass, mesh);
                    self.stats.draw_calls += 1;
                }
            }
            pass.set_pipeline(&self.instance_pipeline);
            for body in bodies.values() {
                if aabb_intersects_frustum(body.world_minimum, body.world_maximum, matrix) {
                    pass.set_vertex_buffer(1, body_instance_slice(body_buffer, body.instance_slot));
                    draw_mesh(&mut pass, &body.mesh);
                    self.stats.draw_calls += 1;
                }
            }
            if player_count > 0 {
                pass.set_vertex_buffer(0, player_mesh.vertex.slice(..));
                pass.set_vertex_buffer(1, player_buffer.slice(..));
                pass.set_index_buffer(player_mesh.index.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(
                    0..player_mesh.index_count,
                    0,
                    0..u32::try_from(player_count).unwrap_or(0),
                );
                self.stats.draw_calls += 1;
            }
        }
        self.dirty = false;
        self.stats.rebuilds = self.stats.rebuilds.saturating_add(1);
    }
}

fn draw_mesh(pass: &mut wgpu::RenderPass<'_>, mesh: &GpuMesh) {
    pass.set_vertex_buffer(0, mesh.vertex.slice(..));
    pass.set_index_buffer(mesh.index.slice(..), wgpu::IndexFormat::Uint32);
    pass.draw_indexed(0..mesh.index_count, 0, 0..1);
}

fn pipelines(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
) -> (wgpu::RenderPipeline, wgpu::RenderPipeline) {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("sky visibility depth shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/sky_depth.wgsl").into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("sky visibility depth layout"),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    let positions = wgpu::vertex_attr_array![0 => Float32x3];
    let instances =
        wgpu::vertex_attr_array![6 => Float32x4, 7 => Float32x4, 8 => Float32x4, 9 => Float32x4];
    let buffers = [
        Some(wgpu::VertexBufferLayout {
            array_stride: size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &positions,
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: BODY_INSTANCE_BYTES,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &instances,
        }),
    ];
    let pipeline = |entry, buffers: &[Option<wgpu::VertexBufferLayout<'_>>]| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(entry),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some(entry),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers,
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: 1,
                    slope_scale: 1.0,
                    clamp: 0.0,
                },
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        })
    };
    (
        pipeline("static_depth", &buffers[..1]),
        pipeline("instance_depth", &buffers),
    )
}

#[cfg(test)]
mod gpu_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finite_directional_matrices_cover_the_near_field_and_snap_camera_motion() {
        let plan = Parameters::at(Vec3::ZERO);
        for (matrix, direction) in plan.matrices.into_iter().zip(plan.directions) {
            let matrix = Mat4::from_cols_array_2d(&matrix);
            assert!(matrix.is_finite());
            assert!((Vec3::from_slice(&direction).length() - 1.0).abs() < 1e-6);
            for point in [Vec3::ZERO, Vec3::X * 40.0, Vec3::Y * 40.0, Vec3::Z * 40.0] {
                let clip = matrix.transform_point3(point);
                assert!(clip.x.abs() < 1.0 && clip.y.abs() < 1.0 && (0.0..1.0).contains(&clip.z));
            }
        }
        assert_eq!(Parameters::at(Vec3::splat(7.9)).matrices, plan.matrices);
        assert_ne!(
            Parameters::at(Vec3::new(8.1, 0.0, 0.0)).matrices,
            plan.matrices
        );
        assert!(48.0 + Vec3::splat(8.0).length() < RADIUS - 0.25);
        assert_eq!(DEPTH_BYTES, 67_108_864);
        assert_eq!(size_of::<super::super::BodyInstance>(), 64);
    }
}
