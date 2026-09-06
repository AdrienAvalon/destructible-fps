//! Bounded linear HDR frame targets; spatial resolve precedes the display transform.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

pub(super) const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const MAX_PIXELS: u64 = 8_388_608;
const _: () = assert!(MAX_PIXELS * 56 < 512 * 1024 * 1024);
const SHADER: &str = concat!(
    include_str!("../shaders/color.wgsl"),
    include_str!("../shaders/display.wgsl")
);

pub(super) fn validate_formats(
    samples: u32,
    color: wgpu::TextureFormatFeatures,
    depth: wgpu::TextureFormatFeatures,
) -> Result<(), String> {
    let color_usage = wgpu::TextureUsages::RENDER_ATTACHMENT
        | wgpu::TextureUsages::TEXTURE_BINDING
        | wgpu::TextureUsages::COPY_SRC;
    if !matches!(samples, 1 | 4)
        || !color.allowed_usages.contains(color_usage)
        || !depth
            .allowed_usages
            .contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
        || !color.flags.sample_count_supported(samples)
        || !depth.flags.sample_count_supported(samples)
        || (samples > 1
            && !color
                .flags
                .contains(wgpu::TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE))
    {
        return Err(format!(
            "combinaison HDR/depth MSAA {samples}x non prise en charge"
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FramePlan {
    pub width: u32,
    pub height: u32,
    pub samples: u32,
    pub texel_bytes: u64,
}

impl FramePlan {
    pub fn new(width: u32, height: u32, samples: u32, max_dimension: u32) -> Result<Self, String> {
        if !matches!(samples, 1 | 4) {
            return Err("MSAA exige exactement 1 ou 4 echantillons".to_owned());
        }
        if width == 0 || height == 0 || width > max_dimension || height > max_dimension {
            return Err("dimensions HDR nulles ou hors limite du GPU".to_owned());
        }
        let pixels = u64::from(width) * u64::from(height);
        if pixels > MAX_PIXELS {
            return Err(format!(
                "cible HDR trop grande: {pixels} pixels, maximum {MAX_PIXELS}"
            ));
        }
        // 1x: color8 + depth4. 4x: resolved8 + color4*8 + depth4*4.
        let texel_bytes = pixels * if samples == 1 { 12 } else { 56 };
        Ok(Self {
            width,
            height,
            samples,
            texel_bytes,
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ResizeAction {
    Ignore,
    Restore(String),
    Defer,
    Reuse,
    Replace(FramePlan),
}

pub(super) fn resize_action(
    current: FramePlan,
    width: u32,
    height: u32,
    max_dimension: u32,
    queue_idle: bool,
) -> ResizeAction {
    if width == 0 || height == 0 {
        return ResizeAction::Ignore;
    }
    let plan = match FramePlan::new(width, height, current.samples, max_dimension) {
        Ok(plan) => plan,
        Err(error) => return ResizeAction::Restore(error),
    };
    if !queue_idle {
        ResizeAction::Defer
    } else if plan == current {
        ResizeAction::Reuse
    } else {
        ResizeAction::Replace(plan)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Parameters {
    exposure: f32,
    manual_srgb: f32,
    hud: f32,
    padding: f32,
}

pub(super) struct DisplayPass {
    pub plan: FramePlan,
    #[cfg(test)]
    pub hdr: wgpu::Texture,
    pub hdr_view: wgpu::TextureView,
    pub depth_view: wgpu::TextureView,
    multisample_view: Option<wgpu::TextureView>,
    pub pipeline: wgpu::RenderPipeline,
    pub bind_group: wgpu::BindGroup,
}

impl DisplayPass {
    pub fn new(device: &wgpu::Device, plan: FramePlan, output: wgpu::TextureFormat) -> Self {
        let extent = wgpu::Extent3d {
            width: plan.width,
            height: plan.height,
            depth_or_array_layers: 1,
        };
        let texture = |label, format, samples, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: extent,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let hdr = texture(
            "linear resolved HDR",
            HDR_FORMAT,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        );
        let hdr_view = hdr.create_view(&wgpu::TextureViewDescriptor::default());
        let multisample_view = (plan.samples > 1).then(|| {
            texture(
                "linear multisample HDR",
                HDR_FORMAT,
                plan.samples,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            )
            .create_view(&wgpu::TextureViewDescriptor::default())
        });
        let depth_view = texture(
            "matching HDR scene depth",
            super::DEPTH_FORMAT,
            plan.samples,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        )
        .create_view(&wgpu::TextureViewDescriptor::default());
        let parameters = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("fixed display exposure and transfer"),
            contents: bytemuck::bytes_of(&Parameters {
                exposure: super::SCENE_EXPOSURE,
                manual_srgb: f32::from(!output.is_srgb()),
                hud: 1.0,
                padding: 0.0,
            }),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let pipeline = display_pipeline(device, output);
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("HDR display input"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&hdr_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: parameters.as_entire_binding(),
                },
            ],
        });
        Self {
            plan,
            #[cfg(test)]
            hdr,
            hdr_view,
            depth_view,
            multisample_view,
            pipeline,
            bind_group,
        }
    }

    pub fn color_attachment(&self) -> wgpu::RenderPassColorAttachment<'_> {
        wgpu::RenderPassColorAttachment {
            view: self.multisample_view.as_ref().unwrap_or(&self.hdr_view),
            depth_slice: None,
            resolve_target: self.multisample_view.as_ref().map(|_| &self.hdr_view),
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: if self.plan.samples > 1 {
                    wgpu::StoreOp::Discard
                } else {
                    wgpu::StoreOp::Store
                },
            },
        }
    }
}

fn display_pipeline(device: &wgpu::Device, output: wgpu::TextureFormat) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("linear HDR display shader"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("HDR to display"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("fullscreen"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("display"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: output,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(test)]
mod gpu_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_allocations_have_checked_dimensions_samples_and_explicit_texel_cost() {
        assert_eq!(
            FramePlan::new(1440, 900, 4, 8192).unwrap().texel_bytes,
            72_576_000
        );
        assert_eq!(
            FramePlan::new(1440, 900, 1, 8192).unwrap().texel_bytes,
            15_552_000
        );
        assert_eq!(
            FramePlan::new(4096, 2048, 4, 8192).unwrap().texel_bytes,
            MAX_PIXELS * 56
        );
        for (w, h, samples, limit) in [
            (0, 1, 4, 8192),
            (1, 0, 4, 8192),
            (4096, 2049, 4, 8192),
            (8193, 1, 1, 8192),
            (u32::MAX, u32::MAX, 4, u32::MAX),
            (1, 1, 0, 8192),
            (1, 1, 2, 8192),
            (1, 1, 8, 8192),
        ] {
            assert!(FramePlan::new(w, h, samples, limit).is_err());
        }
    }

    #[test]
    fn formats_must_support_both_targets_and_a_real_hdr_resolve() {
        let color = HDR_FORMAT.guaranteed_format_features(wgpu::Features::empty());
        let depth = super::super::DEPTH_FORMAT.guaranteed_format_features(wgpu::Features::empty());
        assert!(validate_formats(1, color, depth).is_ok());
        assert!(validate_formats(4, color, depth).is_ok());
        let mut no_four = depth;
        no_four
            .flags
            .remove(wgpu::TextureFormatFeatureFlags::MULTISAMPLE_X4);
        assert!(validate_formats(4, color, no_four).is_err());
        assert!(validate_formats(1, color, no_four).is_ok());
        let mut no_resolve = color;
        no_resolve
            .flags
            .remove(wgpu::TextureFormatFeatureFlags::MULTISAMPLE_RESOLVE);
        assert!(validate_formats(4, no_resolve, depth).is_err());
        let mut no_sample = color;
        no_sample
            .allowed_usages
            .remove(wgpu::TextureUsages::TEXTURE_BINDING);
        assert!(validate_formats(1, no_sample, depth).is_err());
    }

    #[test]
    fn resize_rejects_without_replacing_and_defers_until_old_work_is_idle() {
        let mut current = FramePlan::new(1440, 900, 4, 8192).unwrap();
        let old = current;
        assert_eq!(
            resize_action(current, 0, 900, 8192, false),
            ResizeAction::Ignore
        );
        assert_eq!(
            resize_action(current, 1440, 900, 8192, false),
            ResizeAction::Defer
        );
        assert_eq!(
            resize_action(current, 1440, 900, 8192, true),
            ResizeAction::Reuse
        );
        assert!(matches!(
            resize_action(current, 5120, 2880, 8192, false),
            ResizeAction::Restore(_)
        ));
        assert_eq!(current, old);
        assert_eq!(
            resize_action(current, 900, 600, 8192, false),
            ResizeAction::Defer
        );
        let ResizeAction::Replace(plan) = resize_action(current, 900, 600, 8192, true) else {
            panic!("valid replacement expected only after idle");
        };
        current = plan;
        assert_eq!(current.texel_bytes, 900 * 600 * 56);
        assert_eq!(
            resize_action(current, 900, 600, 8192, true),
            ResizeAction::Reuse
        );
        assert!(matches!(
            resize_action(current, u32::MAX, 1, 8192, true),
            ResizeAction::Restore(_)
        ));
        assert_eq!(current, plan);
    }
}
