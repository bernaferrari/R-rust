//! Portable frontend for base R's `rasterImage` operation.
//!
//! This module only decodes R values and computes device-space affine
//! placements. The active graphics device supplies the coordinate mapping and
//! calls `DrawTarget::draw_image`; this keeps R's column-major storage rules
//! independent of a renderer backend.

use crate::eval::attrib_core::{R_DimSymbol, getAttrib};
use crate::library::grdevices::colors::inRGBpar3;
use crate::mainutils::essentials::{arg_by_name_or_position, base_error};
use crate::mainutils::objects::inherits2;
use crate::sexp::{
    accessors::*,
    ffi::{NA_INTEGER, NA_LOGICAL, SEXP, SEXPTYPE},
    globals::R_NilValue,
};
use r_graphics_engine::{DrawTarget, RasterImage};
use std::os::raw::{c_int, c_uint};

/// One vectorized `rasterImage` placement in user coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RasterPlacement {
    pub x_left: f64,
    pub y_bottom: f64,
    pub x_right: f64,
    pub y_top: f64,
    pub angle: f64,
    pub interpolate: bool,
}

/// Decoded raster image and recycled vectorized placements.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RasterImageRequest {
    pub image: RasterImage,
    pub placements: Vec<RasterPlacement>,
}

/// Cached R-level wrapper for the standard `rasterImage` signature. The
/// internal helper is registered separately so this wrapper can own ordinary
/// argument matching and defaults without recursively dispatching itself.
pub(crate) unsafe fn do_raster_image(_: SEXP, _: SEXP, args: SEXP, rho: SEXP) -> SEXP {
    unsafe {
        crate::mainutils::base_wrappers::apply(
            "rasterImage",
            "function(image, xleft, ybottom, xright, ytop, angle=0, interpolate=TRUE, ...) .rport_rasterImage(image=image, xleft=xleft, ybottom=ybottom, xright=xright, ytop=ytop, angle=angle, interpolate=interpolate, ...)",
            args,
            rho,
            false,
        )
    }
}

/// Parse an evaluated `.Internal(rasterImage(...))` argument list.
pub(crate) unsafe fn parse_raster_image(args: SEXP) -> RasterImageRequest {
    unsafe {
        let image = decode_raster_image(arg(args, "image"));
        let x_left = numeric_vector(arg(args, "xleft"), "xleft");
        let y_bottom = numeric_vector(arg(args, "ybottom"), "ybottom");
        let x_right = numeric_vector(arg(args, "xright"), "xright");
        let y_top = numeric_vector(arg(args, "ytop"), "ytop");
        let angle = numeric_vector_or_default(arg(args, "angle"), "angle", 0.0);
        let interpolate = logical_vector_or_default(arg(args, "interpolate"), "interpolate", true);
        let length = [x_left.len(), y_bottom.len(), x_right.len(), y_top.len()]
            .into_iter()
            .max()
            .unwrap_or(0);
        if length == 0 {
            base_error("invalid rasterImage placement");
        }
        let placements = (0..length)
            .map(|i| RasterPlacement {
                x_left: recycled(&x_left, i),
                y_bottom: recycled(&y_bottom, i),
                x_right: recycled(&x_right, i),
                y_top: recycled(&y_top, i),
                angle: recycled(&angle, i),
                interpolate: recycled(&interpolate, i),
            })
            .collect();
        RasterImageRequest { image, placements }
    }
}

