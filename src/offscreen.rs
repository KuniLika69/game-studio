use std::{
    future::Future,
    sync::{Arc, mpsc},
    task::{Context, Poll, Wake, Waker},
    thread,
    time::{Duration, Instant},
};

use eframe::{egui, egui_wgpu};

use crate::viewport3d;

const COLOR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const GPU_TIMEOUT: Duration = Duration::from_secs(20);
const TIMESTAMP_COUNT: u32 = 7;
const TIMESTAMP_BYTES: u64 = TIMESTAMP_COUNT as u64 * 8;

#[derive(Clone, Copy)]
pub(crate) struct GpuFrameTimes {
    pub(crate) geometry_ms: f32,
    pub(crate) bloom_ms: f32,
    pub(crate) composite_ms: f32,
    pub(crate) shadow_ms: f32,
}

struct GpuTiming {
    queries: wgpu::QuerySet,
    resolved: wgpu::Buffer,
    readback: wgpu::Buffer,
    frames: Vec<GpuFrameTimes>,
}

pub struct Offscreen {
    context: egui::Context,
    last_output: Option<egui::FullOutput>,
    adapter_info: wgpu::AdapterInfo,
    frame_timings: Option<Vec<f32>>,
    gpu_timing: Option<GpuTiming>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: egui_wgpu::Renderer,
    size: [u32; 2],
    extent: wgpu::Extent3d,
    color: wgpu::Texture,
    color_view: wgpu::TextureView,
    multisampled: Option<wgpu::TextureView>,
    depth: wgpu::TextureView,
    readback: wgpu::Buffer,
    padded_row: u32,
}

impl Offscreen {
    pub fn new(size: [u32; 2], samples: u16) -> Result<Self, String> {
        Self::create(size, samples, false)
    }

    pub(crate) fn new_with_gpu_timing(size: [u32; 2], samples: u16) -> Result<Self, String> {
        Self::create(size, samples, true)
    }

