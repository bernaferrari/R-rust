use super::*;

/// Physical location cleanup belongs to the original runtime even after close.
pub(crate) struct ToplevelExprNoGuard {
    owner: crate::sexp::owner::OwnerPin,
}
impl ToplevelExprNoGuard {
    pub(crate) fn new(session: &RSession) -> Self {
        Self {
            owner: session
                .owner_token()
                .expect("active script owner")
                .weak_owner()
                .expect("managed script owner")
                .pin()
                .expect("live script owner"),
        }
    }

    pub(crate) fn require_active(&self) -> RResult<()> {
        self.owner.require_live().map_err(|error| REvalError {
            message: error.to_string(),
        })?;
        if crate::sexp::instance::current_instance_ptr() != Some(self.owner.as_ptr()) {
            return Err(REvalError {
                message: crate::sexp::object::SexpError::OwnerNotActive.to_string(),
            });
        }
        Ok(())
    }

    pub(crate) fn run_checked<T>(&self, operation: impl FnOnce() -> T) -> RResult<T> {
        self.require_active()?;
        let outcome = catch_unwind(AssertUnwindSafe(operation));
        self.require_active()?;
        match outcome {
            Ok(value) => Ok(value),
            Err(payload) => match print_panic_r_error(payload.as_ref()) {
                Some(error) => Err(error),
                None => std::panic::resume_unwind(payload),
            },
        }
    }
}
impl Drop for ToplevelExprNoGuard {
    fn drop(&mut self) {
        unsafe {
            (*self.owner.as_ptr()).error_state.toplevel_expr_no = 0;
        }
    }
}

pub(crate) struct CurrentInstanceGuard {
    transfer_scope: Option<crate::sexp::transfer::TransferScopeGuard>,
    /// Keep actual runtime bytes allocated through callback and restoration.
    _active_pin: Option<crate::sexp::owner::OwnerPin>,
    _previous_pin: Option<crate::sexp::owner::OwnerPin>,
    previous: Option<*mut RInstance>,
    previous_liveness: Option<crate::sexp::instance::InstanceLiveness>,
    previous_rng: Option<*mut rmath_nmath::RngState>,
    previous_state: Option<*mut rmath_nmath::MathState>,
}

impl CurrentInstanceGuard {
    /// Suspend ambient dispatch while constructing an independent runtime.
    /// Retain the original previous allocation through bootstrap callbacks,
    /// and restore only its still-live capability on success or unwind.
    pub(crate) fn detached() -> Self {
        let previous = crate::sexp::instance::current_instance_ptr();
        let previous_pin = previous.and_then(|pointer| unsafe {
            (*pointer)
                .runtime_owner
                .as_ref()
                .and_then(|owner| owner.pin().ok())
        });
        let previous_liveness =
            previous.map(|pointer| unsafe { crate::sexp::instance::instance_liveness(pointer) });
        let previous_rng = unsafe { rmath_nmath::rng::swap_rng(None) };
        let previous_state = rmath_nmath::state::take_state();
        unsafe { replace_current_instance(None) };
        Self {
            transfer_scope: None,
            _active_pin: None,
            _previous_pin: previous_pin,
            previous,
            previous_liveness,
            previous_rng,
            previous_state,
        }
    }

