//! Asynchronous Vello GPU rendering of owned R graphics scenes.
//! No interpreter pointer or session guard crosses an asynchronous boundary.
#![forbid(unsafe_code)]
use r_graphics_engine::{
    Color, DrawTarget, FontBook, LineCap, LineJoin, Path, PathCommand, PlotParameters, Point,
    RasterImage, Scene, Stroke, TextAnchor,
};
use std::sync::Arc;
use vello::{
    kurbo::{self, Affine, BezPath, Rect},
    peniko::{self, Blob, Fill, FontData},
    wgpu,
};

#[derive(Debug, thiserror::Error)]
pub enum GpuError {
    #[error("GPU initialization failed: {0}")]
    Initialization(String),
    #[error("invalid graphics scene: {0}")]
    InvalidScene(String),
    #[error("GPU rendering failed: {0}")]
    Render(String),
    #[error("GPU readback failed: {0}")]
    Readback(String),
    #[error("PNG encoding failed: {0}")]
    Png(#[from] png::EncodingError),
}

/// Reusable GPU device and Vello pipelines. Initialization never falls back to Vello CPU.
pub struct GpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: vello::Renderer,
    adapter_info: wgpu::AdapterInfo,
}
impl GpuRenderer {
    pub async fn new() -> Result<Self, GpuError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await
            .map_err(|e| GpuError::Initialization(e.to_string()))?;
        let adapter_info = adapter.get_info();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("R Vello GPU"),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            })
            .await
            .map_err(|e| GpuError::Initialization(e.to_string()))?;
        let renderer = vello::Renderer::new(
            &device,
            vello::RendererOptions {
                use_cpu: false,
                num_init_threads: std::num::NonZeroUsize::new(1),
                ..Default::default()
            },
        )
        .map_err(|e| GpuError::Initialization(e.to_string()))?;
        Ok(Self {
            device,
            queue,
            renderer,
            adapter_info,
        })
    }
    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.adapter_info
    }

    /// Device used by returned textures, for host compositing without readback.
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Render a premultiplied RGBA8 texture for direct host compositing.
    pub async fn render_texture(&mut self, scene: &Scene) -> Result<wgpu::Texture, GpuError> {
        // Validate programmatically constructed scenes as well as decoded recordings.
        scene
            .validate()
            .map_err(|e| GpuError::InvalidScene(e.into()))?;
        let (width, height) = scene.dimensions();
        if width > self.device.limits().max_texture_dimension_2d
            || height > self.device.limits().max_texture_dimension_2d
            || u64::from(width) * u64::from(height) > 16_777_216
        {
            return Err(GpuError::InvalidScene(
                "canvas exceeds GPU texture or 16M-pixel limit".into(),
            ));
        }
        let mut encoded = Encoder::new(width, height);
        scene.replay(&mut encoded);
        encoded.set_clip(None);
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let oom_scope = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let validation_scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("R plot"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        self.renderer
            .render_to_texture(
                &self.device,
                &self.queue,
                &encoded.scene,
                &texture.create_view(&Default::default()),
                &vello::RenderParams {
                    base_color: peniko::Color::TRANSPARENT,
                    width,
                    height,
                    antialiasing_method: vello::AaConfig::Msaa16,
                },
            )
            .map_err(|e| GpuError::Render(e.to_string()))?;
        let validation_error = validation_scope.pop();
        let oom_error = oom_scope.pop();
        if let Some(error) = validation_error.await.or(oom_error.await) {
            return Err(GpuError::Render(error.to_string()));
        }
        Ok(texture)
    }

    /// Render to straight-alpha RGBA8. Native readback waits at most 30 seconds;
    /// browser readback yields to the browser event loop.
    pub async fn render_rgba(&mut self, scene: &Scene) -> Result<Vec<u8>, GpuError> {
        let texture = self.render_texture(scene).await?;
        let (width, height) = scene.dimensions();
        let size = texture.size();
        let oom_scope = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let validation_scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let stride = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("R plot readback"),
            size: u64::from(stride) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut commands = self.device.create_command_encoder(&Default::default());
        commands.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(height),
                },
            },
            size,
        );
        self.queue.submit([commands.finish()]);
        let validation_error = validation_scope.pop();
        let oom_error = oom_scope.pop();
        if let Some(error) = validation_error.await.or(oom_error.await) {
            return Err(GpuError::Render(error.to_string()));
        }
        let (send, receive) = futures_channel::oneshot::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = send.send(result);
            });
        #[cfg(not(target_arch = "wasm32"))]
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(30)),
            })
            .map_err(|e| GpuError::Readback(e.to_string()))?;
        receive
            .await
            .map_err(|e| GpuError::Readback(e.to_string()))?
            .map_err(|e| GpuError::Readback(e.to_string()))?;
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        {
            let mapped = buffer.slice(..).get_mapped_range();
            for row in mapped.chunks_exact(stride as usize) {
                rgba.extend_from_slice(&row[..width as usize * 4]);
            }
        }
        buffer.unmap();
        for p in rgba.chunks_exact_mut(4) {
            if p[3] > 0 && p[3] < 255 {
                for j in 0..3 {
                    p[j] = ((u32::from(p[j]) * 255 + u32::from(p[3]) / 2) / u32::from(p[3]))
                        .min(255) as u8;
                }
            }
        }
        Ok(rgba)
    }
    pub async fn render_png(&mut self, scene: &Scene) -> Result<Vec<u8>, GpuError> {
        let rgba = self.render_rgba(scene).await?;
        let (width, height) = scene.dimensions();
        let mut output = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut output, width, height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header()?;
            writer.write_image_data(&rgba)?;
            writer.finish()?;
        }
        Ok(output)
    }
}

