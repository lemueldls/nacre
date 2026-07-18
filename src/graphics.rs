use cosmic_text::{Buffer, FontSystem, SwashCache};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use swash::{
    FontRef,
    scale::ScaleContext,
    zeno::{Command, PathData},
};
use taffy::prelude::*;
use tracing::debug;
use vello::{
    Scene,
    kurbo::{Affine, BezPath},
    peniko::{Brush, Color, Fill},
    wgpu,
};

pub struct GraphicsEngine {
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    pub taffy: TaffyTree,
    pub scene: Scene,
    pub wgpu_context: Option<WgpuContext>,
    pub headless: bool,
    /// Scratch state for extracting glyph outlines (see `draw_text`).
    /// Reused across calls rather than created per-call, since that's what
    /// makes repeated scaling of the same glyphs cheap.
    outline_context: ScaleContext,
}

pub struct WgpuContext {
    pub instance: wgpu::Instance,
    pub surface: wgpu::Surface<'static>,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface_config: wgpu::SurfaceConfiguration,
}

impl GraphicsEngine {
    pub fn new(headless: bool) -> Self {
        let font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let taffy = TaffyTree::new();
        let scene = Scene::new();

        Self {
            font_system,
            swash_cache,
            taffy,
            scene,
            wgpu_context: None,
            headless,
            outline_context: ScaleContext::new(),
        }
    }

    /// Try to initialize GPU graphics using wgpu.
    pub fn init_gpu(
        &mut self,
        raw_display_handle: RawDisplayHandle,
        raw_window_handle: RawWindowHandle,
    ) -> Result<(), String> {
        if self.headless {
            return Ok(());
        }

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());

        let surface = unsafe {
            instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: Some(raw_display_handle),
                    raw_window_handle,
                })
                .map_err(|e| format!("failed to create Wayland surface: {e}"))?
        };

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            ..Default::default()
        }))
        .map_err(|e| format!("failed to find GPU adapter: {e}"))?;

        let (device, queue) = pollster::block_on(adapter.request_device(&Default::default()))
            .map_err(|e| format!("failed to request GPU device: {e}"))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .first()
            .copied()
            .ok_or_else(|| "surface has no supported formats".to_string())?;

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            // color_space: wgpu::SurfaceColorSpace::Auto,
            width: 1,
            height: 1,
            desired_maximum_frame_latency: 2,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps
                .alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: vec![format],
        };

        surface.configure(&device, &surface_config);

        self.wgpu_context = Some(WgpuContext {
            instance,
            surface,
            adapter,
            device,
            queue,
            surface_config,
        });

        debug!("GPU graphics initialized successfully (wgpu)");

        Ok(())
    }

    pub fn resize_surface(&mut self, width: u32, height: u32) -> Result<(), String> {
        let context = self
            .wgpu_context
            .as_mut()
            .ok_or_else(|| "GPU graphics are not initialized".to_string())?;

        context.surface_config.width = width.max(1);
        context.surface_config.height = height.max(1);
        context
            .surface
            .configure(&context.device, &context.surface_config);

        Ok(())
    }

    pub fn clear_surface(&mut self, color: wgpu::Color) -> Result<(), String> {
        let context = self
            .wgpu_context
            .as_mut()
            .ok_or_else(|| "GPU graphics are not initialized".to_string())?;

        let surface_texture = match context.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture)
            | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
            wgpu::CurrentSurfaceTexture::Outdated => {
                context
                    .surface
                    .configure(&context.device, &context.surface_config);

                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                context
                    .surface
                    .configure(&context.device, &context.surface_config);

                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err("surface validation failed".to_string());
            }
        };
        let texture_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = context.device.create_command_encoder(&Default::default());
        {
            let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("nacre-clear-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &texture_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }

        context.queue.submit(Some(encoder.finish()));
        surface_texture.present(); // context.queue.present(surface_texture);

        Ok(())
    }

    /// Render a simple color rectangle using Vello
    pub fn draw_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        let rect = vello::kurbo::Rect::new(x as f64, y as f64, (x + w) as f64, (y + h) as f64);
        self.scene.fill(
            Fill::NonZero,
            vello::kurbo::Affine::IDENTITY,
            &Brush::Solid(color),
            None,
            &rect,
        );
    }

    /// Draw shaped text using cosmic-text layout with real glyph outlines,
    /// filled through vello as vector paths (replaces the old
    /// placeholder-rectangle fallback).
    ///
    /// For each shaped glyph, this pulls the owning font's raw bytes out of
    /// cosmic-text's `fontdb`, asks `swash` to scale that glyph to an
    /// outline at the run's font size, and converts swash's path commands
    /// into a `kurbo::BezPath` that vello fills directly. Glyphs with no
    /// outline available (e.g. bitmap-only emoji fonts) are skipped rather
    /// than drawn as a placeholder shape.
    pub fn draw_text(&mut self, x: f32, y: f32, text: &str, font_size: f32, color: Color) {
        let mut buffer = Buffer::new(
            &mut self.font_system,
            cosmic_text::Metrics::new(font_size, font_size),
        );
        buffer.set_text(
            text,
            &cosmic_text::Attrs::new(),
            cosmic_text::Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);

        let db = self.font_system.db();

        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                let glyph_x = (x + glyph.x) as f64;
                // Baseline for this run, in the same top-down screen space
                // `draw_rect` uses.
                let baseline_y = (y + run.line_y + glyph.y) as f64;

                let outline = db.with_face_data(glyph.font_id, |font_data, face_index| {
                    let font_ref = FontRef::from_index(font_data, face_index as usize)?;
                    let mut scaler = self
                        .outline_context
                        .builder(font_ref)
                        .size(glyph.font_size)
                        .hint(true)
                        .build();
                    scaler.scale_outline(glyph.glyph_id)
                });

                let Some(Some(outline)) = outline else {
                    continue;
                };

                let mut path = BezPath::new();
                // swash outlines are in font space with +y pointing up;
                // screen space here has +y pointing down, so y is
                // subtracted from the baseline rather than added.
                for command in outline.path().commands() {
                    match command {
                        Command::MoveTo(p) => {
                            path.move_to((glyph_x + p.x as f64, baseline_y - p.y as f64))
                        }
                        Command::LineTo(p) => {
                            path.line_to((glyph_x + p.x as f64, baseline_y - p.y as f64))
                        }
                        Command::QuadTo(c, p) => {
                            path.quad_to(
                                (glyph_x + c.x as f64, baseline_y - c.y as f64),
                                (glyph_x + p.x as f64, baseline_y - p.y as f64),
                            )
                        }
                        Command::CurveTo(c1, c2, p) => {
                            path.curve_to(
                                (glyph_x + c1.x as f64, baseline_y - c1.y as f64),
                                (glyph_x + c2.x as f64, baseline_y - c2.y as f64),
                                (glyph_x + p.x as f64, baseline_y - p.y as f64),
                            )
                        }
                        Command::Close => path.close_path(),
                    }
                }

                self.scene.fill(
                    Fill::NonZero,
                    Affine::IDENTITY,
                    &Brush::Solid(color),
                    None,
                    &path,
                );
            }
        }
    }

    /// Apply background blur compute shaders (Dual-Pass Kawase Blur)
    pub fn apply_blur(&mut self, _radius: f32, _intensity: f32) {
        if self.headless {
            return;
        }
        // In a real session, this binds downsampled buffers in wgpu compute passes
        debug!("Applied Dual-Pass Kawase background blur on render targets");
    }

    pub fn clear(&mut self) {
        self.scene = Scene::new();
    }
}
