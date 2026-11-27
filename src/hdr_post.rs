use studio_core::render_settings::PostProcess;

pub(super) const HDR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const BLOOM_DIVISOR: u32 = 4;

fn bloom_size(size: [u32; 2]) -> [u32; 2] {
    size.map(|dimension| dimension.div_ceil(BLOOM_DIVISOR))
}

fn uniforms(settings: PostProcess, encode_srgb: bool) -> [f32; 4] {
    [
        settings.exposure_ev,
        settings.bloom_threshold,
        if settings.bloom { 0.22 } else { 0.0 },
        f32::from(u8::from(encode_srgb)),
    ]
}

pub(super) struct PostRenderer {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    bright: wgpu::RenderPipeline,
    blur_horizontal: wgpu::RenderPipeline,
    blur_vertical: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    samples: u32,
    encode_srgb: bool,
    targets: Option<Targets>,
}

struct Targets {
    size: [u32; 2],
    scene: wgpu::TextureView,
    multisampled: Option<wgpu::TextureView>,
    depth: wgpu::TextureView,
    bloom: Option<BloomTargets>,
    composite: wgpu::BindGroup,
}

struct BloomTargets {
    ping: wgpu::TextureView,
    pong: wgpu::TextureView,
    bright_input: wgpu::BindGroup,
    horizontal_input: wgpu::BindGroup,
    vertical_input: wgpu::BindGroup,
}

impl PostRenderer {
    pub(super) fn new(device: &wgpu::Device, output: wgpu::TextureFormat, samples: u16) -> Self {
        use wgpu::{BindingType, ShaderStages};
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::FRAGMENT,
            ty,
            count: None,
        };
        let texture_binding = BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("HDR postprocess bindings"),
            entries: &[
                entry(0, texture_binding),
                entry(1, BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
                entry(
                    2,
                    BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(16),
                    },
                ),
                entry(3, texture_binding),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("HDR postprocess pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("HDR postprocess WGSL"),
            source: wgpu::ShaderSource::Wgsl(include_str!("hdr_post.wgsl").into()),
        });
        let create = |fragment, format: wgpu::TextureFormat, count, depth_stencil| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fragment),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_fullscreen"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    targets: &[Some(format.into())],
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                multisample: wgpu::MultisampleState {
                    count,
                    ..Default::default()
                },
                depth_stencil,
                multiview: None,
                cache: None,
            })
        };
        let samples = u32::from(samples);
        let bright = create("fs_bright", HDR, 1, None);
        let blur_horizontal = create("fs_blur_x", HDR, 1, None);
        let blur_vertical = create("fs_blur_y", HDR, 1, None);
        let composite = create(
            "fs_composite",
            output,
            samples,
            Some(wgpu::DepthStencilState {
                format: super::DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: Default::default(),
                bias: Default::default(),
            }),
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("HDR clamp-to-edge sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("HDR exposure and bloom"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            layout,
            sampler,
            uniform,
            bright,
            blur_horizontal,
            blur_vertical,
            composite,
            samples,
            encode_srgb: !output.is_srgb(),
            targets: None,
        }
    }

    pub(super) fn configure(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: [u32; 2],
        settings: PostProcess,
    ) {
        let data = uniforms(settings, self.encode_srgb);
        queue.write_buffer(&self.uniform, 0, bytemuck::cast_slice(&data));
        if self.targets.as_ref().is_some_and(|targets| {
            targets.size == size && (targets.bloom.is_some() || !settings.bloom)
        }) {
            return;
        }
        let scene = target(device, "HDR scene color", size, HDR, 1);
        let multisampled =
            (self.samples > 1).then(|| target(device, "HDR scene MSAA", size, HDR, self.samples));
        let depth = target(
            device,
            "HDR scene depth",
            size,
            super::DEPTH_FORMAT,
            self.samples,
        );
        let bloom = settings.bloom.then(|| {
            let size = bloom_size(size);
            let ping = target(device, "Bloom ping", size, HDR, 1);
            let pong = target(device, "Bloom pong", size, HDR, 1);
            let bright_input = self.bind(device, &scene, &scene);
            let horizontal_input = self.bind(device, &ping, &scene);
            let vertical_input = self.bind(device, &pong, &scene);
            BloomTargets {
                ping,
                pong,
                bright_input,
                horizontal_input,
                vertical_input,
            }
        });
        let composite = self.bind(
            device,
            &scene,
            bloom.as_ref().map_or(&scene, |bloom| &bloom.ping),
        );
        self.targets = Some(Targets {
            size,
            scene,
            multisampled,
            depth,
            bloom,
            composite,
        });
    }

    fn bind(
        &self,
        device: &wgpu::Device,
        source: &wgpu::TextureView,
        bloom: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        use wgpu::BindingResource;
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("HDR postprocess inputs"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::TextureView(bloom),
                },
            ],
        })
    }

    pub(super) fn scene_pass<'a>(
        &self,
        encoder: &'a mut wgpu::CommandEncoder,
    ) -> Option<wgpu::RenderPass<'a>> {
        let targets = self.targets.as_ref()?;
        Some(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Linear HDR viewport"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: targets.multisampled.as_ref().unwrap_or(&targets.scene),
                resolve_target: targets.multisampled.as_ref().map(|_| &targets.scene),
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: if targets.multisampled.is_some() {
                        wgpu::StoreOp::Discard
                    } else {
                        wgpu::StoreOp::Store
                    },
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &targets.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        }))
    }

    pub(super) fn bloom(&self, encoder: &mut wgpu::CommandEncoder) {
        let Some(targets) = self
            .targets
            .as_ref()
            .and_then(|targets| targets.bloom.as_ref())
        else {
            return;
        };
        for (output, pipeline, input) in [
            (&targets.ping, &self.bright, &targets.bright_input),
            (
                &targets.pong,
                &self.blur_horizontal,
                &targets.horizontal_input,
            ),
            (&targets.ping, &self.blur_vertical, &targets.vertical_input),
        ] {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Bloom bright-pass or blur"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: output,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, input, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    pub(super) fn composite(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some(targets) = &self.targets {
            pass.set_pipeline(&self.composite);
            pass.set_bind_group(0, &targets.composite, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

fn target(
    device: &wgpu::Device,
    label: &str,
    size: [u32; 2],
    format: wgpu::TextureFormat,
    sample_count: u32,
) -> wgpu::TextureView {
    let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
    if sample_count == 1 && format == HDR {
        usage |= wgpu::TextureUsages::TEXTURE_BINDING;
    }
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a3_bloom_buffers_cover_odd_rectangles_and_tiny_previews() {
        assert_eq!(bloom_size([2560, 1440]), [640, 360]);
        assert_eq!(bloom_size([1025, 515]), [257, 129]);
        assert_eq!(bloom_size([1, 2]), [1, 1]);
    }

    #[test]
    fn a3_legacy_scene_defaults_to_no_bloom_and_zero_ev() {
        assert_eq!(
            uniforms(PostProcess::default(), false),
            [0.0, 1.0, 0.0, 0.0]
        );
    }

    #[test]
    fn a3_uniforms_send_hdr_parameters_and_srgb_conversion_separately() {
        let settings = PostProcess {
            exposure_ev: 1.5,
            bloom_threshold: 3.5,
            bloom: true,
            ..PostProcess::default()
        };
        assert_eq!(uniforms(settings, true), [1.5, 3.5, 0.22, 1.0]);
        assert_eq!(uniforms(settings, false), [1.5, 3.5, 0.22, 0.0]);
    }
}
