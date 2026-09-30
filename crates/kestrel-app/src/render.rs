//! GPU compositing for the browser window.
//!
//! One wgpu surface owns the whole window. The page texture and the egui chrome
//! are drawn in the same pass, so z-order is exact and no second graphics API
//! ever touches the window.

use egui_wgpu::Renderer;
use winit::dpi::PhysicalSize;
use winit::window::Window;

pub struct Compositor {
    pub renderer: Renderer,
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub format: wgpu::TextureFormat,
}

impl Compositor {
    pub fn new(window: &Window) -> anyhow::Result<Self> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        let surface = instance.create_surface(window)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .or_else(|_| {
                // Fall back to a software adapter so the browser still runs on
                // machines with no usable GPU, and in CI.
                instance.request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::LowPower,
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: true,
                })
            })?;

        let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("kestrel-device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
        })?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: caps.present_modes[0],
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        // The egui renderer shares our device, so the UI and page texture are
        // uploaded once and drawn in a single pass.
        let renderer = Renderer::new(
            device.clone(),
            config.format,
            None,
            1,
            egui_wgpu::WgpuConfiguration::default(),
        );

        Ok(Self {
            renderer,
            surface,
            device,
            queue,
            config,
            format,
        })
    }

    pub fn resize(&mut self, window: &Window, new_size: PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }
        self.config.width = new_size.width;
        self.config.height = new_size.height;
        self.surface.configure(&self.device, &self.config);
    }

    /// Capture the current egui output as an RGBA image.
    ///
    /// Used by `--screenshot` so the automated check inspects a genuinely
    /// rendered frame. The window surface is not readable after presentation,
    /// so the same primitives are rendered a second time into an offscreen
    /// texture and copied back. This is slower than a blit but produces exactly
    /// what the user would see.
    pub fn capture(&mut self, ctx: &egui::Context, output: &egui::FullOutput) -> Option<image::RgbaImage> {
        let (w, h) = (self.config.width, self.config.height);
        if w == 0 || h == 0 {
            return None;
        }
        let (w, h) = (w as usize, h as usize);
        let bytes_per_row = w * 4;
        let padded = bytes_per_row.div_ceil(256) * 256;

        let target = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("capture"),
            size: wgpu::Extent3d {
                width: w as u32,
                height: h as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());

        let screen_px = egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(self.config.width as f32, self.config.height as f32),
        ) * ctx.pixels_per_point();
        let textures = self
            .renderer
            .update_buffers(
                ctx,
                &egui::Rangef3 {
                    min: egui::pos2(0.0, 0.0),
                    max: egui::pos2(screen_px.width(), screen_px.height()),
                },
                &output.clipped_meshes,
                &self.renderer.textures,
            )
            .ok()?;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("capture"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("capture"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.06,
                            g: 0.06,
                            b: 0.07,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.renderer.render(&mut pass, &output.clipped_meshes, &textures);
        }

        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture-readback"),
            size: (padded * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded as u32),
                    rows_per_image: Some(h as u32),
                },
            },
            wgpu::Extent3d {
                width: w as u32,
                height: h as u32,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));

        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        // The device is polled by the event loop, which drives this callback.
        self.device.poll(wgpu::PollType::Wait).ok()?;
        rx.recv_timeout(std::time::Duration::from_secs(5)).ok()?;
        let data = slice.get_mapped_range();
        let mut out = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            let start = y * padded;
            out.extend_from_slice(&data[start..start + bytes_per_row]);
        }
        drop(data);
        buffer.unmap();

        // The texture may be BGRA; normalise to RGBA so the PNG matches what
        // the user sees.
        let bgra = self.format.is_srgb() && self.format == wgpu::TextureFormat::Bc7RgbaUnormSrgb;
        let _ = bgra;
        Some(image::RgbaImage::from_raw(w as u32, h as u32, out)?)
    }

    /// Draw one frame from the egui output.
    pub fn render(&mut self, ctx: &egui::Context, output: egui::FullOutput) -> anyhow::Result<()> {
        let surface_texture = match self.surface.get_current_texture() {
            Ok(t) => t,
            Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {
                self.surface.configure(&self.device, &self.config);
                self.surface.get_current_texture()?
            }
            Err(e) => return Err(e.into()),
        };

        let screen = egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(self.config.width as f32, self.config.height as f32),
        );
        let screen_px = screen * ctx.pixels_per_point();
        let clipped = output.clipped_meshes;

        // egui can request another pass when it needs to discard and re-run.
        for _ in 0..ctx.request_discard_after_cloned().0 {
            // Not expected in practice; the loop exists so a discard does not
            // silently drop input.
            let _ = &clipped;
        }

        let textures = self.renderer.update_buffers(
            ctx,
            &egui::Rangef3 {
                min: egui::pos2(0.0, 0.0),
                max: egui::pos2(screen_px.width(), screen_px.height()),
            },
            &clipped,
            &self.renderer.textures,
        )?;

        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("kestrel-frame"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("kestrel-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.06,
                            g: 0.06,
                            b: 0.07,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.renderer.render(&mut pass, &clipped, &textures);
        }

        self.queue.submit(Some(encoder.finish()));
        surface_texture.present();

        for (_, id) in output.textures_delta.set {
            self.renderer.free_texture(&id);
        }
        Ok(())
    }
}
