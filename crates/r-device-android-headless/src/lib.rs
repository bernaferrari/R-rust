//! Portable PNG rendering through Vello CPU, with vector glyphs and clipping.
//! No GPU initialization is required on native, mobile or Wasm targets.
#![forbid(unsafe_code)]
use r_graphics_engine::{
    Color, FontBook, LineCap, LineJoin, Path, PathCommand, PlotParameters, Point, RasterImage,
    RenderPlot, Stroke, TextAnchor, TextMetrics,
};

use std::sync::Arc;
use vello_cpu::kurbo::{self, Affine, BezPath, Rect, Shape};
use vello_cpu::peniko::{Blob, FontData};
use vello_cpu::{Glyph, Pixmap, RenderContext, Resources};

/// Vello's CPU rasterizer behind the shared graphics device interface.
/// Canvas sizes are limited to 16 million pixels and u16 dimensions.
pub struct VelloRenderer {
    width: u16,
    height: u16,
    context: RenderContext,
    resources: Resources,
    font: Option<FontBook>,
    clipped: bool,
}
/// Compatibility name for existing embedding and mobile callers.
pub type AndroidHeadlessRenderer = VelloRenderer;
impl std::fmt::Debug for VelloRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VelloRenderer")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}
impl Default for VelloRenderer {
    fn default() -> Self {
        Self::new(1, 1)
    }
}
impl VelloRenderer {
    pub fn new(width: u32, height: u32) -> Self {
        Self::try_new(width.max(1), height.max(1)).expect("invalid Vello canvas dimensions")
    }
    pub fn try_new(width: u32, height: u32) -> Result<Self, String> {
        if width == 0
            || height == 0
            || width > u16::MAX as u32
            || height > u16::MAX as u32
            || u64::from(width) * u64::from(height) > 16_777_216
        {
            return Err(
                "plot canvas exceeds Vello limits (65535 per dimension, 16777216 pixels)".into(),
            );
        }
        let (width, height) = (width as u16, height as u16);
        Ok(Self {
            width,
            height,
            context: RenderContext::new(width, height),
            resources: Resources::new(),
            font: Some(FontBook::default()),
            clipped: false,
        })
    }
    /// Encode the rendered image, reporting any PNG encoding failure.
    pub fn try_finish(mut self) -> Result<Vec<u8>, png::EncodingError> {
        let rgba = self.straight_alpha_pixmap();
        let mut output = Vec::new();
        {
            let mut encoder =
                png::Encoder::new(&mut output, u32::from(self.width), u32::from(self.height));
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header()?;
            writer.write_image_data(rgba.data_as_u8_slice())?;
            writer.finish()?;
        }
        Ok(output)
    }
    pub fn set_font(&mut self, bytes: Vec<u8>) -> Result<(), String> {
        self.font = Some(FontBook::from_bytes(bytes)?);
        Ok(())
    }
    // Keep the canvas in one allocation through PNG encoding. This pixmap
    // contains straight alpha and must not be passed back to Vello.
    fn straight_alpha_pixmap(&mut self) -> Pixmap {
        let mut image = Pixmap::new(self.width, self.height);
        self.context.flush();
        self.context.render(&mut image, &mut self.resources);
        for p in image.data_as_u8_slice_mut().chunks_exact_mut(4) {
            if p[3] > 0 && p[3] < 255 {
                for j in 0..3 {
                    p[j] = ((u32::from(p[j]) * 255 + u32::from(p[3]) / 2) / u32::from(p[3]))
                        .min(255) as u8;
                }
            }
        }
        image
    }