/// Draw all vectorized placements after mapping user coordinates to device
/// coordinates. Log axes are rejected by the caller because an affine image
/// transform cannot represent logarithmic distortion.
pub(crate) fn draw_raster_image(
    request: &RasterImageRequest,
    target: &mut dyn DrawTarget,
    map: impl Fn(f64, f64) -> (f64, f64),
) {
    let width = request.image.width() as f64;
    let height = request.image.height() as f64;
    for placement in &request.placements {
        let (x_left, y_bottom) = map(placement.x_left, placement.y_bottom);
        let (x_right, y_top) = map(placement.x_right, placement.y_top);
        if ![x_left, y_bottom, x_right, y_top, placement.angle]
            .into_iter()
            .all(f64::is_finite)
        {
            base_error("non-finite rasterImage placement");
        }
        target.draw_image(
            &request.image,
            affine_transform(
                x_left,
                y_bottom,
                x_right,
                y_top,
                width,
                height,
                placement.angle,
            ),
            placement.interpolate,
        );
    }
}

/// Build an affine transform from device-space bounds. Image row zero is the
/// top edge (`y_top`) and positive image rows move toward `y_bottom`. Rotation
/// is anticlockwise around the bottom-left corner, matching R's contract.
pub(crate) fn affine_transform(
    x_left: f64,
    y_bottom: f64,
    x_right: f64,
    y_top: f64,
    width: f64,
    height: f64,
    angle: f64,
) -> [f64; 6] {
    let sx = (x_right - x_left) / width;
    let sy = (y_bottom - y_top) / height;
    let radians = angle.to_radians();
    let (sin, cos) = radians.sin_cos();
    let a = cos * sx;
    let b = -sin * sx;
    let c = sin * sy;
    let d = cos * sy;
    let e = x_left - c * height;
    let f = y_bottom - d * height;
    [a, b, c, d, e, f]
}

/// Decode a color matrix, grayscale numeric matrix, or nativeRaster integer
/// matrix into top-down row-major RGBA8 pixels.
pub(crate) unsafe fn decode_raster_image(value: SEXP) -> RasterImage {
    unsafe {
        if value.is_null() || value == R_NilValue() {
            base_error("invalid raster image");
        }
        let dimensions = getAttrib(value, R_DimSymbol());
        if TYPEOF(dimensions) != SEXPTYPE::INTSXP || XLENGTH(dimensions) != 2 {
            base_error("raster image must be a two-dimensional matrix");
        }
        let height = *INTEGER(dimensions) as i64;
        let width = *INTEGER(dimensions).add(1) as i64;
        if height <= 0 || width <= 0 {
            base_error("raster image dimensions must be positive");
        }
        let height = height as usize;
        let width = width as usize;
        let length = height
            .checked_mul(width)
            .unwrap_or_else(|| base_error("raster image is too large"));
        if XLENGTH(value) as usize != length {
            base_error("raster image dimensions do not match its data");
        }
        let native =
            TYPEOF(value) == SEXPTYPE::INTSXP && inherits2(value, c"nativeRaster".as_ptr()) != 0;
        let mut pixels = Vec::with_capacity(length * 4);
        for row in 0..height {
            for column in 0..width {
                let index = row + column * height;
                let color = if native {
                    packed_color(*INTEGER(value).add(index) as c_uint)
                } else {
                    color_at(value, index as i64)
                };
                pixels.extend_from_slice(&color);
            }
        }
        RasterImage::from_rgba8(width as u32, height as u32, pixels)
            .unwrap_or_else(|error| base_error(format!("invalid raster image: {error}")))
    }
}

unsafe fn color_at(value: SEXP, index: i64) -> [u8; 4] {
    unsafe {
        match SEXPTYPE(TYPEOF(value)) {
            SEXPTYPE::STRSXP => {
                let color = inRGBpar3(value, index as c_int, 0x00FF_FFFF);
                packed_color(color)
            }
            SEXPTYPE::REALSXP => grayscale(*REAL(value).add(index as usize)),
            SEXPTYPE::INTSXP => {
                let value = *INTEGER(value).add(index as usize);
                if value == NA_INTEGER {
                    [0, 0, 0, 0]
                } else {
                    grayscale(value as f64)
                }
            }
            SEXPTYPE::LGLSXP => {
                let value = *LOGICAL(value).add(index as usize);
                if value == NA_LOGICAL {
                    [0, 0, 0, 0]
                } else {
                    grayscale(value as f64)
                }
            }
            _ => base_error("raster image must be a color, numeric, or nativeRaster matrix"),
        }
    }
}

