//! Graphics engine public interface

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::vec::Vec;

pub mod font;
pub use font::{FontBook, default_font_book};

/// An owned straight-alpha RGBA8 image.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "RasterImageWire")]
pub struct RasterImage {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

/// Errors returned when raster image storage does not match its dimensions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RasterImageError {
    EmptyDimensions,
    DimensionsTooLarge,
    DimensionOverflow,
    PixelDataLength { expected: usize, actual: usize },
}

impl std::fmt::Display for RasterImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyDimensions => f.write_str("raster image dimensions must be nonzero"),
            Self::DimensionsTooLarge => f.write_str("raster image dimensions exceed 65535"),
            Self::DimensionOverflow => f.write_str("raster image dimensions overflow"),
            Self::PixelDataLength { expected, actual } => write!(
                f,
                "raster image requires {expected} RGBA bytes, got {actual}"
            ),
        }
    }
}

impl std::error::Error for RasterImageError {}

#[derive(Deserialize)]
struct RasterImageWire {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl TryFrom<RasterImageWire> for RasterImage {
    type Error = RasterImageError;

    fn try_from(value: RasterImageWire) -> Result<Self, Self::Error> {
        Self::new(value.width, value.height, value.pixels)
    }
}

impl RasterImage {
    /// Create an owned straight-alpha RGBA8 image.
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, RasterImageError> {
        let image = Self {
            width,
            height,
            pixels,
        };
        image.validate()?;
        Ok(image)
    }

    /// Alias documenting the pixel format at call sites.
    pub fn from_rgba8(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, RasterImageError> {
        Self::new(width, height, pixels)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub(crate) fn validate(&self) -> Result<(), RasterImageError> {
        if self.width == 0 || self.height == 0 {
            return Err(RasterImageError::EmptyDimensions);
        }
        if self.width > u16::MAX as u32 || self.height > u16::MAX as u32 {
            return Err(RasterImageError::DimensionsTooLarge);
        }
        let expected = self
            .width
            .checked_mul(self.height)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(RasterImageError::DimensionOverflow)? as usize;
        if self.pixels.len() == expected {
            Ok(())
        } else {
            Err(RasterImageError::PixelDataLength {
                expected,
                actual: self.pixels.len(),
            })
        }
    }
}

/// RGBA Color
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[repr(C)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const WHITE: Self = Self {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    pub const BLUE: Self = Self {
        r: 0,
        g: 0,
        b: 255,
        a: 255,
    };
    pub const RED: Self = Self {
        r: 255,
        g: 0,
        b: 0,
        a: 255,
    };
}

/// 2D Point
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// Line cap style
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LineCap {
    #[default]
    Butt,
    Round,
    Square,
}

/// Line join style
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LineJoin {
    #[default]
    Miter,
    Round,
    Bevel,
}

/// Dash pattern for stroked lines
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[repr(C)]
pub struct DashPattern {
    pub intervals: Vec<f32>,
    pub offset: f32,
}

/// Stroke parameters
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Stroke {
    pub width: f32,
    pub color: Color,
    pub cap: LineCap,
    pub join: LineJoin,
    pub miter_limit: f32,
    pub dash_pattern: Option<DashPattern>,
}

impl Stroke {
    pub fn new(width: f32, color: Color) -> Self {
        Self {
            width,
            color,
            miter_limit: 4.0,
            ..Default::default()
        }
    }
}

/// Path drawing command
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PathCommand {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    QuadTo(f32, f32, f32, f32),
    CubicTo(f32, f32, f32, f32, f32, f32),
    ArcTo { rx: f32, ry: f32, x: f32, y: f32 },
    Close,
}

/// Drawable path
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Path {
    pub commands: Vec<PathCommand>,
    pub fill: Color,
    pub stroke: Stroke,
    pub anti_alias: bool,
}

impl Path {
    pub fn rect(x: f32, y: f32, w: f32, h: f32) -> Self {
        let commands = vec![
            PathCommand::MoveTo(x, y),
            PathCommand::LineTo(x + w, y),
            PathCommand::LineTo(x + w, y + h),
            PathCommand::LineTo(x, y + h),
            PathCommand::Close,
        ];
        Self {
            commands,
            anti_alias: true,
            ..Default::default()
        }
    }

    pub fn circle(cx: f32, cy: f32, r: f32) -> Self {
        // Approximate circle with 4 cubic Bézier segments
        let k = 0.552_284_8_f32 * r;
        Self {
            commands: vec![
                PathCommand::MoveTo(cx + r, cy),
                PathCommand::CubicTo(cx + r, cy + k, cx + k, cy + r, cx, cy + r),
                PathCommand::CubicTo(cx - k, cy + r, cx - r, cy + k, cx - r, cy),
                PathCommand::CubicTo(cx - r, cy - k, cx - k, cy - r, cx, cy - r),
                PathCommand::CubicTo(cx + k, cy - r, cx + r, cy - k, cx + r, cy),
                PathCommand::Close,
            ],
            anti_alias: true,
            ..Default::default()
        }
    }

    pub fn with_fill(mut self, color: Color) -> Self {
        self.fill = color;
        self
    }

    pub fn with_stroke(mut self, stroke: Stroke) -> Self {
        self.stroke = stroke;
        self
    }
}

/// Text anchor/alignment
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TextAnchor {
    #[default]
    Start,
    Middle,
    End,
}

/// Logical font styles. Renderers synthesize weight and slant consistently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FontFace {
    #[default]
    Plain,
    Bold,
    Italic,
    BoldItalic,
}
impl FontFace {
    pub fn is_bold(self) -> bool {
        matches!(self, Self::Bold | Self::BoldItalic)
    }
    pub fn is_italic(self) -> bool {
        matches!(self, Self::Italic | Self::BoldItalic)
    }
    pub fn italic_shear(self) -> f64 {
        if self.is_italic() { -0.2125565617 } else { 0.0 }
    }
    pub fn bold_stroke_width(self, size: f32) -> f64 {
        if self.is_bold() {
            f64::from(size) * 0.03
        } else {
            0.0
        }
    }
}

/// Logical text advance and positive distances above/below the baseline.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TextMetrics {
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
}

/// Plot rendering parameters
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlotParameters {
    #[serde(default)]
    pub font_face: FontFace,
    pub font_size: f32,
    pub text_color: Color,
    pub dpi: f32,
    pub text_anchor: TextAnchor,
    pub text_angle: f32,
}

/// RenderPlot interface
pub trait RenderPlot: Sized {
    type Output;