    #[cfg(test)]
    fn pixels(&mut self) -> Vec<u8> {
        self.straight_alpha_pixmap().data_as_u8_slice().to_vec()
    }
}
fn color(c: Color) -> vello_cpu::color::AlphaColor<vello_cpu::color::Srgb> {
    vello_cpu::color::AlphaColor::from_rgba8(c.r, c.g, c.b, c.a)
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
impl RenderPlot for VelloRenderer {
    type Output = Vec<u8>;
    fn new(w: u32, h: u32) -> Self {
        Self::new(w, h)
    }
    fn dimensions(&self) -> (u32, u32) {
        (u32::from(self.width), u32::from(self.height))
    }
    fn clear(&mut self, c: Color) {
        self.context.reset();
        self.clipped = false;
        self.context.set_paint(color(c));
        self.context.fill_rect(&Rect::new(
            0.,
            0.,
            f64::from(self.width),
            f64::from(self.height),
        ));
    }
    fn set_clip(&mut self, rect: Option<[f32; 4]>) {
        if self.clipped {
            self.context.pop_clip_path();
            self.clipped = false;
        }
        if let Some([x0, y0, x1, y1]) = rect {
            let path =
                Rect::new(f64::from(x0), f64::from(y0), f64::from(x1), f64::from(y1)).to_path(0.1);
            self.context.push_clip_path(&path);
            self.clipped = true;
        }
    }
    fn draw_path(&mut self, p: &Path) {
        self.context
            .set_aliasing_threshold(if p.anti_alias { None } else { Some(127) });
        let path = geometry(p);
        if p.fill.a > 0 {
            self.context.set_paint(color(p.fill));
            self.context.fill_path(&path);
        }
        if p.stroke.width > 0. && p.stroke.color.a > 0 {
            self.context.set_paint(color(p.stroke.color));
            self.context.set_stroke(stroke(&p.stroke));
            self.context.stroke_path(&path);
        }
    }
    fn draw_image(&mut self, image: &RasterImage, transform: [f64; 6], interpolate: bool) {
        use vello_cpu::color::PremulRgba8;
        use vello_cpu::peniko::{ImageQuality, ImageSampler};
        let pixels = image
            .pixels()
            .chunks_exact(4)
            .map(|p| {
                let premultiply = |v: u8| ((u16::from(v) * u16::from(p[3]) + 127) / 255) as u8;
                PremulRgba8 {
                    r: premultiply(p[0]),
                    g: premultiply(p[1]),
                    b: premultiply(p[2]),
                    a: p[3],
                }
            })
            .collect();
        let pixmap = Pixmap::from_parts(pixels, image.width() as u16, image.height() as u16);
        self.context.set_aliasing_threshold(None);
        self.context.set_transform(Affine::new(transform));
        self.context.set_paint(vello_cpu::Image {
            image: vello_cpu::ImageSource::Pixmap(Arc::new(pixmap)),
            sampler: ImageSampler {
                quality: if interpolate {
                    ImageQuality::Medium
                } else {
                    ImageQuality::Low
                },
                ..Default::default()
            },
        });
        self.context.fill_rect(&Rect::new(
            0.,
            0.,
            f64::from(image.width()),
            f64::from(image.height()),
        ));
        self.context.reset_transform();
    }
    fn measure_math_text(&self, text: &str, params: &PlotParameters) -> TextMetrics {
        self.font
            .as_ref()
            .map_or_else(TextMetrics::default, |font| {
                font.measure_math_text(text, params.font_size, params.font_face)
            })
    }
    fn measure_text(&self, text: &str, params: &PlotParameters) -> TextMetrics {
        self.font
            .as_ref()
            .map_or_else(TextMetrics::default, |font| {
                font.measure_text(text, params.font_size, params.font_face)
            })
    }
    fn draw_text(&mut self, text: &str, pos: Point, params: &PlotParameters) {
        self.context.set_aliasing_threshold(None);
        let Some(font) = &self.font else {
            return;
        };
        let size = if params.font_size > 0. {
            params.font_size
        } else {
            12.
        };
        if !pos.x.is_finite()
            || !pos.y.is_finite()
            || !size.is_finite()
            || !params.text_angle.is_finite()
            || params.text_color.a == 0
        {
            return;
        }
        let chars: Vec<_> = text.chars().filter(|c| !c.is_control()).collect();
        let width: f32 = chars
            .iter()
            .map(|c| font.advance_width_for_face(*c, size, params.font_face))
            .sum();
        let mut x = match params.text_anchor {
            TextAnchor::Start => 0.,
            TextAnchor::Middle => -width / 2.,
            TextAnchor::End => -width,
        };
        let glyphs: Vec<_> = chars
            .into_iter()
            .map(|c| {
                let g = Glyph {
                    id: u32::from(font.glyph_index_for_face(c, params.font_face)),
                    x,
                    y: 0.,
                };
                x += font.advance_width_for_face(c, size, params.font_face);
                g
            })
            .collect();
        self.context.set_paint(color(params.text_color));
        self.context.set_transform(
            Affine::translate((f64::from(pos.x), f64::from(pos.y)))
                * Affine::rotate(-f64::from(params.text_angle).to_radians()),
        );
        let data = FontData::new(Blob::new(font.bytes_for_face(params.font_face)), 0);
        self.context
            .glyph_run(&mut self.resources, &data)
            .font_size(size)
            .fill_glyphs(glyphs.iter().copied());
        self.context.reset_transform();
    }
    fn finish(self) -> Vec<u8> {
        self.try_finish()
            .expect("validated in-memory RGBA PNG encoding")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_encoding_preserves_straight_alpha_pixels() {
        let mut renderer = VelloRenderer::new(2, 1);
        let image = RasterImage::new(2, 1, vec![200, 100, 50, 128, 0, 0, 0, 0]).unwrap();
        renderer.draw_image(&image, [1., 0., 0., 1., 0., 0.], false);
        let png = renderer.try_finish().unwrap();
        let mut reader = png::Decoder::new(std::io::Cursor::new(png))
            .read_info()
            .unwrap();
        let mut bytes = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut bytes).unwrap();
        assert_eq!((info.width, info.height), (2, 1));
        assert_eq!(info.color_type, png::ColorType::Rgba);
        // Premultiplication is quantized to eight bits by the renderer.
        assert_eq!(
            &bytes[..info.buffer_size()],
            &[199, 100, 50, 128, 0, 0, 0, 0]
        );
    }

    #[test]
    fn bold_and_italic_render_distinct_outlines() {
        use r_graphics_engine::FontFace;
        let render = |face| {
            let mut renderer = VelloRenderer::new(100, 70);
            renderer.clear(Color::WHITE);
            renderer.draw_text(
                "Hello",
                Point { x: 10., y: 50. },
                &PlotParameters {
                    font_size: 28.,
                    text_color: Color::BLACK,
                    font_face: face,
                    ..Default::default()
                },
            );
            renderer.pixels()
        };
        let plain = render(FontFace::Plain);
        let bold = render(FontFace::Bold);
        let italic = render(FontFace::Italic);
        let both = render(FontFace::BoldItalic);
        let ink = |pixels: &[u8]| {
            pixels
                .chunks_exact(4)
                .map(|p| u64::from(255 - p[0]))
                .sum::<u64>()
        };
        assert!(ink(&bold) > ink(&plain));
        assert_ne!(plain, italic);
        assert_ne!(bold, both);
        assert_ne!(italic, both);
    }

    #[test]
    fn custom_font_measurement_matches_draw_advances() {
        let mut renderer = VelloRenderer::new(100, 70);
        renderer
            .set_font(include_bytes!("../../r-graphics-engine/assets/DejaVuSans.ttf").to_vec())
            .unwrap();
        let params = PlotParameters {
            font_size: 20.,
            ..Default::default()
        };
        let actual = RenderPlot::measure_text(&renderer, "Wii", &params);
        let font = renderer.font.as_ref().unwrap();
        let expected: f32 = "Wii".chars().map(|c| font.advance_width(c, 20.)).sum();
        assert_eq!(actual.width, expected);
        assert!(actual.ascent > 0. && actual.descent > 0.);
    }

    #[test]
    fn rejects_invalid_or_excessive_canvas_sizes() {
        for (w, h) in [(0, 10), (10, 0), (65536, 1), (1, 65536), (8192, 8192)] {
            assert!(VelloRenderer::try_new(w, h).is_err());
        }
    }

    #[test]
    fn transformed_raster_preserves_orientation_alpha_and_clip() {
        let image = RasterImage::from_rgba8(
            2,
            2,
            vec![
                255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 0, 0, 128,
            ],
        )
        .unwrap();
        let mut r = VelloRenderer::new(40, 40);
        r.clear(Color::WHITE);
        r.set_clip(Some([10., 10., 30., 30.]));
        r.draw_image(&image, [10., 0., 0., 10., 10., 10.], false);
        let pixels = r.pixels();
        let pixel = |x: usize, y: usize| &pixels[(y * 40 + x) * 4..(y * 40 + x) * 4 + 4];
        assert_eq!(pixel(15, 15), [255, 0, 0, 255]);
        assert_eq!(pixel(25, 15), [0, 255, 0, 255]);
        assert_eq!(pixel(15, 25), [0, 0, 255, 255]);
        assert_eq!(pixel(25, 25), [255, 127, 127, 255]);
        assert_eq!(pixel(5, 5), [255, 255, 255, 255]);
    }

    #[test]
    fn test_render_basic_plot() {
        let mut renderer = AndroidHeadlessRenderer::new(800, 600);
        renderer.clear(Color::WHITE);

        let path = Path::rect(100.0, 100.0, 200.0, 150.0)
            .with_fill(Color::BLUE)
            .with_stroke(Stroke::new(2.0, Color::BLACK));

        renderer.draw_path(&path);
        let png = renderer.finish();

        assert!(!png.is_empty());
        assert!(png.starts_with(&[0x89, 0x50, 0x4E, 0x47]));
    }

    #[test]
    fn test_draw_text_no_panic_without_font() {
        let mut renderer = AndroidHeadlessRenderer::new(200, 100);
        renderer.font = None;
        renderer.draw_text(
            "Hello",
            Point { x: 10.0, y: 50.0 },
            &PlotParameters::default(),
        );
        let png = renderer.finish();
        assert!(!png.is_empty());
    }

    #[test]
    fn test_draw_text_keeps_non_ascii_glyphs() {
        let mut renderer = AndroidHeadlessRenderer::new(120, 60);
        renderer.clear(Color::WHITE);
        assert!(renderer.font.is_some(), "bundled font must be available");

        renderer.draw_text(
            "μ",
            Point { x: 12.0, y: 34.0 },
            &PlotParameters {
                font_size: 28.0,
                text_color: Color::BLACK,
                dpi: 96.0,
                ..Default::default()
            },
        );

        assert!(
            renderer
                .pixels()
                .chunks_exact(4)
                .any(|rgba| rgba != [255, 255, 255, 255])
        );
    }
    #[test]
    fn clips_geometry_and_can_restore_full_canvas() {
        let mut r = AndroidHeadlessRenderer::new(40, 40);
        r.clear(Color::WHITE);
        r.set_clip(Some([10., 10., 30., 30.]));
        r.draw_path(&Path::rect(0., 0., 40., 40.).with_fill(Color::RED));
        let pixels = r.pixels();
        assert_eq!(
            &pixels[(5 * 40 + 5) * 4..(5 * 40 + 5) * 4 + 4],
            &[255, 255, 255, 255]
        );
        assert_eq!(pixels[(20 * 40 + 20) * 4 + 1], 0);
        r.set_clip(None);
        r.draw_path(&Path::rect(0., 0., 8., 8.).with_fill(Color::BLUE));
        assert_eq!(r.pixels()[(5 * 40 + 5) * 4], 0);
    }

    #[test]
    fn bundled_font_draws_rotated_clipped_unicode() {
        let mut r = AndroidHeadlessRenderer::new(100, 100);
        r.set_font(include_bytes!("../../r-graphics-engine/assets/DejaVuSans.ttf").to_vec())
            .unwrap();
        r.clear(Color::WHITE);
        r.set_clip(Some([20., 20., 80., 80.]));
        r.draw_text(
            "μabc",
            Point { x: 50., y: 70. },
            &PlotParameters {
                font_size: 25.,
                text_color: Color::BLACK,
                text_angle: 90.,
                ..Default::default()
            },
        );
        let p = r.pixels();
        let mut ink = 0;
        for y in 0..100 {
            for x in 0..100 {
                let red = p[(y * 100 + x) * 4];
                if red != 255 {
                    ink += 1;
                    assert!((20..80).contains(&x) && (20..80).contains(&y));
                }
            }
        }
        assert!(ink > 50);
    }
}