    pub(crate) unsafe fn new(instance: *mut RInstance) -> Self {
        let owner = unsafe { (*instance).runtime_owner.clone() };
        let active_pin = owner
            .clone()
            .map(|owner| owner.pin().expect("cannot activate a closed R owner"));
        let previous = crate::sexp::instance::current_instance_ptr();
        let previous_pin = previous.and_then(|pointer| unsafe {
            (*pointer)
                .runtime_owner
                .as_ref()
                .and_then(|owner| owner.pin().ok())
        });
        let previous_liveness =
            previous.map(|owner| unsafe { crate::sexp::instance::instance_liveness(owner) });
        unsafe { replace_current_instance(Some(instance)) };
        // Scope the nmath RNG to this session for the duration of the
        // activation, mirroring the instance swap above: session-owned
        // streams must not leak across concurrently-live sessions.
        let previous_rng = unsafe {
            rmath_nmath::rng::swap_rng(Some(
                &mut (*instance).rng_state as *mut rmath_nmath::RngState,
            ))
        };
        // Scope the nmath sampler state (rgamma/beta/... caches) the same
        // way; without this a detached session's sampler state resets on
        // every access and stateful algorithms like rgamma's GD loop fail.
        let previous_state = unsafe {
            rmath_nmath::state::replace_state(
                &mut (*instance).math_state as *mut rmath_nmath::MathState,
            )
        };
        let mut guard = CurrentInstanceGuard {
            transfer_scope: None,
            _active_pin: active_pin,
            _previous_pin: previous_pin,
            previous,
            previous_liveness,
            previous_rng,
            previous_state,
        };
        guard.transfer_scope = owner.map(|owner| {
            crate::sexp::transfer::TransferScopeGuard::enter(owner)
                .expect("cannot enter closed transfer scope")
        });
        guard
    }
}

/// Scope a translated operation to its explicit owner and restore instance,
/// RNG and numerical state on every exit, including an R error unwind.
///
/// # Safety
/// The owner is live on entry; no whole-instance or payload borrow may
/// overlap reentry. Callers using it after a callback must retain a session
/// borrow or check its teardown identity. The guard restores prior state only
/// while that prior owner remains live.
pub(crate) unsafe fn with_instance_active<T>(instance: *mut RInstance, f: impl FnOnce() -> T) -> T {
    let _guard = unsafe { CurrentInstanceGuard::new(instance) };
    f()
}

impl Drop for CurrentInstanceGuard {
    fn drop(&mut self) {
        drop(self.transfer_scope.take());
        // Callback code can destroy an ambient owner stored in thread-local
        // user state. Never reinstall any of that owner's now dangling state.
        let previous_alive = self
            .previous_liveness
            .as_ref()
            .is_none_or(|owner| owner.is_live());
        unsafe {
            replace_current_instance(if previous_alive { self.previous } else { None });
            rmath_nmath::rng::swap_rng(if previous_alive {
                self.previous_rng
            } else {
                None
            });
            rmath_nmath::state::restore_state(if previous_alive {
                self.previous_state
            } else {
                None
            });
        }
    }
}

/// Records owned commands while forwarding to the live device. No borrow of
/// the recording survives a draw call, so recordPlot can snapshot it reentrantly.
#[cfg(feature = "renderplot-device")]
pub(crate) struct RecordingTarget<'a> {
    target: &'a mut dyn r_graphics_engine::DrawTarget,
    recording: std::rc::Rc<std::cell::RefCell<r_graphics_engine::Scene>>,
}
#[cfg(feature = "renderplot-device")]
impl r_graphics_engine::DrawTarget for RecordingTarget<'_> {
    pub(crate) fn dimensions(&self) -> (u32, u32) {
        self.target.dimensions()
    }
    pub(crate) fn measure_text(
        &self,
        text: &str,
        params: &r_graphics_engine::PlotParameters,
    ) -> r_graphics_engine::TextMetrics {
        self.target.measure_text(text, params)
    }
    pub(crate) fn measure_math_text(
        &self,
        text: &str,
        params: &r_graphics_engine::PlotParameters,
    ) -> r_graphics_engine::TextMetrics {
        self.target.measure_math_text(text, params)
    }
    pub(crate) fn clear(&mut self, color: r_graphics_engine::Color) {
        self.recording.borrow_mut().clear(color);
        self.target.clear(color);
    }
    pub(crate) fn set_clip(&mut self, clip: Option<[f32; 4]>) {
        self.recording.borrow_mut().set_clip(clip);
        self.target.set_clip(clip);
    }
    pub(crate) fn draw_path(&mut self, path: &r_graphics_engine::Path) {
        self.recording.borrow_mut().draw_path(path);
        self.target.draw_path(path);
    }
    pub(crate) fn draw_image(
        &mut self,
        image: &r_graphics_engine::RasterImage,
        transform: [f64; 6],
        interpolate: bool,
    ) {
        self.recording
            .borrow_mut()
            .draw_image(image, transform, interpolate);
        self.target.draw_image(image, transform, interpolate);
    }
    pub(crate) fn draw_text(
        &mut self,
        text: &str,
        point: r_graphics_engine::Point,
        params: &r_graphics_engine::PlotParameters,
    ) {
        self.recording.borrow_mut().draw_text(text, point, params);
        self.target.draw_text(text, point, params);
    }
}