    fn dimensions(&self) -> (u32, u32) {
        (640, 480)
    }

    /// Create new renderer with given dimensions
    fn new(width: u32, height: u32) -> Self;

    /// Clear canvas with background color
    fn clear(&mut self, background: Color);

    /// Set an optional device-space rectangular clip [left, top, right, bottom].
    fn set_clip(&mut self, _rect: Option<[f32; 4]>) {}

    /// Draw path geometry
    fn draw_path(&mut self, path: &Path);

    /// Draw text at position
    fn draw_text(&mut self, text: &str, position: Point, params: &PlotParameters);
    /// Measure logical text advances using the same font as rendering.
    fn measure_text(&self, text: &str, params: &PlotParameters) -> TextMetrics {
        font::default_font_book().measure_text(text, params.font_size, params.font_face)
    }
    /// Measure advance and visible vertical bounds using the drawing font.
    fn measure_math_text(&self, text: &str, params: &PlotParameters) -> TextMetrics {
        font::default_font_book().measure_math_text(text, params.font_size, params.font_face)
    }

    /// Draw an owned RGBA8 image through an affine device-space transform.
    ///
    /// Backends with native image support can override this. The default
    /// implementation emits one transformed filled quad per pixel.
    fn draw_image(&mut self, image: &RasterImage, transform: [f64; 6], interpolate: bool) {
        draw_raster_image_as_quads(image, transform, interpolate, |path| self.draw_path(path));
    }