fn grayscale(value: f64) -> [u8; 4] {
    if !value.is_finite() {
        return [0, 0, 0, 0];
    }
    if !(0.0..=1.0).contains(&value) {
        base_error("numeric raster image values must be between 0 and 1");
    }
    let channel = (value * 255.0).round() as u8;
    [channel, channel, channel, 255]
}

fn packed_color(value: c_uint) -> [u8; 4] {
    [
        value as u8,
        (value >> 8) as u8,
        (value >> 16) as u8,
        (value >> 24) as u8,
    ]
}

unsafe fn arg(args: SEXP, name: &str) -> SEXP {
    unsafe { arg_by_name_or_position(args, &[name], usize::MAX) }
}

unsafe fn numeric_vector(value: SEXP, name: &str) -> Vec<f64> {
    unsafe {
        if value == R_NilValue() || value.is_null() || XLENGTH(value) == 0 {
            base_error(format!("invalid '{name}'"));
        }
        let length = XLENGTH(value) as usize;
        match SEXPTYPE(TYPEOF(value)) {
            SEXPTYPE::REALSXP => (0..length)
                .map(|index| *REAL(value).add(index))
                .collect::<Vec<_>>(),
            SEXPTYPE::INTSXP | SEXPTYPE::LGLSXP => (0..length)
                .map(|index| {
                    let value = *INTEGER(value).add(index);
                    if value == NA_INTEGER {
                        f64::NAN
                    } else {
                        value as f64
                    }
                })
                .collect(),
            _ => base_error(format!("invalid '{name}'")),
        }
        .tap_finite(name)
    }
}

unsafe fn numeric_vector_or_default(value: SEXP, name: &str, default: f64) -> Vec<f64> {
    if unsafe { value == R_NilValue() } {
        vec![default]
    } else {
        unsafe { numeric_vector(value, name) }
    }
}

unsafe fn logical_vector_or_default(value: SEXP, name: &str, default: bool) -> Vec<bool> {
    unsafe {
        if value == R_NilValue() {
            return vec![default];
        }
        if TYPEOF(value) != SEXPTYPE::LGLSXP || XLENGTH(value) == 0 {
            base_error(format!("invalid '{name}'"));
        }
        (0..XLENGTH(value) as usize)
            .map(|index| match *LOGICAL(value).add(index) {
                0 => false,
                1 => true,
                _ => base_error(format!("invalid '{name}'")),
            })
            .collect()
    }
}

trait FiniteValues {
    fn tap_finite(self, name: &str) -> Self;
}

impl FiniteValues for Vec<f64> {
    fn tap_finite(self, name: &str) -> Self {
        if self.iter().any(|value| !value.is_finite()) {
            base_error(format!("invalid '{name}'"));
        }
        self
    }
}

fn recycled<T: Copy>(values: &[T], index: usize) -> T {
    values[index % values.len()]
}

#[cfg(test)]
mod tests {
    use super::affine_transform;

    fn map(transform: [f64; 6], x: f64, y: f64) -> (f64, f64) {
        (
            transform[0] * x + transform[2] * y + transform[4],
            transform[1] * x + transform[3] * y + transform[5],
        )
    }

    #[test]
    fn angle_rotates_anticlockwise_about_bottom_left() {
        let transform = affine_transform(10.0, 30.0, 30.0, 10.0, 2.0, 2.0, 90.0);
        assert_eq!(map(transform, 0.0, 2.0), (10.0, 30.0));
        assert!((map(transform, 2.0, 2.0).0 - 10.0).abs() < 1e-12);
        assert!((map(transform, 2.0, 2.0).1 - 10.0).abs() < 1e-12);
    }
}
