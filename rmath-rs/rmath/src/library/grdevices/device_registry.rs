use std::os::raw::{c_double, c_int};
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
const OPAQUE_WHITE_NATIVE: c_int = 0x00ff_ffff;

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
            haveRaster: 0,
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

pub(crate) fn reset_registry_for_tests() {
    with_registry(|registry| registry.reset());
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
