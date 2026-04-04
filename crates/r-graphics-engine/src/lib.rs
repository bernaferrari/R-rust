//! Graphics engine public interface

#![forbid(unsafe_code)]

use std::vec::Vec;

/// RGBA Color
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// Line cap style
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineCap {
    #[default]
    Butt,
    Round,
    Square,
}

/// Line join style
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineJoin {
    #[default]
    Miter,
    Round,
    Bevel,
}

/// Stroke parameters
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Stroke {
    pub width: f32,
    pub color: Color,
    pub cap: LineCap,
    pub join: LineJoin,
    pub miter_limit: f32,
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
#[derive(Debug, Clone, PartialEq)]
pub enum PathCommand {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    QuadTo(f32, f32, f32, f32),
    CubicTo(f32, f32, f32, f32, f32, f32),
    Close,
}

/// Drawable path
#[derive(Debug, Clone, Default)]
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

    pub fn with_fill(mut self, color: Color) -> Self {
        self.fill = color;
        self
    }

    pub fn with_stroke(mut self, stroke: Stroke) -> Self {
        self.stroke = stroke;
        self
    }
}

/// Plot rendering parameters
#[derive(Debug, Clone, Default)]
pub struct PlotParameters {
    pub font_size: f32,
    pub text_color: Color,
    pub dpi: f32,
}

/// RenderPlot interface
pub trait RenderPlot: Sized {
    type Output;

    /// Create new renderer with given dimensions
    fn new(width: u32, height: u32) -> Self;

    /// Clear canvas with background color
    fn clear(&mut self, background: Color);

    /// Draw path geometry
    fn draw_path(&mut self, path: &Path);

    /// Draw text at position
    fn draw_text(&mut self, text: &str, position: Point, params: &PlotParameters);

    /// Finalize render and return output bytes
    fn finish(self) -> Self::Output;
}
