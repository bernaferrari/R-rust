//! Headless pure Rust graphics backend (tiny-skia + fontdue PNG).
//!
//! Suitable for Android (via r-embed), WASM (wasm32-unknown-unknown), servers,
//! and other targets without a display. Text rendering gracefully degrades if
//! no system fonts are loadable (e.g. WASM no FS). Use set_font() to embed a TTF.
//!
//! The r-embed crate's render_* API uses this for simple plot PNG output.
//! Internal R grDevices on Android uses a separate pure pixel DeviceRegistry.

#![forbid(unsafe_code)]

use std::sync::OnceLock;
use std::vec::Vec;

use r_graphics_engine::{
    Color, LineCap, LineJoin, Path, PathCommand, PlotParameters, Point, RenderPlot, Stroke,
    TextAnchor,
};

/// System font paths to try when loading a default font.
const SYSTEM_FONT_PATHS: &[&str] = &[
    // Linux
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    // macOS
    "/System/Library/Fonts/Geneva.ttf",
    "/System/Library/Fonts/SFNSDisplay.ttf",
    "/Library/Fonts/Arial.ttf",
    // Android
    "/system/fonts/NotoSans-Regular.ttf",
    "/system/fonts/DroidSans.ttf",
    // Windows
    "C:\\Windows\\Fonts\\arial.ttf",
];

/// Wrapper around fontdue::Font to allow Debug derivation.
struct TextFont(fontdue::Font);

impl std::fmt::Debug for TextFont {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextFont").finish_non_exhaustive()
    }
}

/// Globally cached system font so we only hit the filesystem once.
static CACHED_FONT: OnceLock<Option<fontdue::Font>> = OnceLock::new();

fn load_system_font() -> Option<TextFont> {
    let cached = CACHED_FONT.get_or_init(|| {
        for path in SYSTEM_FONT_PATHS {
            if let Ok(data) = std::fs::read(path)
                && let Ok(font) =
                    fontdue::Font::from_bytes(data, fontdue::FontSettings::default())
            {
                return Some(font);
            }
        }
        None
    });
    cached.clone().map(TextFont)
}

/// Android headless plot renderer.
pub struct AndroidHeadlessRenderer {
    width: u32,
    height: u32,
    pixmap: Option<tiny_skia::Pixmap>,
    font: Option<TextFont>,
}

impl std::fmt::Debug for AndroidHeadlessRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AndroidHeadlessRenderer")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("pixmap", &self.pixmap)
            .field("font", &self.font)
            .finish()
    }
}

impl Default for AndroidHeadlessRenderer {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            pixmap: None,
            font: load_system_font(),
        }
    }
}

impl AndroidHeadlessRenderer {
    /// Create a new renderer with the given dimensions.
    pub fn new(width: u32, height: u32) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        Self {
            width,
            height,
            pixmap: tiny_skia::Pixmap::new(width, height),
            font: load_system_font(),
        }
    }

    /// Set a custom font for text rendering.
    pub fn set_font(&mut self, font_data: Vec<u8>) -> Result<(), String> {
        let font = fontdue::Font::from_bytes(font_data, fontdue::FontSettings::default())
            .map_err(|e| format!("Failed to parse font: {:?}", e))?;
        self.font = Some(TextFont(font));
        Ok(())
    }
}

/// Convert an r_graphics_engine::Path to a tiny_skia::Path.
/// Returns `None` when the path has no commands or is degenerate.
fn path_to_skia(path: &Path) -> Option<tiny_skia::Path> {
    if path.commands.is_empty() {
        return None;
    }
    let mut pb = tiny_skia::PathBuilder::new();
    for cmd in &path.commands {
        match cmd {
            PathCommand::MoveTo(x, y) => pb.move_to(*x, *y),
            PathCommand::LineTo(x, y) => pb.line_to(*x, *y),
            PathCommand::QuadTo(x1, y1, x2, y2) => pb.quad_to(*x1, *y1, *x2, *y2),
            PathCommand::CubicTo(x1, y1, x2, y2, x3, y3) => {
                pb.cubic_to(*x1, *y1, *x2, *y2, *x3, *y3);
            }
            PathCommand::ArcTo { x, y, .. } => pb.line_to(*x, *y),
            PathCommand::Close => pb.close(),
        }
    }
    pb.finish()
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
        dash: stroke.dash_pattern.as_ref().and_then(|dp| {
            tiny_skia::StrokeDash::new(dp.intervals.clone(), dp.offset)
        }),
    }
}

