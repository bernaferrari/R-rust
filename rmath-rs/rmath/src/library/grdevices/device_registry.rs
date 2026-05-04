use std::os::raw::{c_double, c_int, c_uint};
use std::ptr;

use crate::attrib_core::{R_DimSymbol, setAttrib};
use crate::sexp::accessors::INTEGER;
use crate::sexp::constructors::Rf_allocVector;
use crate::sexp::ffi::SEXP;
use crate::sexp::ffi::SEXPTYPE;
use crate::sexp::globals::R_NilValue;
use crate::sexp::instance::with_required_current_instance;

const DEFAULT_WIDTH_INCHES: c_double = 7.0;
const DEFAULT_HEIGHT_INCHES: c_double = 7.0;
const DEFAULT_DPI: c_int = 72;
const DEVICE_UNIT: c_int = 0;
const NDC_UNIT: c_int = 1;
const INCHES_UNIT: c_int = 2;
const CM_UNIT: c_int = 3;
const TRANSPARENT_NATIVE: c_int = 0x7fff_ffff;
const OPAQUE_WHITE_NATIVE: c_int = 0x00ff_ffff;
const OPAQUE_BLACK_NATIVE: c_int = 0x0000_0000;

/// Minimal graphics device descriptor used by the headless Rust registry.
///
/// The layout is intentionally small but stable enough for the local grDevices
/// helpers that still expect a GE device pointer.
#[repr(C)]
pub(crate) struct GEDeviceDesc {
    pub label: c_int,
    pub displayListOn: c_int,
    pub lock: c_int,
    pub recordGraphics: c_int,
    pub deviceVersion: c_int,
    pub haveTransparency: c_int,
    pub haveTransparentBg: c_int,
    pub haveRaster: c_int,
    pub haveCapture: c_int,
    pub haveLocator: c_int,
    pub canGenMouseDown: c_int,
    pub canGenMouseMove: c_int,
    pub canGenMouseUp: c_int,
    pub canGenKeybd: c_int,
    pub canGenIdle: c_int,
    pub width: c_double,
    pub height: c_double,
    pub pixel_width: c_int,
    pub pixel_height: c_int,
    pub canvas: Vec<c_int>,
    pub holdflush_level: c_int,
}

impl GEDeviceDesc {
    fn new(label: c_int) -> Self {
        Self {
            label,
            displayListOn: 0,
            lock: 0,
            recordGraphics: 0,
            deviceVersion: 0,
            haveTransparency: 0,
            haveTransparentBg: 0,
            haveRaster: 1,
            haveCapture: 1,
            haveLocator: 0,
            canGenMouseDown: 0,
            canGenMouseMove: 0,
            canGenMouseUp: 0,
            canGenKeybd: 0,
            canGenIdle: 0,
            width: DEFAULT_WIDTH_INCHES,
            height: DEFAULT_HEIGHT_INCHES,
            pixel_width: (DEFAULT_WIDTH_INCHES as c_int) * DEFAULT_DPI,
            pixel_height: (DEFAULT_HEIGHT_INCHES as c_int) * DEFAULT_DPI,
            canvas: vec![
                OPAQUE_WHITE_NATIVE;
                ((DEFAULT_WIDTH_INCHES as c_int)
                    * DEFAULT_DPI
                    * (DEFAULT_HEIGHT_INCHES as c_int)
                    * DEFAULT_DPI) as usize
            ],
            holdflush_level: 0,
        }
    }
}

pub(crate) type pGEDevDesc = *mut GEDeviceDesc;
pub(crate) type pDevDesc = *mut GEDeviceDesc;

pub(crate) struct DeviceRegistry {
    null_device: Box<GEDeviceDesc>,
    devices: Vec<Box<GEDeviceDesc>>,
    current_label: c_int,
    next_label: c_int,
}

