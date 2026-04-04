//! Android headless pure Rust graphics backend.
//!
//! Uses tiny-skia for zero system dependencies rendering.

#![forbid(unsafe_code)]

use std::vec::Vec;

use r_graphics_engine::{
    Color, LineCap, LineJoin, Path, PathCommand, PlotParameters, Point, RenderPlot, Stroke,
};

/// Android headless plot renderer.
#[derive(Debug, Default)]
pub struct AndroidHeadlessRenderer {
    width: u32,
    height: u32,
    pixmap: Option<tiny_skia::Pixmap>,
}

impl AndroidHeadlessRenderer {
    /// Create a new renderer with the given dimensions.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixmap: tiny_skia::Pixmap::new(width, height),
        }
    }
}

/// Convert an r_graphics_engine::Path to a tiny_skia::Path.
fn path_to_skia(path: &Path) -> tiny_skia::Path {
    let mut pb = tiny_skia::PathBuilder::new();
    for cmd in &path.commands {
        match cmd {
            PathCommand::MoveTo(x, y) => pb.move_to(*x, *y),
            PathCommand::LineTo(x, y) => pb.line_to(*x, *y),
            PathCommand::QuadTo(x1, y1, x2, y2) => pb.quad_to(*x1, *y1, *x2, *y2),
            PathCommand::CubicTo(x1, y1, x2, y2, x3, y3) => {
                pb.cubic_to(*x1, *y1, *x2, *y2, *x3, *y3);
            }
            PathCommand::Close => pb.close(),
        }
    }
    pb.finish()
        .unwrap_or_else(|| tiny_skia::PathBuilder::new().finish().unwrap())
}

/// Convert an r_graphics_engine::Stroke to a tiny_skia::Stroke.
fn stroke_to_skia(stroke: &Stroke) -> tiny_skia::Stroke {
    tiny_skia::Stroke {
        width: stroke.width,
        miter_limit: stroke.miter_limit,
        line_cap: match stroke.cap {
            LineCap::Butt => tiny_skia::LineCap::Butt,
            LineCap::Round => tiny_skia::LineCap::Round,
            LineCap::Square => tiny_skia::LineCap::Square,
        },
        line_join: match stroke.join {
            LineJoin::Miter => tiny_skia::LineJoin::Miter,
            LineJoin::Round => tiny_skia::LineJoin::Round,
            LineJoin::Bevel => tiny_skia::LineJoin::Bevel,
        },
        dash: None,
    }
}

impl RenderPlot for AndroidHeadlessRenderer {
    type Output = Vec<u8>;

    fn new(width: u32, height: u32) -> Self {
        Self::new(width, height)
    }

    fn clear(&mut self, color: Color) {
        if let Some(pixmap) = &mut self.pixmap {
            let pa = tiny_skia::PremultipliedColorU8::from_rgba(color.r, color.g, color.b, color.a)
                .unwrap_or(tiny_skia::PremultipliedColorU8::from_rgba(0, 0, 0, 255).unwrap());
            let skia_color =
                tiny_skia::Color::from_rgba8(pa.red(), pa.green(), pa.blue(), pa.alpha());
            pixmap.fill(skia_color);
        }
    }

    fn draw_path(&mut self, path: &Path) {
        if let Some(pixmap) = &mut self.pixmap {
            let mut paint = tiny_skia::Paint::default();
            paint.set_color_rgba8(path.fill.r, path.fill.g, path.fill.b, path.fill.a);
            paint.anti_alias = path.anti_alias;

            let skia_path = path_to_skia(path);
            pixmap.fill_path(
                &skia_path,
                &paint,
                tiny_skia::FillRule::Winding,
                tiny_skia::Transform::identity(),
                None,
            );

            if path.stroke.width > 0.0 {
                let mut stroke_paint = tiny_skia::Paint::default();
                stroke_paint.set_color_rgba8(
                    path.stroke.color.r,
                    path.stroke.color.g,
                    path.stroke.color.b,
                    path.stroke.color.a,
                );
                stroke_paint.anti_alias = path.anti_alias;

                let skia_stroke = stroke_to_skia(&path.stroke);
                pixmap.stroke_path(
                    &skia_path,
                    &stroke_paint,
                    &skia_stroke,
                    tiny_skia::Transform::identity(),
                    None,
                );
            }
        }
    }

    fn draw_text(&mut self, _text: &str, _pos: Point, _params: &PlotParameters) {
        // Text rendering requires a text shaping library.
        // In a full implementation, use rustybuzz + resvg.
    }

    fn finish(self) -> Self::Output {
        if let Some(pixmap) = self.pixmap {
            let mut buf = Vec::new();
            let mut encoder = png::Encoder::new(&mut buf, self.width, self.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            if let Ok(mut writer) = encoder.write_header() {
                let _ = writer.write_image_data(pixmap.data());
            }
            buf
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