    fn create([width, height]: [u32; 2], samples: u16, measure_gpu: bool) -> Result<Self, String> {
        if width == 0 || height == 0 {
            return Err("Размер кадра должен быть больше нуля.".into());
        }
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12 | wgpu::Backends::VULKAN,
            ..Default::default()
        });
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            ..Default::default()
        }))
        .map_err(|error| format!("Видеоустройство для фонового рендера недоступно: {error}"))?;
        let sample_count = u32::from(samples);
        if ![COLOR, viewport3d::DEPTH_FORMAT].into_iter().all(|format| {
            adapter
                .get_texture_format_features(format)
                .flags
                .sample_count_supported(sample_count)
        }) {
            return Err(format!("Фоновый рендер не поддерживает {samples}× MSAA."));
        }
        let required_features = if measure_gpu {
            wgpu::Features::TIMESTAMP_QUERY
                | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS
                | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES
        } else {
            wgpu::Features::empty()
        };
        if !adapter.features().contains(required_features) {
            return Err("Видеоустройство не поддерживает замер HDR-проходов.".into());
        }
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Studio offscreen device"),
            required_features,
            ..Default::default()
        }))
        .map_err(|error| format!("Не удалось запустить фоновый рендер: {error}"))?;
        let limits = device.limits();
        if width.max(height) > limits.max_texture_dimension_2d {
            return Err(format!(
                "Размер кадра превышает {} пикселей по одной из сторон.",
                limits.max_texture_dimension_2d
            ));
        }
        let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_row = (width * 4).div_ceil(alignment) * alignment;
        let byte_count = u64::from(padded_row) * u64::from(height);
        if byte_count > limits.max_buffer_size {
            return Err("Кадр превышает допустимый размер буфера видеоустройства.".into());
        }
        let extent = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = |label, format, usage, sample_count| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: extent,
                mip_level_count: 1,
                sample_count,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let color = texture(
            "Studio offscreen color",
            COLOR,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            1,
        );
        let color_view = color.create_view(&Default::default());
        let multisampled = (sample_count > 1).then(|| {
            texture(
                "Studio offscreen MSAA",
                COLOR,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
                sample_count,
            )
            .create_view(&Default::default())
        });
        let depth = texture(
            "Studio offscreen depth",
            viewport3d::DEPTH_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
            sample_count,
        )
        .create_view(&Default::default());
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Studio offscreen readback"),
            size: byte_count,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut renderer = egui_wgpu::Renderer::new(
            &device,
            COLOR,
            egui_wgpu::RendererOptions {
                msaa_samples: sample_count,
                depth_stencil_format: Some(viewport3d::DEPTH_FORMAT),
                ..Default::default()
            },
        );
        viewport3d::install_renderer(&mut renderer, &device, &queue, COLOR, samples);
        let gpu_timing = measure_gpu.then(|| {
            let queries = device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("HDR pass timings"), ty: wgpu::QueryType::Timestamp, count: TIMESTAMP_COUNT,
            });
            renderer.callback_resources.insert(viewport3d::GpuTimingQueries(queries.clone()));
            let make_buffer = |label, usage| device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label), size: TIMESTAMP_BYTES, usage, mapped_at_creation: false,
            });
            GpuTiming {
                queries,
                resolved: make_buffer("Resolve HDR timings", wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC),
                readback: make_buffer("Read HDR timings", wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST),
                frames: Vec::new(),
            }
        });
        let context = egui::Context::default();
        context.options_mut(|options| options.max_passes = std::num::NonZeroUsize::MIN);
        Ok(Self {
            context,
            last_output: None,
            adapter_info: adapter.get_info(),
            frame_timings: None,
            gpu_timing,
            device,
            queue,
            renderer,
            size: [width, height],
            extent,
            color,
            color_view,
            multisampled,
            depth,
            readback,
            padded_row,
        })
    }

    pub fn context(&self) -> &egui::Context {
        &self.context
    }

    pub fn last_output(&self) -> Option<&egui::FullOutput> {
        self.last_output.as_ref()
    }

    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.adapter_info
    }

    pub fn start_timing(&mut self) {
        self.frame_timings = Some(Vec::new());
    }

    /// Stops recording and returns elapsed milliseconds for each completed frame.
    pub fn stop_timing(&mut self) -> Vec<f32> {
        self.frame_timings.take().unwrap_or_default()
    }

    pub(crate) fn take_gpu_frame_times(&mut self) -> Vec<GpuFrameTimes> {
        self.gpu_timing.as_mut().map_or_else(Vec::new, |timing| std::mem::take(&mut timing.frames))
    }

    pub(crate) fn measure_transparency(&mut self) {
        self.renderer.callback_resources.insert(viewport3d::TransparencyProbe::default());
    }

    pub(crate) fn take_transparency_samples(&mut self) -> Vec<viewport3d::SortSample> {
        self.renderer.callback_resources.get_mut::<viewport3d::TransparencyProbe>()
            .map_or_else(Vec::new, |probe| std::mem::take(&mut probe.0))
    }

    /// Runs the editor's real layout and WGPU callbacks, returning packed top-to-bottom RGB8.
    pub fn frame(
        &mut self,
        mut input: egui::RawInput,
        draw: impl FnMut(&egui::Context),
    ) -> Result<Vec<u8>, String> {
        let start = self.frame_timings.is_some().then(Instant::now);
        let [width, height] = self.size;
        input.max_texture_side = Some(self.device.limits().max_texture_dimension_2d as usize);
        input.viewport_id = egui::ViewportId::ROOT;
        let viewport = input.viewports.entry(egui::ViewportId::ROOT).or_default();
        let scale = viewport.native_pixels_per_point.filter(|value| value.is_finite() && *value > 0.0).unwrap_or(1.0);
        let bounds = egui::Rect::from_min_size(
            egui::Pos2::ZERO, egui::vec2(width as f32 / scale, height as f32 / scale),
        );
        input.screen_rect = Some(bounds);
        viewport.native_pixels_per_point = Some(scale);
        viewport.inner_rect = Some(bounds);
        viewport.focused = Some(input.focused);
        let output = self.context.run(input, draw);
        let pixels_per_point = output.pixels_per_point;
        let jobs = self
            .context
            .tessellate(output.shapes.clone(), pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: self.size,
            pixels_per_point,
        };
        let mut encoder = self.device.create_command_encoder(&Default::default());
        for (id, delta) in &output.textures_delta.set {
            self.renderer
                .update_texture(&self.device, &self.queue, *id, delta);
        }
        let callbacks =
            self.renderer
                .update_buffers(&self.device, &self.queue, &mut encoder, &jobs, &screen);
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Studio offscreen frame"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: self.multisampled.as_ref().unwrap_or(&self.color_view),
                        depth_slice: None,
                        resolve_target: self.multisampled.as_ref().map(|_| &self.color_view),
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: if self.multisampled.is_some() {
                                wgpu::StoreOp::Discard
                            } else {
                                wgpu::StoreOp::Store
                            },
                        },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &self.depth,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Discard,
                        }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                })
                .forget_lifetime();
            self.renderer.render(&mut pass, &jobs, &screen);
        }
        if let Some(timing) = &self.gpu_timing {
            encoder.resolve_query_set(&timing.queries, 0..TIMESTAMP_COUNT, &timing.resolved, 0);
            encoder.copy_buffer_to_buffer(&timing.resolved, 0, &timing.readback, 0, TIMESTAMP_BYTES);
        }
        encoder.copy_texture_to_buffer(
            self.color.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_row),
                    rows_per_image: Some(height),
                },
            },
            self.extent,
        );
        let submission = self
            .queue
            .submit(callbacks.into_iter().chain([encoder.finish()]));
        for id in &output.textures_delta.free {
            self.renderer.free_texture(id);
        }
        self.last_output = Some(output);

        let (mapped, result) = mpsc::sync_channel(1);
        let slice = self.readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, move |status| {
            let _ = mapped.send(status);
        });
        let time_result = self.gpu_timing.as_ref().map(|timing| {
            let (mapped, result) = mpsc::sync_channel(1);
            timing.readback.slice(..).map_async(wgpu::MapMode::Read, move |status| {
                let _ = mapped.send(status);
            });
            result
        });
        let ready = self
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(GPU_TIMEOUT),
            })
            .map_err(|error| format!("Фоновый рендер не завершил кадр: {error}"))
            .and_then(|_| {
                result
                    .recv_timeout(GPU_TIMEOUT)
                    .map_err(|error| format!("Фоновый рендер не вернул кадр: {error}"))?
                    .map_err(|error| format!("Не удалось прочитать пиксели кадра: {error}"))
            });
        if let Err(error) = ready {
            self.readback.unmap();
            if let Some(timing) = &self.gpu_timing {
                timing.readback.unmap();
            }
            return Err(error);
        }
        let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
        {
            let data = slice.get_mapped_range();
            for row in data.chunks_exact(self.padded_row as usize) {
                for rgba in row[..width as usize * 4].chunks_exact(4) {
                    rgb.extend_from_slice(&rgba[..3]);
                }
            }
        }
        self.readback.unmap();
        if let (Some(timing), Some(receipt)) = (&mut self.gpu_timing, time_result) {
            receipt.recv_timeout(GPU_TIMEOUT)
                .map_err(|error| format!("Видеоустройство не вернуло замер HDR: {error}"))?
                .map_err(|error| format!("Не удалось прочитать замер HDR: {error}"))?;
            let times = timing.readback.slice(..).get_mapped_range();
            let ticks: &[u64] = bytemuck::cast_slice(&times);
            let ms = |start: usize, end: usize| ticks[end].saturating_sub(ticks[start]) as f32 * self.queue.get_timestamp_period() / 1_000_000.0;
            timing.frames.push(GpuFrameTimes { geometry_ms: ms(0, 1), bloom_ms: ms(1, 2), composite_ms: ms(3, 4), shadow_ms: ms(5, 6) });
            drop(times);
            timing.readback.unmap();
        }
        if let (Some(start), Some(samples)) = (start, self.frame_timings.as_mut()) {
            samples.push(start.elapsed().as_secs_f32() * 1_000.0);
        }
        Ok(rgb)
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    struct Resume(thread::Thread);

    impl Wake for Resume {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(Arc::new(Resume(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => thread::park(),
        }
    }
}