impl DeviceRegistry {
    fn new() -> Self {
        Self {
            null_device: Box::new(GEDeviceDesc::new(1)),
            devices: Vec::new(),
            current_label: 1,
            next_label: 2,
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }

    fn no_devices(&self) -> bool {
        self.devices.is_empty()
    }

    fn num_devices(&self) -> c_int {
        1 + self.devices.len() as c_int
    }

    fn current_external(&self) -> c_int {
        self.current_label.max(1)
    }

    fn current_internal(&self) -> c_int {
        self.current_external() - 1
    }

    fn first_real_label(&self) -> Option<c_int> {
        self.devices.first().map(|device| device.label)
    }

    fn last_real_label(&self) -> Option<c_int> {
        self.devices.last().map(|device| device.label)
    }

    fn position_of(&self, label: c_int) -> Option<usize> {
        self.devices.iter().position(|device| device.label == label)
    }

    fn device_ptr(&mut self, label: c_int) -> pGEDevDesc {
        if label <= 1 {
            return self.null_device.as_mut();
        }

        self.devices
            .iter_mut()
            .find(|device| device.label == label)
            .map(|device| device.as_mut() as pGEDevDesc)
            .unwrap_or(ptr::null_mut())
    }

    fn current_ptr(&mut self) -> pGEDevDesc {
        self.device_ptr(self.current_external())
    }

    fn find_device_mut(&mut self, gdd: pGEDevDesc) -> Option<&mut GEDeviceDesc> {
        if gdd.is_null() {
            return None;
        }
        if ptr::eq(self.null_device.as_mut() as pGEDevDesc, gdd) {
            return Some(self.null_device.as_mut());
        }
        for device in &mut self.devices {
            if ptr::eq(device.as_mut() as *mut GEDeviceDesc, gdd) {
                return Some(device.as_mut());
            }
        }
        None
    }

    fn contains_ptr(&mut self, gdd: pGEDevDesc) -> bool {
        self.find_device_mut(gdd).is_some()
    }

    fn open_new_device(&mut self) -> c_int {
        let label = self.next_label;
        self.next_label += 1;
        self.devices.push(Box::new(GEDeviceDesc::new(label)));
        self.current_label = label;
        label
    }

    fn next_label_after(&self, label: c_int) -> Option<c_int> {
        let idx = self.position_of(label)?;
        if self.devices.is_empty() {
            None
        } else if idx + 1 < self.devices.len() {
            Some(self.devices[idx + 1].label)
        } else {
            Some(self.devices[0].label)
        }
    }

    fn prev_label_before(&self, label: c_int) -> Option<c_int> {
        let idx = self.position_of(label)?;
        if self.devices.is_empty() {
            None
        } else if idx == 0 {
            Some(self.devices[self.devices.len() - 1].label)
        } else {
            Some(self.devices[idx - 1].label)
        }
    }

    fn select_external(&mut self, external_label: c_int) -> c_int {
        if self.devices.is_empty() {
            self.open_new_device();
            return 1;
        }

        if external_label == 1 {
            self.open_new_device();
            return 1;
        }

        if external_label > 1 && self.position_of(external_label).is_some() {
            self.current_label = external_label;
            return external_label;
        }

        if let Some(first) = self.first_real_label() {
            self.current_label = first;
            return first;
        }

        self.open_new_device();
        1
    }

    fn next_external(&self, external_label: c_int) -> c_int {
        if self.devices.is_empty() {
            return 1;
        }

        self.next_label_after(external_label)
            .or_else(|| self.first_real_label())
            .unwrap_or(1)
    }

    fn prev_external(&self, external_label: c_int) -> c_int {
        if self.devices.is_empty() {
            return 1;
        }

        self.prev_label_before(external_label)
            .or_else(|| self.last_real_label())
            .unwrap_or(1)
    }

    fn kill_external(&mut self, external_label: c_int) -> c_int {
        if external_label <= 1 {
            return self.current_external();
        }

        let Some(idx) = self.position_of(external_label) else {
            return self.current_external();
        };

        let next_current = if self.devices.len() == 1 {
            1
        } else if idx + 1 < self.devices.len() {
            self.devices[idx + 1].label
        } else {
            self.devices[0].label
        };

        self.devices.remove(idx);

        if self.current_label == external_label {
            self.current_label = next_current;
        }

        self.current_external()
    }
}

impl Default for DeviceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn with_registry<R>(f: impl FnOnce(&mut DeviceRegistry) -> R) -> R {
    with_required_current_instance(|instance| f(&mut instance.graphics_device_registry))
}

fn with_device_mut<R>(gdd: pGEDevDesc, f: impl FnOnce(&mut GEDeviceDesc) -> R) -> Option<R> {
    if gdd.is_null() {
        return None;
    }
    with_registry(|registry| registry.find_device_mut(gdd).map(f))
}

pub(crate) fn reset_registry_for_tests() {
    with_registry(|registry| registry.reset());
}

pub(crate) fn is_registered_device(gdd: pGEDevDesc) -> bool {
    if gdd.is_null() {
        return false;
    }
    with_registry(|registry| registry.contains_ptr(gdd))
}

fn round_pixel(value: c_double) -> Option<c_int> {
    value.is_finite().then(|| value.round() as c_int)
}

fn clamp_endpoint(value: c_int, max: c_int) -> c_int {
    value.clamp(0, max.saturating_sub(1))
}

fn native_color_from_u32(color: c_uint) -> c_int {
    let color = color as c_int;
    if color == TRANSPARENT_NATIVE {
        TRANSPARENT_NATIVE
    } else {
        color & OPAQUE_WHITE_NATIVE
    }
}

fn set_pixel(device: &mut GEDeviceDesc, x: c_int, y: c_int, color: c_int) {
    if color == TRANSPARENT_NATIVE
        || x < 0
        || y < 0
        || x >= device.pixel_width
        || y >= device.pixel_height
    {
        return;
    }
    let index = y as usize * device.pixel_width as usize + x as usize;
    if let Some(pixel) = device.canvas.get_mut(index) {
        *pixel = color;
    }
}

fn draw_line_pixels(
    device: &mut GEDeviceDesc,
    x0: c_int,
    y0: c_int,
    x1: c_int,
    y1: c_int,
    color: c_int,
) {
    let mut x0 = x0;
    let mut y0 = y0;
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        set_pixel(device, x0, y0, color);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

fn fill_rect_pixels(
    device: &mut GEDeviceDesc,
    x0: c_int,
    y0: c_int,
    x1: c_int,
    y1: c_int,
    color: c_int,
) {
    if color == TRANSPARENT_NATIVE || device.pixel_width <= 0 || device.pixel_height <= 0 {
        return;
    }
    let left = clamp_endpoint(x0.min(x1), device.pixel_width);
    let right = clamp_endpoint(x0.max(x1), device.pixel_width);
    let top = clamp_endpoint(y0.min(y1), device.pixel_height);
    let bottom = clamp_endpoint(y0.max(y1), device.pixel_height);
    for y in top..=bottom {
        let row = y as usize * device.pixel_width as usize;
        for x in left..=right {
            device.canvas[row + x as usize] = color;
        }
    }
}

fn fill_polygon_pixels(device: &mut GEDeviceDesc, points: &[(c_int, c_int)], color: c_int) {
    if color == TRANSPARENT_NATIVE || points.len() < 3 {
        return;
    }
    let min_y = points
        .iter()
        .map(|(_, y)| *y)
        .min()
        .unwrap_or(0)
        .clamp(0, device.pixel_height.saturating_sub(1));
    let max_y = points
        .iter()
        .map(|(_, y)| *y)
        .max()
        .unwrap_or(0)
        .clamp(0, device.pixel_height.saturating_sub(1));

    for y in min_y..=max_y {
        let scan_y = y as c_double + 0.5;
        let mut crossings = Vec::new();
        for idx in 0..points.len() {
            let (x1, y1) = points[idx];
            let (x2, y2) = points[(idx + 1) % points.len()];
            let y1f = y1 as c_double;
            let y2f = y2 as c_double;
            if (y1f <= scan_y && y2f > scan_y) || (y2f <= scan_y && y1f > scan_y) {
                let x = x1 as c_double + (scan_y - y1f) * (x2 - x1) as c_double / (y2f - y1f);
                crossings.push(x.round() as c_int);
            }
        }
        crossings.sort_unstable();
        for pair in crossings.chunks_exact(2) {
            let left = clamp_endpoint(pair[0], device.pixel_width);
            let right = clamp_endpoint(pair[1], device.pixel_width);
            for x in left.min(right)..=left.max(right) {
                set_pixel(device, x, y, color);
            }
        }
    }
}

pub(crate) fn from_device_x(value: c_double, to: c_int, gdd: pGEDevDesc) -> Option<c_double> {
    with_device_mut(gdd, |device| match to {
        DEVICE_UNIT => value,
        NDC_UNIT => value / device.pixel_width as c_double,
        INCHES_UNIT => value / DEFAULT_DPI as c_double,
        CM_UNIT => value / DEFAULT_DPI as c_double * 2.54,
        _ => value,
    })
}

pub(crate) fn to_device_x(value: c_double, from: c_int, gdd: pGEDevDesc) -> Option<c_double> {
    with_device_mut(gdd, |device| match from {
        DEVICE_UNIT => value,
        NDC_UNIT => value * device.pixel_width as c_double,
        INCHES_UNIT => value * DEFAULT_DPI as c_double,
        CM_UNIT => value / 2.54 * DEFAULT_DPI as c_double,
        _ => value,
    })
}

pub(crate) fn from_device_y(value: c_double, to: c_int, gdd: pGEDevDesc) -> Option<c_double> {
    with_device_mut(gdd, |device| {
        let from_top = device.pixel_height as c_double - value;
        match to {
            DEVICE_UNIT => value,
            NDC_UNIT => from_top / device.pixel_height as c_double,
            INCHES_UNIT => from_top / DEFAULT_DPI as c_double,
            CM_UNIT => from_top / DEFAULT_DPI as c_double * 2.54,
            _ => value,
        }
    })
}

pub(crate) fn to_device_y(value: c_double, from: c_int, gdd: pGEDevDesc) -> Option<c_double> {
    with_device_mut(gdd, |device| match from {
        DEVICE_UNIT => value,
        NDC_UNIT => device.pixel_height as c_double - value * device.pixel_height as c_double,
        INCHES_UNIT => device.pixel_height as c_double - value * DEFAULT_DPI as c_double,
        CM_UNIT => device.pixel_height as c_double - value / 2.54 * DEFAULT_DPI as c_double,
        _ => value,
    })
}

pub(crate) fn from_device_width(value: c_double, to: c_int, gdd: pGEDevDesc) -> Option<c_double> {
    with_device_mut(gdd, |device| match to {
        DEVICE_UNIT => value,
        NDC_UNIT => value / device.pixel_width as c_double,
        INCHES_UNIT => value / DEFAULT_DPI as c_double,
        CM_UNIT => value / DEFAULT_DPI as c_double * 2.54,
        _ => value,
    })
}

pub(crate) fn to_device_width(value: c_double, from: c_int, gdd: pGEDevDesc) -> Option<c_double> {
    to_device_x(value, from, gdd)
}

pub(crate) fn from_device_height(value: c_double, to: c_int, gdd: pGEDevDesc) -> Option<c_double> {
    with_device_mut(gdd, |device| match to {
        DEVICE_UNIT => value,
        NDC_UNIT => value / device.pixel_height as c_double,
        INCHES_UNIT => value / DEFAULT_DPI as c_double,
        CM_UNIT => value / DEFAULT_DPI as c_double * 2.54,
        _ => value,
    })
}

pub(crate) fn to_device_height(value: c_double, from: c_int, gdd: pGEDevDesc) -> Option<c_double> {
    with_device_mut(gdd, |device| match from {
        DEVICE_UNIT => value,
        NDC_UNIT => value * device.pixel_height as c_double,
        INCHES_UNIT => value * DEFAULT_DPI as c_double,
        CM_UNIT => value / 2.54 * DEFAULT_DPI as c_double,
        _ => value,
    })
}

pub(crate) fn new_page(gdd: pGEDevDesc) -> bool {
    with_device_mut(gdd, |device| {
        device.canvas.fill(OPAQUE_WHITE_NATIVE);
        true
    })
    .unwrap_or(false)
}

pub(crate) fn mode(gdd: pGEDevDesc, _mode: c_int) -> bool {
    is_registered_device(gdd)
}

pub(crate) fn set_clip(
    gdd: pGEDevDesc,
    _x1: c_double,
    _y1: c_double,
    _x2: c_double,
    _y2: c_double,
) -> bool {
    is_registered_device(gdd)
}

pub(crate) fn draw_line(
    gdd: pGEDevDesc,
    x0: c_double,
    y0: c_double,
    x1: c_double,
    y1: c_double,
) -> bool {
    with_device_mut(gdd, |device| {
        let (Some(x0), Some(y0), Some(x1), Some(y1)) = (
            round_pixel(x0),
            round_pixel(y0),
            round_pixel(x1),
            round_pixel(y1),
        ) else {
            return true;
        };
        draw_line_pixels(device, x0, y0, x1, y1, OPAQUE_BLACK_NATIVE);
        true
    })
    .unwrap_or(false)
}

pub(crate) fn draw_polyline(gdd: pGEDevDesc, points: &[(c_double, c_double)]) -> bool {
    with_device_mut(gdd, |device| {
        for pair in points.windows(2) {
            let (Some(x0), Some(y0), Some(x1), Some(y1)) = (
                round_pixel(pair[0].0),
                round_pixel(pair[0].1),
                round_pixel(pair[1].0),
                round_pixel(pair[1].1),
            ) else {
                continue;
            };
            draw_line_pixels(device, x0, y0, x1, y1, OPAQUE_BLACK_NATIVE);
        }
        true
    })
    .unwrap_or(false)
}

pub(crate) fn draw_polygon(gdd: pGEDevDesc, points: &[(c_double, c_double)]) -> bool {
    with_device_mut(gdd, |device| {
        let points: Vec<_> = points
            .iter()
            .filter_map(|(x, y)| Some((round_pixel(*x)?, round_pixel(*y)?)))
            .collect();
        fill_polygon_pixels(device, &points, OPAQUE_BLACK_NATIVE);
        for idx in 0..points.len() {
            let (x0, y0) = points[idx];
            let (x1, y1) = points[(idx + 1) % points.len()];
            draw_line_pixels(device, x0, y0, x1, y1, OPAQUE_BLACK_NATIVE);
        }
        true
    })
    .unwrap_or(false)
}

pub(crate) fn draw_rect(
    gdd: pGEDevDesc,
    x0: c_double,
    y0: c_double,
    x1: c_double,
    y1: c_double,
) -> bool {
    with_device_mut(gdd, |device| {
        let (Some(x0), Some(y0), Some(x1), Some(y1)) = (
            round_pixel(x0),
            round_pixel(y0),
            round_pixel(x1),
            round_pixel(y1),
        ) else {
            return true;
        };
        fill_rect_pixels(device, x0, y0, x1, y1, OPAQUE_BLACK_NATIVE);
        true
    })
    .unwrap_or(false)
}

pub(crate) fn draw_circle(gdd: pGEDevDesc, x: c_double, y: c_double, radius: c_double) -> bool {
    with_device_mut(gdd, |device| {
        let (Some(cx), Some(cy)) = (round_pixel(x), round_pixel(y)) else {
            return true;
        };
        if !radius.is_finite() || radius <= 0.0 {
            return true;
        }
        let radius = radius.round().max(1.0) as c_int;
        let r2 = radius * radius;
        for py in (cy - radius)..=(cy + radius) {
            for px in (cx - radius)..=(cx + radius) {
                let dx = px - cx;
                let dy = py - cy;
                if dx * dx + dy * dy <= r2 {
                    set_pixel(device, px, py, OPAQUE_BLACK_NATIVE);
                }
            }
        }
        true
    })
    .unwrap_or(false)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_raster(
    gdd: pGEDevDesc,
    raster: *mut c_uint,
    w: c_int,
    h: c_int,
    x: c_double,
    y: c_double,
    width: c_double,
    height: c_double,
) -> bool {
    with_device_mut(gdd, |device| {
        if raster.is_null() || w <= 0 || h <= 0 || width == 0.0 || height == 0.0 {
            return true;
        }
        let (Some(x0), Some(y0), Some(x1), Some(y1)) = (
            round_pixel(x),
            round_pixel(y),
            round_pixel(x + width),
            round_pixel(y + height),
        ) else {
            return true;
        };
        let left = clamp_endpoint(x0.min(x1), device.pixel_width);
        let right = clamp_endpoint(x0.max(x1), device.pixel_width);
        let top = clamp_endpoint(y0.min(y1), device.pixel_height);
        let bottom = clamp_endpoint(y0.max(y1), device.pixel_height);
        for dy in top..=bottom {
            let target_y = if bottom == top {
                0
            } else {
                ((dy - top) as i64 * h as i64 / (bottom - top + 1) as i64) as c_int
            }
            .clamp(0, h - 1);
            for dx in left..=right {
                let target_x = if right == left {
                    0
                } else {
                    ((dx - left) as i64 * w as i64 / (right - left + 1) as i64) as c_int
                }
                .clamp(0, w - 1);
                let source = unsafe { *raster.add((target_y * w + target_x) as usize) };
                set_pixel(device, dx, dy, native_color_from_u32(source));
            }
        }
        true
    })
    .unwrap_or(false)
}

#[unsafe(export_name = "rmath_GEcurrentDevice")]
pub unsafe extern "C" fn GEcurrentDevice() -> pGEDevDesc {
    with_registry(|registry| registry.current_ptr())
}

pub unsafe extern "C" fn GEgetDevice(dev: c_int) -> pGEDevDesc {
    with_registry(|registry| registry.device_ptr(dev + 1))
}

pub unsafe extern "C" fn curDevice() -> c_int {
    with_registry(|registry| registry.current_internal())
}

pub unsafe extern "C" fn nextDevice(dev: c_int) -> c_int {
    with_registry(|registry| registry.next_external(dev + 1) - 1)
}

pub unsafe extern "C" fn prevDevice(dev: c_int) -> c_int {
    with_registry(|registry| registry.prev_external(dev + 1) - 1)
}

pub unsafe extern "C" fn selectDevice(dev: c_int) -> c_int {
    with_registry(|registry| registry.select_external(dev + 1) - 1)
}

pub unsafe extern "C" fn killDevice(dev: c_int) {
    let _ = with_registry(|registry| registry.kill_external(dev + 1));
}

pub unsafe extern "C" fn NoDevices() -> c_int {
    with_registry(|registry| registry.no_devices() as c_int)
}

pub unsafe extern "C" fn NumDevices() -> c_int {
    with_registry(|registry| registry.num_devices())
}

pub unsafe extern "C" fn GEinitDisplayList(_gdd: pGEDevDesc) {}

pub unsafe extern "C" fn GEcopyDisplayList(_devnum: c_int) {}

pub unsafe extern "C" fn GECap(_gdd: pGEDevDesc) -> SEXP {
    unsafe {
        if _gdd.is_null() {
            return R_NilValue();
        }
        let width = (*_gdd).pixel_width;
        let height = (*_gdd).pixel_height;
        if width <= 0 || height <= 0 {
            return R_NilValue();
        }
        let len = width.saturating_mul(height);
        let raster = Rf_allocVector(SEXPTYPE::INTSXP, len);
        let out = INTEGER(raster);
        for (index, pixel) in (*_gdd).canvas.iter().take(len as usize).enumerate() {
            *out.add(index) = *pixel;
        }

        let dim = Rf_allocVector(SEXPTYPE::INTSXP, 2);
        *INTEGER(dim).add(0) = height;
        *INTEGER(dim).add(1) = width;
        setAttrib(raster, R_DimSymbol(), dim);
        raster
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::session::RSession;
    use std::sync::{Mutex, MutexGuard};

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    struct DeviceRegistryTestGuard {
        _lock: MutexGuard<'static, ()>,
        _session: RSession,
    }

    fn reset_registry() -> DeviceRegistryTestGuard {
        let lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let session = RSession::new();
        reset_registry_for_tests();
        DeviceRegistryTestGuard {
            _lock: lock,
            _session: session,
        }
    }

    #[test]
    fn starts_on_null_device_only() {
        let _guard = reset_registry();
        unsafe {
            assert_eq!(NoDevices(), 1);
            assert_eq!(NumDevices(), 1);
            assert_eq!(curDevice(), 0);
            assert!(!GEcurrentDevice().is_null());
            assert_eq!((*GEcurrentDevice()).label, 1);
            assert_eq!(nextDevice(0), 0);
            assert_eq!(prevDevice(0), 0);
        }
    }

    #[test]
    fn open_select_and_wrap_devices() {
        let _guard = reset_registry();
        with_registry(|registry| {
            assert_eq!(registry.open_new_device(), 2);
            assert_eq!(registry.open_new_device(), 3);
            assert_eq!(registry.open_new_device(), 4);
        });

        unsafe {
            assert_eq!(NoDevices(), 0);
            assert_eq!(NumDevices(), 4);
            assert_eq!(curDevice(), 3);
            assert_eq!(nextDevice(0), 1);
            assert_eq!(nextDevice(1), 2);
            assert_eq!(nextDevice(2), 3);
            assert_eq!(nextDevice(3), 1);
            assert_eq!(nextDevice(98), 1);
            assert_eq!(prevDevice(0), 3);
            assert_eq!(prevDevice(1), 3);
            assert_eq!(prevDevice(2), 1);
            assert_eq!(prevDevice(3), 2);
            assert_eq!(prevDevice(98), 3);
            assert!(!GEgetDevice(0).is_null());
        }

        unsafe {
            assert_eq!(selectDevice(0), 0);
            assert_eq!(curDevice(), 4);
            assert_eq!(selectDevice(98), 1);
            assert_eq!(curDevice(), 1);
            assert_eq!(selectDevice(1), 1);
            assert_eq!(curDevice(), 1);
            assert_eq!(selectDevice(0), 0);
            assert_eq!(curDevice(), 5);
        }
    }

    #[test]
    fn kill_current_device_advances_to_next_or_null() {
        let _guard = reset_registry();
        with_registry(|registry| {
            assert_eq!(registry.open_new_device(), 2);
            assert_eq!(registry.open_new_device(), 3);
            assert_eq!(registry.open_new_device(), 4);
        });

        unsafe {
            assert_eq!(curDevice(), 3);
            killDevice(3);
            assert_eq!(curDevice(), 1);
            assert!(GEgetDevice(3).is_null());
            assert!(!GEgetDevice(2).is_null());

            killDevice(1);
            assert_eq!(curDevice(), 2);
            assert!(!GEgetDevice(2).is_null());

            killDevice(2);
            assert_eq!(curDevice(), 0);
            assert!(GEgetDevice(2).is_null());

            killDevice(0);
            assert_eq!(curDevice(), 0);
        }
    }

    #[test]
    fn device_registry_is_session_local_on_same_thread() {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let left = RSession::new();
        let right = RSession::new();

        left.with_protected(|| {
            with_registry(|registry| {
                assert_eq!(registry.open_new_device(), 2);
                assert_eq!(registry.open_new_device(), 3);
            });
            unsafe {
                assert_eq!(NoDevices(), 0);
                assert_eq!(NumDevices(), 3);
                assert_eq!(curDevice(), 2);
            }
        });

        right.with_protected(|| unsafe {
            assert_eq!(NoDevices(), 1);
            assert_eq!(NumDevices(), 1);
            assert_eq!(curDevice(), 0);
        });

        left.with_protected(|| unsafe {
            assert_eq!(NoDevices(), 0);
            assert_eq!(NumDevices(), 3);
            assert_eq!(curDevice(), 2);
        });
    }
}
