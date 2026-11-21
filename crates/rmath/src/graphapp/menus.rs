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
    let handle_match = objects::find_object(wparam as *mut c_void, 0, 0);
    if !handle_match.is_null() {
        handle_match
    } else {
        objects::find_object(ptr::null_mut(), wparam as c_int, 0)
    }
}

fn menu_text_ptr(name: &'static [u8]) -> *const core::ffi::c_char {
    name.as_ptr() as *const core::ffi::c_char
}

fn new_menu_object(kind: c_int, parent: object, text: *const core::ffi::c_char) -> object {
    objects::init_objects();

    let obj = objects::new_object(kind, ptr::null_mut(), parent);
    let Some(data) = obj_mut(obj) else {
        return ptr::null_mut();
    };
    data.text = strings::new_string(text);
    data.id = next_menu_id();
    data.state |= GA_Enabled;
    data.handle = data.id as usize as *mut c_void;
    obj
}

fn install_items(parent: menu, items: *mut MenuItem) {
    if parent.is_null() || items.is_null() {
        return;
    }
    let mut idx = 0usize;
    loop {
        // Walk the caller's sentinel-terminated `MenuItem` array.
        let Some(item) = ptr_ref(items.wrapping_add(idx)) else {
            break;
        };
        if item.nm.is_null() && item.fn_.is_none() && item.key == 0 {
            break;
        }

        let menu_item = new_menu_object(MenuitemObject, parent, item.nm);
        if menu_item.is_null() {
            break;
        }
        if let Some(data) = obj_mut(menu_item) {
            data.key = if (0..=255).contains(&item.key) {
                (item.key as u8).to_ascii_uppercase() as c_int
            } else {
                item.key
            };
        }
        with_graphapp_runtime(|runtime| {
            runtime.menus.actions.insert(menu_item as usize, item.fn_);
        });

        if let Some(submenu) = obj_mut(item.m) {
            submenu.parent = parent;
        }

        idx += 1;
    }
}

fn trigger_menu_item(item: menuitem) {
    if item.is_null() || controls::isenabled(item) == 0 {
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
    let menu = new_menu_object(MenuObject, ptr::null_mut(), menu_text_ptr(b"MDI\0"));
    with_graphapp_runtime(|runtime| runtime.menus.current_menu = menu);
    menu
}

pub fn newpopup(fn_: actionfn) -> menu {
    let popup = new_menu_object(MenuObject, ptr::null_mut(), menu_text_ptr(b"Popup\0"));
    if let Some(data) = obj_mut(popup) {
        data.action = fn_;
    }
    with_graphapp_runtime(|runtime| runtime.menus.current_menu = popup);
    popup
}

pub fn gmenubar(fn_: actionfn, items: *mut MenuItem) -> menubar {
    let parent = windows::get_current_window();
    let menubar = new_menu_object(MenubarObject, parent, menu_text_ptr(b"Menubar\0"));
    if !menubar.is_null() {
        if let Some(data) = obj_mut(menubar) {
            data.action = fn_;
        }
        if let Some(parent_data) = obj_mut(parent) {
            parent_data.menubar = menubar;
        }
        with_graphapp_runtime(|runtime| runtime.menus.current_menubar = menubar);

        install_items(menubar, items);
    }
    menubar
}

pub fn gpopup(fn_: actionfn, items: *mut MenuItem) -> menu {
    let popup = newpopup(fn_);

    install_items(popup, items);

    popup
}

pub fn gchangepopup(w: window, p: menu) {
    if let Some(data) = obj_mut(w) {
        data.popup = p;
    } else {
        return;
    }
    with_graphapp_runtime(|runtime| runtime.menus.current_menu = p);
}

pub fn gchangemenubar(mb: menubar) {
    let window = windows::get_current_window();
    if let Some(data) = obj_mut(window) {
        data.menubar = mb;
    }
    with_graphapp_runtime(|runtime| runtime.menus.current_menubar = mb);
}

pub fn adjust_menu(wparam: usize) {
    let obj = find_menu_object(wparam);
    let Some(data) = obj_ref(obj) else {
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
            if let Some(parent_id) = obj_ref(data.parent).map(|parent| parent.id) {
                adjust_menu(parent_id as usize);
            }
        }
        _ => {}
    }
}

pub fn handle_menu_id(wparam: usize) {
    let obj = find_menu_object(wparam);
    if obj_ref(obj).is_some_and(|data| data.kind == MenuitemObject) {
        trigger_menu_item(obj);
    }
}

pub fn handle_menu_key(wparam: usize) -> c_int {
    let key = (wparam as u8 as char).to_ascii_uppercase() as c_int;
    let obj = objects::find_object(ptr::null_mut(), 0, key);
    let Some(data) = obj_ref(obj) else {
        return 0;
    };
    if let Some(parent_id) = obj_ref(data.parent).map(|parent| parent.id) {
        adjust_menu(parent_id as usize);
    }
    trigger_menu_item(obj);
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

        let menubar = gmenubar(Some(adjust_callback), items.as_mut_ptr());
        assert_eq!(handle_menu_key('s' as usize), 1);
        assert!(!menubar.is_null());
        assert_eq!(ADJUST_CALLS.with(|calls| calls.get()), 1);
        assert_eq!(MENU_CALLS.with(|calls| calls.get()), 1);
    }
}
