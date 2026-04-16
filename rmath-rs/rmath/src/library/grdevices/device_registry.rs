#![allow(non_snake_case, non_camel_case_types, dead_code)]

use std::os::raw::{c_double, c_int};
use std::ptr;
use std::sync::{Mutex, OnceLock};

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
            haveLocator: 1,
            canGenMouseDown: 0,
            canGenMouseMove: 0,
            canGenMouseUp: 0,
            canGenKeybd: 0,
            canGenIdle: 0,
            width: 0.0,
            height: 0.0,
            holdflush_level: 0,
        }
    }
}

pub(crate) type pGEDevDesc = *mut GEDeviceDesc;
pub(crate) type pDevDesc = *mut GEDeviceDesc;

struct DeviceRegistry {
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

static DEVICE_REGISTRY: OnceLock<Mutex<DeviceRegistry>> = OnceLock::new();

fn registry() -> &'static Mutex<DeviceRegistry> {
    DEVICE_REGISTRY.get_or_init(|| Mutex::new(DeviceRegistry::new()))
}

fn with_registry<R>(f: impl FnOnce(&mut DeviceRegistry) -> R) -> R {
    let mut guard = registry().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard)
}

pub(crate) fn reset_registry_for_tests() {
    with_registry(|registry| registry.reset());
}

pub(crate) unsafe fn GEcurrentDevice() -> pGEDevDesc {
    with_registry(|registry| registry.current_ptr())
}

pub(crate) unsafe fn GEgetDevice(dev: c_int) -> pGEDevDesc {
    with_registry(|registry| registry.device_ptr(dev + 1))
}

pub(crate) unsafe fn curDevice() -> c_int {
    with_registry(|registry| registry.current_internal())
}

pub(crate) unsafe fn nextDevice(dev: c_int) -> c_int {
    with_registry(|registry| registry.next_external(dev + 1) - 1)
}

pub(crate) unsafe fn prevDevice(dev: c_int) -> c_int {
    with_registry(|registry| registry.prev_external(dev + 1) - 1)
}

pub(crate) unsafe fn selectDevice(dev: c_int) -> c_int {
    with_registry(|registry| registry.select_external(dev + 1) - 1)
}

pub(crate) unsafe fn killDevice(dev: c_int) {
    let _ = with_registry(|registry| registry.kill_external(dev + 1));
}

pub(crate) unsafe fn NoDevices() -> c_int {
    with_registry(|registry| registry.no_devices() as c_int)
}

pub(crate) unsafe fn NumDevices() -> c_int {
    with_registry(|registry| registry.num_devices())
}

#[unsafe(no_mangle)]
pub(crate) unsafe fn GEinitDisplayList(_gdd: pGEDevDesc) {}

#[unsafe(no_mangle)]
pub(crate) unsafe fn GEcopyDisplayList(_devnum: c_int) {}

#[unsafe(no_mangle)]
pub(crate) unsafe fn GECap(_gdd: pGEDevDesc) -> *mut std::ffi::c_void {
    ptr::null_mut()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reset_registry() {
        reset_registry_for_tests();
    }

    #[test]
    fn starts_on_null_device_only() {
        reset_registry();
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
        reset_registry();
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
        reset_registry();
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
}