struct Encoder {
    scene: vello::Scene,
    width: u32,
    height: u32,
    clipped: bool,
    font: FontBook,
}
impl Encoder {
    fn new(width: u32, height: u32) -> Self {
        Self {
            scene: vello::Scene::new(),
            width,
            height,
            clipped: false,
            font: FontBook::default(),
        }
    }
}
fn color(c: Color) -> peniko::Color {
    peniko::Color::from_rgba8(c.r, c.g, c.b, c.a)
}
fn geometry(path: &Path) -> BezPath {
    let mut p = BezPath::new();
    for command in &path.commands {
        match *command {
            PathCommand::MoveTo(x, y) => p.move_to((f64::from(x), f64::from(y))),
            PathCommand::LineTo(x, y) => p.line_to((f64::from(x), f64::from(y))),
            PathCommand::QuadTo(a, b, x, y) => {
                p.quad_to((f64::from(a), f64::from(b)), (f64::from(x), f64::from(y)))
            }
            PathCommand::CubicTo(a, b, c, d, x, y) => p.curve_to(
                (f64::from(a), f64::from(b)),
                (f64::from(c), f64::from(d)),
                (f64::from(x), f64::from(y)),
            ),
            // This legacy command has no arc flags/rotation; retain its documented endpoint fallback.
            PathCommand::ArcTo { x, y, .. } => p.line_to((f64::from(x), f64::from(y))),
            PathCommand::Close => p.close_path(),
        }
    }
    p
}
fn stroke(s: &Stroke) -> kurbo::Stroke {
    let mut result = kurbo::Stroke::new(f64::from(s.width));
    result.start_cap = match s.cap {
        LineCap::Butt => kurbo::Cap::Butt,
        LineCap::Round => kurbo::Cap::Round,
        LineCap::Square => kurbo::Cap::Square,
    };
    result.end_cap = result.start_cap;
    result.join = match s.join {
        LineJoin::Miter => kurbo::Join::Miter,
        LineJoin::Round => kurbo::Join::Round,
        LineJoin::Bevel => kurbo::Join::Bevel,
    };
    result.miter_limit = f64::from(s.miter_limit);
    if let Some(dash) = &s.dash_pattern {
        result.dash_pattern = dash.intervals.iter().map(|v| f64::from(*v)).collect();
        result.dash_offset = f64::from(dash.offset);
    }
    result
}
impl DrawTarget for Encoder {
    fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    fn clear(&mut self, c: Color) {
        self.scene.reset();
        self.clipped = false;
        self.scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            color(c),
            None,
            &Rect::new(0., 0., self.width as f64, self.height as f64),
        );
    }
    fn set_clip(&mut self, clip: Option<[f32; 4]>) {
        if self.clipped {
            self.scene.pop_layer();
            self.clipped = false;
        }
        if let Some([x0, y0, x1, y1]) = clip {
            self.scene.push_clip_layer(
                Fill::NonZero,
                Affine::IDENTITY,
                &Rect::new(x0 as f64, y0 as f64, x1 as f64, y1 as f64),
            );
            self.clipped = true;
        }
    }
    fn draw_path(&mut self, p: &Path) {
        let path = geometry(p);
        if p.fill.a > 0 {
            self.scene
                .fill(Fill::NonZero, Affine::IDENTITY, color(p.fill), None, &path);
        }
        if p.stroke.width > 0. && p.stroke.color.a > 0 {
            self.scene.stroke(
                &stroke(&p.stroke),
                Affine::IDENTITY,
                color(p.stroke.color),
                None,
                &path,
            );
        }
    }
    fn draw_image(&mut self, image: &RasterImage, transform: [f64; 6], interpolate: bool) {
        let brush = peniko::ImageBrush {
            image: peniko::ImageData {
                data: Blob::new(Arc::new(image.pixels().to_vec())),
                format: peniko::ImageFormat::Rgba8,
                alpha_type: peniko::ImageAlphaType::Alpha,
                width: image.width(),
                height: image.height(),
            },
            sampler: peniko::ImageSampler {
                quality: if interpolate {
                    peniko::ImageQuality::Medium
                } else {
                    peniko::ImageQuality::Low
                },
                ..Default::default()
            },
        };
        self.scene.draw_image(&brush, Affine::new(transform));
    }
    fn draw_text(&mut self, text: &str, pos: Point, params: &PlotParameters) {
        let size = if params.font_size > 0. {
            params.font_size
        } else {
            12.
        };
        let width = self.font.measure_text(text, size, params.font_face).width;
        let mut x = match params.text_anchor {
            TextAnchor::Start => 0.,
            TextAnchor::Middle => -width / 2.,
            TextAnchor::End => -width,
        };
        let glyphs: Vec<_> = text
            .chars()
            .filter(|c| !c.is_control())
            .map(|c| {
                let glyph = vello::Glyph {
                    id: self.font.glyph_index(c).into(),
                    x,
                    y: 0.,
                };
                x += self.font.advance_width(c, size);
                glyph
            })
            .collect();
        let data = FontData::new(Blob::new(self.font.bytes()), 0);
        let transform = Affine::translate((pos.x as f64, pos.y as f64))
            * Affine::rotate(-(params.text_angle as f64).to_radians());
        let shear = Some(Affine::new([
            1.,
            0.,
            params.font_face.italic_shear(),
            1.,
            0.,
            0.,
        ]));
        self.scene
            .draw_glyphs(&data)
            .font_size(size)
            .transform(transform)
            .glyph_transform(shear)
            .brush(color(params.text_color))
            .draw(Fill::NonZero, glyphs.iter().copied());
        if params.font_face.is_bold() {
            self.scene
                .draw_glyphs(&data)
                .font_size(size)
                .transform(transform)
                .glyph_transform(shear)
                .brush(color(params.text_color))
                .draw(
                    &kurbo::Stroke::new(params.font_face.bold_stroke_width(size)),
                    glyphs.iter().copied(),
                );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_scene_is_rejected_without_gpu() {
        assert!(Scene::new(0, 100).validate().is_err());
    }
    /// Explicit opt-in because ordinary test runners may have no GPU adapter.
    #[test]
    #[ignore = "requires a compute-capable GPU; run with --ignored --nocapture"]
    fn actual_gpu_pixels_and_reuse() {
        pollster::block_on(async {
            let mut gpu = GpuRenderer::new().await.expect("GPU adapter required");
            eprintln!("adapter: {:?}", gpu.adapter_info());
            let mut scene = Scene::new(65, 48); // exercises padded readback rows
            scene.clear(Color::WHITE);
            let image = RasterImage::new(1, 1, vec![255, 0, 0, 128]).unwrap();
            scene.draw_image(&image, [20., 0., 0., 20., 5., 5.], false);
            let pixels = gpu.render_rgba(&scene).await.unwrap();
            let p = &pixels[(10 * 65 + 10) * 4..][..4];
            assert_eq!(p[0], 255);
            assert!((125..=130).contains(&p[1]));
            assert_eq!(p[3], 255);
            assert_eq!(&pixels[..4], &[255, 255, 255, 255]);
            scene.clear(Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            });
            scene.draw_text(
                "GPU α",
                Point { x: 4., y: 30. },
                &PlotParameters {
                    font_size: 18.,
                    text_color: Color::BLACK,
                    ..Default::default()
                },
            );
            let pixels = gpu.render_rgba(&scene).await.unwrap();
            assert!(pixels.chunks_exact(4).filter(|p| p[3] > 0).count() > 40);
            assert_eq!(&pixels[..4], &[0, 0, 0, 0]);
        });
    }
}
