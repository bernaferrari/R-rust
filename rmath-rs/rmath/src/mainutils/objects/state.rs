#![allow(
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    unused_variables,
    unused_imports
)]

use super::*;

// ---------------------------------------------------------------------------
// IS_S4_OBJECT -- check the S4 object bit in sxpinfo
// ---------------------------------------------------------------------------

/// Check whether the S4 object bit is set on an SEXP.
/// The S4 bit is gp bit 4 (value 16) in R's SxpInfo.
pub(crate) unsafe fn IS_S4_OBJECT(x: SEXP) -> c_int {
    unsafe {
        if x.is_null() {
            return FALSE;
        }
        let gp = (*x).sxpinfo.gp();
        if (gp & 16) != 0 { TRUE } else { FALSE }
    }
}

/// Set the S4 object bit.
unsafe fn SET_S4_OBJECT(x: SEXP) {
    unsafe {
        if !x.is_null() {
            let gp = (*x).sxpinfo.gp();
            (*x).sxpinfo.set_gp(gp | 16);
        }
    }
}

/// Unset the S4 object bit.
unsafe fn UNSET_S4_OBJECT(x: SEXP) {
    unsafe {
        if !x.is_null() {
            let gp = (*x).sxpinfo.gp();
            (*x).sxpinfo.set_gp(gp & !16);
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: install pre-defined symbols (lazy)
// ---------------------------------------------------------------------------

/// Get or install the ".__S3MethodsTable__." symbol.
pub(crate) unsafe fn S3MethodsTable_symbol() -> SEXP {
    unsafe { Rf_install(b".__S3MethodsTable__.\x00".as_ptr() as *const c_char) }
}

pub(crate) unsafe fn error(msg: &str) -> ! {
    std::panic::panic_any(crate::sexp::context::RError {
        message: msg.to_string(),
    });
}

/// Install a named symbol, caching the result.
pub(crate) unsafe fn sym(name: &str) -> SEXP {
    unsafe {
        let cstr = std::ffi::CString::new(name).unwrap_or_default();
        Rf_install(cstr.as_ptr())
    }
}

// ---------------------------------------------------------------------------
// R_stdGen_ptr_t type alias
// ---------------------------------------------------------------------------

/// Function pointer type for standardGeneric dispatch.
pub type R_stdGen_ptr_t = Option<unsafe fn(arg: SEXP, env: SEXP, fdef: SEXP) -> SEXP>;

const DEFAULT_N_PRIM_METHODS: c_int = 100;

/// Primitive method status codes.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum prim_methods_t {
    NO_METHODS = 0,
    NEEDS_RESET = 1,
    HAS_METHODS = 2,
    SUPPRESSED = 3,
}

pub(crate) struct ObjectsRuntimeState {
    max_methods_offset: c_int,
    pub(crate) cur_max_offset: c_int,
    pub(crate) allow_primitive_methods: c_int,
    pub(crate) prim_methods: Vec<prim_methods_t>,
    pub(crate) prim_generics: Vec<SEXP>,
    pub(crate) prim_mlist: Vec<SEXP>,
    pub(crate) standard_generic_ptr: R_stdGen_ptr_t,
    pub(crate) quick_method_check_ptr: R_stdGen_ptr_t,
    pub(crate) deferred_default_object: SEXP,
    s4_classes: HashMap<String, S4ClassDef>,
}

#[derive(Clone, Default)]
pub(crate) struct S4ClassDef {
    pub slots: Vec<String>,
    pub contains: Vec<String>,
    pub virtual_class: bool,
    pub has_validity: bool,
}

impl Default for ObjectsRuntimeState {
    fn default() -> Self {
        Self {
            max_methods_offset: 0,
            cur_max_offset: 0,
            allow_primitive_methods: TRUE,
            prim_methods: Vec::new(),
            prim_generics: Vec::new(),
            prim_mlist: Vec::new(),
            standard_generic_ptr: None,
            quick_method_check_ptr: None,
            deferred_default_object: ptr::null_mut(),
            s4_classes: HashMap::new(),
        }
    }
}

impl ObjectsRuntimeState {
    fn ensure_primitive_tables(&mut self) {
        if self.prim_methods.is_empty() {
            let n = DEFAULT_N_PRIM_METHODS as usize;
            self.prim_methods.resize(n, prim_methods_t::NO_METHODS);
            self.prim_generics.resize(n, ptr::null_mut());
            self.prim_mlist.resize(n, ptr::null_mut());
            self.max_methods_offset = DEFAULT_N_PRIM_METHODS;
        }
    }

    pub(crate) fn ensure_primitive_slot(&mut self, offset: usize) {
        self.ensure_primitive_tables();
        if offset >= self.prim_methods.len() {
            let new_len = (offset + 1).max(self.prim_methods.len() * 2);
            self.prim_methods
                .resize(new_len, prim_methods_t::NO_METHODS);
            self.prim_generics.resize(new_len, ptr::null_mut());
            self.prim_mlist.resize(new_len, ptr::null_mut());
            self.max_methods_offset = new_len as c_int;
        }
    }
}

pub(crate) fn with_objects_state<F, R>(f: F) -> R
where
    F: FnOnce(&mut ObjectsRuntimeState) -> R,
{
    crate::sexp::instance::with_required_current_instance(|inst| f(&mut inst.objects_state))
}

pub(crate) fn register_s4_class(name: String, slots: Vec<String>, virtual_class: bool) {
    register_s4_class_with_extends(name, slots, Vec::new(), virtual_class);
}

pub(crate) fn register_s4_class_with_extends(
    name: String,
    slots: Vec<String>,
    contains: Vec<String>,
    virtual_class: bool,
) {
    with_objects_state(|state| {
        state.s4_classes.insert(
            name,
            S4ClassDef {
                slots,
                contains,
                virtual_class,
                has_validity: false,
            },
        );
    });
}

pub(crate) fn set_s4_validity(name: &str) -> bool {
    with_objects_state(|state| {
        let Some(class_def) = state.s4_classes.get_mut(name) else {
            return false;
        };
        class_def.has_validity = true;
        true
    })
}

pub(crate) fn s4_class(name: &str) -> Option<S4ClassDef> {
    with_objects_state(|state| state.s4_classes.get(name).cloned())
}

fn collect_s4_slots(
    classes: &HashMap<String, S4ClassDef>,
    name: &str,
    visited: &mut HashSet<String>,
    out: &mut Vec<String>,
) {
    if !visited.insert(name.to_string()) {
        return;
    }
    let Some(class_def) = classes.get(name) else {
        return;
    };
    for slot in &class_def.slots {
        if !out.iter().any(|existing| existing == slot) {
            out.push(slot.clone());
        }
    }
    for parent in &class_def.contains {
        collect_s4_slots(classes, parent, visited, out);
    }
}

pub(crate) fn s4_all_slots(name: &str) -> Option<Vec<String>> {
    with_objects_state(|state| {
        state.s4_classes.contains_key(name).then(|| {
            let mut slots = Vec::new();
            collect_s4_slots(&state.s4_classes, name, &mut HashSet::new(), &mut slots);
            slots
        })
    })
}

pub(crate) fn s4_class_extends(class1: &str, class2: &str) -> bool {
    with_objects_state(|state| {
        s4_extends_registered(&state.s4_classes, class1, class2, &mut HashSet::new())
    })
}

fn s4_extends_registered(
    classes: &HashMap<String, S4ClassDef>,
    class1: &str,
    class2: &str,
    visited: &mut HashSet<String>,
) -> bool {
    if class1 == class2 {
        return true;
    }
    if !visited.insert(class1.to_string()) {
        return false;
    }
    let Some(class_def) = classes.get(class1) else {
        return false;
    };
    class_def
        .contains
        .iter()
        .any(|parent| parent == class2 || s4_extends_registered(classes, parent, class2, visited))
}

pub(crate) unsafe fn primitive_offset(op: SEXP) -> Option<usize> {
    unsafe {
        if op.is_null()
            || (TYPEOF(op) != SEXPTYPE::BUILTINSXP && TYPEOF(op) != SEXPTYPE::SPECIALSXP)
        {
            return None;
        }
        let offset = PRIMOFFSET(op);
        if offset < 0 {
            None
        } else {
            Some(offset as usize)
        }
    }
}