    /// Finalize render and return output bytes
    fn finish(self) -> Self::Output;
}

/// Object-safe subset of drawing operations.
///
/// This trait is dyn-compatible so it can be used as `dyn DrawTarget` for
/// pluggable backends (e.g. when a GE device forwards R graphics drawing
/// commands to a RenderPlot implementation).
///
/// `RenderPlot` types automatically implement `DrawTarget` via a blanket impl.
pub trait DrawTarget {
    fn dimensions(&self) -> (u32, u32) {
        (640, 480)
    }
    fn clear(&mut self, background: Color);
    fn set_clip(&mut self, _rect: Option<[f32; 4]>) {}
    fn draw_path(&mut self, path: &Path);
    fn draw_text(&mut self, text: &str, position: Point, params: &PlotParameters);
    /// Measure logical text advances using the same font as rendering.
    fn measure_text(&self, text: &str, params: &PlotParameters) -> TextMetrics {
        font::default_font_book().measure_text(text, params.font_size, params.font_face)
    }
    /// Measure advance and visible vertical bounds using the drawing font.
    fn measure_math_text(&self, text: &str, params: &PlotParameters) -> TextMetrics {
        font::default_font_book().measure_math_text(text, params.font_size, params.font_face)
    }

    fn draw_image(&mut self, image: &RasterImage, transform: [f64; 6], interpolate: bool) {
        draw_raster_image_as_quads(image, transform, interpolate, |path| self.draw_path(path));
    }
}

impl<T: RenderPlot> DrawTarget for T {
    fn measure_math_text(&self, text: &str, params: &PlotParameters) -> TextMetrics {
        <Self as RenderPlot>::measure_math_text(self, text, params)
    }

    fn measure_text(&self, text: &str, params: &PlotParameters) -> TextMetrics {
        <Self as RenderPlot>::measure_text(self, text, params)
    }

    fn dimensions(&self) -> (u32, u32) {
        <Self as RenderPlot>::dimensions(self)
    }
    fn clear(&mut self, background: Color) {
        <Self as RenderPlot>::clear(self, background);
    }
    fn set_clip(&mut self, rect: Option<[f32; 4]>) {
        <Self as RenderPlot>::set_clip(self, rect);
    }
    fn draw_path(&mut self, path: &Path) {
        <Self as RenderPlot>::draw_path(self, path);
    }
    fn draw_text(&mut self, text: &str, position: Point, params: &PlotParameters) {
        <Self as RenderPlot>::draw_text(self, text, position, params);
    }
    fn draw_image(&mut self, image: &RasterImage, transform: [f64; 6], interpolate: bool) {
        <Self as RenderPlot>::draw_image(self, image, transform, interpolate);
    }
}

fn draw_raster_image_as_quads(
    image: &RasterImage,
    transform: [f64; 6],
    interpolate: bool,
    mut draw_path: impl FnMut(&Path),
) {
    for y in 0..image.height {
        for x in 0..image.width {
            let offset = ((y as usize * image.width as usize) + x as usize) * 4;
            let [r, g, b, a] = image.pixels[offset..offset + 4] else {
                return;
            };
            let pixel = |x: f64, y: f64| Point {
                x: (transform[0] * x + transform[2] * y + transform[4]) as f32,
                y: (transform[1] * x + transform[3] * y + transform[5]) as f32,
            };
            let x = x as f64;
            let y = y as f64;
            draw_path(&Path {
                commands: vec![
                    PathCommand::MoveTo(pixel(x, y).x, pixel(x, y).y),
                    PathCommand::LineTo(pixel(x + 1.0, y).x, pixel(x + 1.0, y).y),
                    PathCommand::LineTo(pixel(x + 1.0, y + 1.0).x, pixel(x + 1.0, y + 1.0).y),
                    PathCommand::LineTo(pixel(x, y + 1.0).x, pixel(x, y + 1.0).y),
                    PathCommand::Close,
                ],
                fill: Color { r, g, b, a },
                anti_alias: interpolate,
                ..Default::default()
            });
        }
    }
}

mod scene;

pub use scene::{DisplayList, DrawOperation, Scene};

pub mod math;
