#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]
#![warn(unused_unsafe)]

//! Menu management for GraphApp.

use std::ffi::c_void;
use std::os::raw::c_int;
use std::ptr;

use super::controls;
use super::objects;
use super::runtime::with_graphapp_runtime;
use super::strings;
use super::types::*;
use super::windows;

fn next_menu_id() -> c_int {
    with_graphapp_runtime(|runtime| {
        let id = runtime.menus.next_menu_id;
        runtime.menus.next_menu_id += 1;
        id
    })
}

fn find_menu_object(wparam: usize) -> object {
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    let handle_match = unsafe { objects::find_object(wparam as *mut c_void, 0, 0) };
    if !handle_match.is_null() {
        handle_match
    } else {
        // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
        unsafe { objects::find_object(ptr::null_mut(), wparam as c_int, 0) }
    }
}

fn menu_text_ptr(name: &'static [u8]) -> *const core::ffi::c_char {
    name.as_ptr() as *const core::ffi::c_char
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn new_menu_object(kind: c_int, parent: object, text: *const core::ffi::c_char) -> object {
    objects::init_objects();

    // SAFETY: forwarded raw pointers satisfy `new_object`'s contract per this function's own `# Safety` requirements.
    let obj = unsafe { objects::new_object(kind, ptr::null_mut(), parent) };
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    let Some(data) = (unsafe { obj_mut(obj) }) else {
        return ptr::null_mut();
    };
    // SAFETY: forwarded raw pointers satisfy `new_string`'s contract per this function's own `# Safety` requirements.
    data.text = unsafe { strings::new_string(text) };
    data.id = next_menu_id();
    data.state |= GA_Enabled;
    data.handle = data.id as usize as *mut c_void;
    obj
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn install_items(parent: menu, items: *mut MenuItem) {
    if parent.is_null() || items.is_null() {
        return;
    }
    let mut idx = 0usize;
    loop {
        // Walk the caller's sentinel-terminated `MenuItem` array.
        // SAFETY: forwarded raw pointers satisfy `ptr_ref`'s contract per this function's own `# Safety` requirements.
        let Some(item) = (unsafe { ptr_ref(items.wrapping_add(idx)) }) else {
            break;
        };
        if item.nm.is_null() && item.fn_.is_none() && item.key == 0 {
            break;
        }

        // SAFETY: forwarded raw pointers satisfy `new_menu_object`'s contract per this function's own `# Safety` requirements.
        let menu_item = unsafe { new_menu_object(MenuitemObject, parent, item.nm) };
        if menu_item.is_null() {
            break;
        }
        // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
        if let Some(data) = unsafe { obj_mut(menu_item) } {
            data.key = if (0..=255).contains(&item.key) {
                (item.key as u8).to_ascii_uppercase() as c_int
            } else {
                item.key
            };
        }
        with_graphapp_runtime(|runtime| {
            runtime.menus.actions.insert(menu_item as usize, item.fn_);
        });

        // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
        if let Some(submenu) = unsafe { obj_mut(item.m) } {
            submenu.parent = parent;
        }

        idx += 1;
    }
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
unsafe fn trigger_menu_item(item: menuitem) {
    // SAFETY: forwarded raw pointers satisfy `isenabled`'s contract per this function's own `# Safety` requirements.
    if item.is_null() || unsafe { controls::isenabled(item) } == 0 {
        return;
    }
    with_graphapp_runtime(|runtime| {
        if let Some(Some(callback)) = runtime.menus.actions.get(&(item as usize)).copied() {
            callback(item);
        }
    });
}

pub fn init_menus() {
    with_graphapp_runtime(|runtime| {
        runtime.menus.current_menubar = ptr::null_mut();
        runtime.menus.current_menu = ptr::null_mut();
        runtime.menus.next_menu_id = MinMenuID as c_int;
        runtime.menus.actions.clear();
    });
}

pub fn newmdimenu() -> menu {
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    let menu = unsafe { new_menu_object(MenuObject, ptr::null_mut(), menu_text_ptr(b"MDI\0")) };
    with_graphapp_runtime(|runtime| runtime.menus.current_menu = menu);
    menu
}

pub fn newpopup(fn_: actionfn) -> menu {
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    let popup = unsafe { new_menu_object(MenuObject, ptr::null_mut(), menu_text_ptr(b"Popup\0")) };
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    if let Some(data) = unsafe { obj_mut(popup) } {
        data.action = fn_;
    }
    with_graphapp_runtime(|runtime| runtime.menus.current_menu = popup);
    popup
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn gmenubar(fn_: actionfn, items: *mut MenuItem) -> menubar {
    let parent = windows::get_current_window();
    // SAFETY: forwarded raw pointers satisfy `new_menu_object`'s contract per this function's own `# Safety` requirements.
    let menubar = unsafe { new_menu_object(MenubarObject, parent, menu_text_ptr(b"Menubar\0")) };
    if !menubar.is_null() {
        // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
        if let Some(data) = unsafe { obj_mut(menubar) } {
            data.action = fn_;
        }
        // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
        if let Some(parent_data) = unsafe { obj_mut(parent) } {
            parent_data.menubar = menubar;
        }
        with_graphapp_runtime(|runtime| runtime.menus.current_menubar = menubar);

        // SAFETY: forwarded raw pointers satisfy `install_items`'s contract per this function's own `# Safety` requirements.
        unsafe { install_items(menubar, items) };
    }
    menubar
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn gpopup(fn_: actionfn, items: *mut MenuItem) -> menu {
    let popup = newpopup(fn_);

    // SAFETY: forwarded raw pointers satisfy `install_items`'s contract per this function's own `# Safety` requirements.
    unsafe { install_items(popup, items) };

    popup
}

/// # Safety
///
/// Each raw-pointer argument must be null where the C original accepts null, or else point to a
/// live, properly initialized object of its pointee type (C strings NUL-terminated, buffers valid for
/// the accompanying lengths) for the duration of the call.
pub unsafe fn gchangepopup(w: window, p: menu) {
    // SAFETY: forwarded raw pointers satisfy `obj_mut`'s contract per this function's own `# Safety` requirements.
    if let Some(data) = unsafe { obj_mut(w) } {
        data.popup = p;
    } else {
        return;
    }
    with_graphapp_runtime(|runtime| runtime.menus.current_menu = p);
}

pub fn gchangemenubar(mb: menubar) {
    let window = windows::get_current_window();
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    if let Some(data) = unsafe { obj_mut(window) } {
        data.menubar = mb;
    }
    with_graphapp_runtime(|runtime| runtime.menus.current_menubar = mb);
}

pub fn adjust_menu(wparam: usize) {
    let obj = find_menu_object(wparam);
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    let Some(data) = (unsafe { obj_ref(obj) }) else {
        return;
    };

    match data.kind {
        MenubarObject => {
            with_graphapp_runtime(|runtime| runtime.menus.current_menubar = obj);
            if let Some(action) = data.action {
                action(obj);
            }
        }
        MenuObject => {
            with_graphapp_runtime(|runtime| runtime.menus.current_menu = obj);
            if let Some(action) = data.action {
                action(obj);
            }
        }
        MenuitemObject => {
            // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
            if let Some(parent_id) = unsafe { obj_ref(data.parent) }.map(|parent| parent.id) {
                adjust_menu(parent_id as usize);
            }
        }
        _ => {}
    }
}

pub fn handle_menu_id(wparam: usize) {
    let obj = find_menu_object(wparam);
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    if unsafe { obj_ref(obj) }.is_some_and(|data| data.kind == MenuitemObject) {
        // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
        unsafe { trigger_menu_item(obj) };
    }
}

pub fn handle_menu_key(wparam: usize) -> c_int {
    let key = (wparam as u8 as char).to_ascii_uppercase() as c_int;
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    let obj = unsafe { objects::find_object(ptr::null_mut(), 0, key) };
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    let Some(data) = (unsafe { obj_ref(obj) }) else {
        return 0;
    };
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    if let Some(parent_id) = unsafe { obj_ref(data.parent) }.map(|parent| parent.id) {
        adjust_menu(parent_id as usize);
    }
    // SAFETY: pointer arguments are null, locals/literals of this caller, or GraphApp handles taken from the live object tree or registry (objects stay allocated until deleted).
    unsafe { trigger_menu_item(obj) };
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::ffi::CString;

    thread_local! {
        static MENU_CALLS: Cell<c_int> = const { Cell::new(0) };
        static ADJUST_CALLS: Cell<c_int> = const { Cell::new(0) };
    }

    extern "C" fn item_callback(_item: menuitem) {
        MENU_CALLS.with(|calls| calls.set(calls.get() + 1));
    }

    extern "C" fn adjust_callback(_control: control) {
        ADJUST_CALLS.with(|calls| calls.set(calls.get() + 1));
    }

    #[test]
    fn popup_creation_installs_dispatchable_items() {
        let _session = crate::sexp::session::RSession::new_for_gc_tests();
        MENU_CALLS.with(|calls| calls.set(0));
        unsafe {
            objects::init_objects();
            init_menus();

            let item_name = CString::new("Open").unwrap_or_else(|e| panic!("{e}"));
            let mut items = [
                MenuItem {
                    nm: item_name.as_ptr() as *mut core::ffi::c_char,
                    fn_: Some(item_callback),
                    key: 'O' as c_int,
                    m: ptr::null_mut(),
                },
                MenuItem {
                    nm: ptr::null_mut(),
                    fn_: None,
                    key: 0,
                    m: ptr::null_mut(),
                },
            ];

            let popup = gpopup(None, items.as_mut_ptr());
            let child = (*popup).child;
            assert!(!child.is_null());

            handle_menu_id((*child).id as usize);
            assert_eq!(MENU_CALLS.with(|calls| calls.get()), 1);
        }
    }

    #[test]
    fn menu_keys_run_adjust_then_action() {
        let _session = crate::sexp::session::RSession::new_for_gc_tests();
        MENU_CALLS.with(|calls| calls.set(0));
        ADJUST_CALLS.with(|calls| calls.set(0));

        objects::init_objects();
        init_menus();

        let item_name = CString::new("Save").unwrap_or_else(|e| panic!("{e}"));
        let mut items = [
            MenuItem {
                nm: item_name.as_ptr() as *mut core::ffi::c_char,
                fn_: Some(item_callback),
                key: 'S' as c_int,
                m: ptr::null_mut(),
            },
            MenuItem {
                nm: ptr::null_mut(),
                fn_: None,
                key: 0,
                m: ptr::null_mut(),
            },
        ];

        // SAFETY: pointer arguments are live locals, NUL-terminated literals or owned CString/Vec buffers of this caller, valid for the call.
        let menubar = unsafe { gmenubar(Some(adjust_callback), items.as_mut_ptr()) };
        assert_eq!(handle_menu_key('s' as usize), 1);
        assert!(!menubar.is_null());
        assert_eq!(ADJUST_CALLS.with(|calls| calls.get()), 1);
        assert_eq!(MENU_CALLS.with(|calls| calls.get()), 1);
    }
}