#[cfg(feature = "renderplot-device")]
pub(crate) struct RenderPlotBackendGuard {
    instance: *mut RInstance,
    pin: Option<crate::sexp::owner::OwnerPin>,
    availability: crate::sexp::instance::InstanceLiveness,
    previous: Option<*mut dyn r_graphics_engine::DrawTarget>,
}

#[cfg(feature = "renderplot-device")]
impl RenderPlotBackendGuard {
    pub(crate) fn install<'a>(
        instance: *mut RInstance,
        backend: *mut (dyn r_graphics_engine::DrawTarget + 'a),
    ) -> Self {
        let pin = unsafe { (*instance).runtime_owner.clone() }
            .map(|owner| owner.pin().expect("live RenderPlot owner"));
        let availability = unsafe { crate::sexp::instance::instance_liveness(instance) };
        // SAFETY: lifetime-erasing the backend pointer to 'static for storage
        // in the instance slot. This is sound because the guard's Drop
        // restores the previous slot value before the caller's 'a borrow ends,
        // so the erased pointer can never be observed after it dangles.
        #[allow(clippy::transmute_ptr_to_ptr)] // fat-pointer lifetime erasure; `as` cannot do this
        let erased: *mut (dyn r_graphics_engine::DrawTarget + 'static) =
            unsafe { std::mem::transmute(backend) };
        // P2: strictly-local Cell access; no ambient write intervenes.
        let previous = unsafe { (*instance).current_renderplot_backend.replace(erased) };
        Self {
            instance,
            pin,
            availability,
            previous,
        }
    }
}

#[cfg(feature = "renderplot-device")]
impl Drop for RenderPlotBackendGuard {
    fn drop(&mut self) {
        let instance = match &self.pin {
            Some(pin) => pin.as_ptr(),
            None if self.availability.is_live() => self.instance,
            None => return,
        };
        unsafe {
            (*instance).current_renderplot_backend = self.previous;
        }
    }
}

pub(crate) struct ProtectScope {
    /// Original shared-cell projection, never reconstituted from an address.
    instance: std::ptr::NonNull<RInstance>,
    /// Weak availability observation; scopes do not retain closed owners.
    liveness: crate::sexp::instance::InstanceLiveness,
    legacy_depth: usize,
    root_depth: u64,
}

impl ProtectScope {
    pub(crate) fn new(instance: *mut RInstance) -> Self {
        let liveness = unsafe { crate::sexp::instance::instance_liveness(instance) };
        let (legacy_depth, root_depth) = unsafe {
            (
                (*instance).legacy_protect.len(),
                (*instance).root_table.checkpoint(),
            )
        };
        Self {
            instance: std::ptr::NonNull::new(instance).expect("live protection owner"),
            liveness,
            legacy_depth,
            root_depth,
        }
    }
}

impl Drop for ProtectScope {
    fn drop(&mut self) {
        if !self.liveness.is_live() {
            return;
        }
        let instance = self.instance.as_ptr();
        // The observation is weak, and no callback occurs between this check
        // and these short storage operations.
        unsafe {
            (*instance).legacy_protect.truncate(self.legacy_depth);
            (*instance).root_table.restore(self.root_depth);
        }
    }
}