impl RenderPlot for AndroidHeadlessRenderer {
    type Output = Vec<u8>;

    fn new(width: u32, height: u32) -> Self {
        Self::new(width, height)
    }

    fn clear(&mut self, color: Color) {
        if let Some(pixmap) = &mut self.pixmap {
            let skia_color = tiny_skia::Color::from_rgba8(color.r, color.g, color.b, color.a);
            pixmap.fill(skia_color);
        }
    }

    fn draw_path(&mut self, path: &Path) {
        if let Some(pixmap) = &mut self.pixmap {
            let Some(skia_path) = path_to_skia(path) else {
                return;
            };

            let mut paint = tiny_skia::Paint::default();
            paint.set_color_rgba8(path.fill.r, path.fill.g, path.fill.b, path.fill.a);
            paint.anti_alias = path.anti_alias;
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

    fn draw_text(&mut self, text: &str, pos: Point, params: &PlotParameters) {
        let Some(pixmap) = &mut self.pixmap else {
            return;
        };
        let Some(TextFont(font)) = &self.font else {
            return;
        };

        let font_size = if params.font_size > 0.0 {
            params.font_size
        } else {
            12.0
        };
        let text_color = if params.text_color.a > 0 {
            params.text_color
        } else {
            Color::WHITE
        };

        let text_width: f32 = text
            .chars()
            .filter(|ch| !ch.is_control())
            .map(|ch| {
                let (metrics, _) = font.rasterize(ch, font_size);
                metrics.advance_width
            })
            .sum();

        let start_x = match params.text_anchor {
            TextAnchor::Start => pos.x,
            TextAnchor::Middle => pos.x - text_width / 2.0,
            TextAnchor::End => pos.x - text_width,
        };

        let pw = pixmap.width() as usize;
        let ph = pixmap.height() as usize;
        let data = pixmap.data_mut();
        let mut x_cursor = start_x;

        for ch in text.chars() {
            if ch == ' ' {
                let (space_metrics, _) = font.rasterize(' ', font_size);
                x_cursor += space_metrics.advance_width;
                continue;
            }
            if ch.is_control() {
                continue;
            }

            let (metrics, bitmap) = font.rasterize(ch, font_size);
            let gw = metrics.width;
            let gh = metrics.height;

            let glyph_base_x = x_cursor + metrics.xmin as f32;
            let glyph_base_y = pos.y + metrics.ymin as f32;

            for row in 0..gh {
                for col in 0..gw {
                    let coverage = bitmap[row * gw + col];
                    if coverage == 0 {
                        continue;
                    }

                    let px = (glyph_base_x + col as f32) as usize;
                    let py = (glyph_base_y + row as f32) as usize;

                    if px >= pw || py >= ph {
                        continue;
                    }

                    let idx = (py * pw + px) * 4;
                    let a = coverage as f32 / 255.0;
                    let inv_a = 1.0 - a;

                    let sr = text_color.r as f32 * a;
                    let sg = text_color.g as f32 * a;
                    let sb = text_color.b as f32 * a;
                    let sa = text_color.a as f32 * a;

                    data[idx] = (sr + data[idx] as f32 * inv_a).min(255.0) as u8;
                    data[idx + 1] = (sg + data[idx + 1] as f32 * inv_a).min(255.0) as u8;
                    data[idx + 2] = (sb + data[idx + 2] as f32 * inv_a).min(255.0) as u8;
                    data[idx + 3] = (sa + data[idx + 3] as f32 * inv_a).min(255.0) as u8;
                }
            }

            x_cursor += metrics.advance_width;
        }
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

    #[test]
    fn test_draw_text_no_panic_without_font() {
        let mut renderer = AndroidHeadlessRenderer {
            width: 200,
            height: 100,
            pixmap: tiny_skia::Pixmap::new(200, 100),
            font: None,
        };
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
        if renderer.font.is_none() {
            return;
        }

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

        let pixmap = renderer.pixmap.as_ref().expect("pixmap");
        assert!(
            pixmap
                .data()
                .chunks_exact(4)
                .any(|rgba| rgba != [255, 255, 255, 255])
        );
    }
}
