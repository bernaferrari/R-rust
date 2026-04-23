#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Menu management for GraphApp.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::os::raw::c_int;
use std::ptr;

use super::controls;
use super::objects;
use super::strings;
use super::types::*;
use super::windows;

thread_local! {
    static CURRENT_MENUBAR: Cell<menubar> = Cell::new(ptr::null_mut());
    static CURRENT_MENU: Cell<menu> = Cell::new(ptr::null_mut());
    static NEXT_MENU_ID: Cell<c_int> = Cell::new(MinMenuID as c_int);
    static MENU_ACTIONS: RefCell<HashMap<usize, menufn>> = RefCell::new(HashMap::new());
}

fn next_menu_id() -> c_int {
    NEXT_MENU_ID.with(|next| {
        let id = next.get();
        next.set(id + 1);
        id
    })
}

fn find_menu_object(wparam: usize) -> object {
    unsafe {
        let handle_match = objects::find_object(wparam as *mut c_void, 0, 0);
        if !handle_match.is_null() {
            handle_match
        } else {
            objects::find_object(ptr::null_mut(), wparam as c_int, 0)
        }
    }
}

fn menu_text_ptr(name: &'static [u8]) -> *const i8 {
    name.as_ptr() as *const libc::c_char
}

unsafe fn new_menu_object(kind: c_int, parent: object, text: *const i8) -> object {
    objects::init_objects();
    let obj = objects::new_object(kind, ptr::null_mut(), parent);
    if obj.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        (*obj).text = strings::new_string(text);
        (*obj).id = next_menu_id();
        (*obj).state |= GA_Enabled;
        (*obj).handle = (*obj).id as usize as *mut c_void;
    }
    obj
}

unsafe fn install_items(parent: menu, items: *mut MenuItem) {
    if parent.is_null() || items.is_null() {
        return;
    }
    let mut idx = 0usize;
    loop {
        let item = unsafe { &*items.add(idx) };
        if item.nm.is_null() && item.fn_.is_none() && item.key == 0 {
            break;
        }

        let menu_item = unsafe { new_menu_object(MenuitemObject, parent, item.nm) };
        if menu_item.is_null() {
            break;
        }
        unsafe {
            (*menu_item).key = if (0..=255).contains(&item.key) {
                (item.key as u8).to_ascii_uppercase() as c_int
            } else {
                item.key
            };
        }
        MENU_ACTIONS.with(|actions| {
            actions.borrow_mut().insert(menu_item as usize, item.fn_);
        });

        if !item.m.is_null() {
            unsafe {
                (*item.m).parent = parent;
            }
        }

        idx += 1;
    }
}

unsafe fn trigger_menu_item(item: menuitem) {
    if item.is_null() || unsafe { controls::isenabled(item) } == 0 {
        return;
    }
    MENU_ACTIONS.with(|actions| {
        if let Some(Some(callback)) = actions.borrow().get(&(item as usize)).copied() {
            unsafe {
                callback(item);
            }
        }
    });
}

pub fn init_menus() {
    CURRENT_MENUBAR.with(|current| current.set(ptr::null_mut()));
    CURRENT_MENU.with(|current| current.set(ptr::null_mut()));
    NEXT_MENU_ID.with(|next| next.set(MinMenuID as c_int));
    MENU_ACTIONS.with(|actions| actions.borrow_mut().clear());
}

pub unsafe fn newmdimenu() -> menu {
    let menu = unsafe { new_menu_object(MenuObject, ptr::null_mut(), menu_text_ptr(b"MDI\0")) };
    CURRENT_MENU.with(|current| current.set(menu));
    menu
}

pub unsafe fn newpopup(fn_: actionfn) -> menu {
    let popup = unsafe { new_menu_object(MenuObject, ptr::null_mut(), menu_text_ptr(b"Popup\0")) };
    if !popup.is_null() {
        unsafe {
            (*popup).action = fn_;
        }
    }
    CURRENT_MENU.with(|current| current.set(popup));
    popup
}

pub unsafe fn gmenubar(fn_: actionfn, items: *mut MenuItem) -> menubar {
    let parent = windows::get_current_window();
    let menubar = unsafe { new_menu_object(MenubarObject, parent, menu_text_ptr(b"Menubar\0")) };
    if !menubar.is_null() {
        unsafe {
            (*menubar).action = fn_;
            if !parent.is_null() {
                (*parent).menubar = menubar;
            }
        }
        CURRENT_MENUBAR.with(|current| current.set(menubar));
        install_items(menubar, items);
    }
    menubar
}

pub unsafe fn gpopup(fn_: actionfn, items: *mut MenuItem) -> menu {
    let popup = newpopup(fn_);
    install_items(popup, items);
    popup
}

pub unsafe fn gchangepopup(w: window, p: menu) {
    if w.is_null() {
        return;
    }
    unsafe {
        (*w).popup = p;
    }
    CURRENT_MENU.with(|current| current.set(p));
}

pub unsafe fn gchangemenubar(mb: menubar) {
    let window = windows::get_current_window();
    if !window.is_null() {
        unsafe {
            (*window).menubar = mb;
        }
    }
    CURRENT_MENUBAR.with(|current| current.set(mb));
}

pub unsafe fn adjust_menu(wparam: usize) {
    let obj = find_menu_object(wparam);
    if obj.is_null() {
        return;
    }

    unsafe {
        match (*obj).kind {
            MenubarObject => {
                CURRENT_MENUBAR.with(|current| current.set(obj));
                if let Some(action) = (*obj).action {
                    action(obj);
                }
            }
            MenuObject => {
                CURRENT_MENU.with(|current| current.set(obj));
                if let Some(action) = (*obj).action {
                    action(obj);
                }
            }
            MenuitemObject => {
                if let Some(parent) = (!(*obj).parent.is_null()).then_some((*obj).parent) {
                    adjust_menu((*parent).id as usize);
                }
            }
            _ => {}
        }
    }
}

pub unsafe fn handle_menu_id(wparam: usize) {
    let obj = find_menu_object(wparam);
    if obj.is_null() {
        return;
    }
    unsafe {
        if (*obj).kind == MenuitemObject {
            trigger_menu_item(obj);
        }
    }
}

pub unsafe fn handle_menu_key(wparam: usize) -> c_int {
    let key = (wparam as u8 as char).to_ascii_uppercase() as c_int;
    let obj = unsafe { objects::find_object(ptr::null_mut(), 0, key) };
    if obj.is_null() {
        return 0;
    }
    unsafe {
        if !(*obj).parent.is_null() {
            adjust_menu((*(*obj).parent).id as usize);
        }
        trigger_menu_item(obj);
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    thread_local! {
        static MENU_CALLS: Cell<c_int> = const { Cell::new(0) };
        static ADJUST_CALLS: Cell<c_int> = const { Cell::new(0) };
    }

    unsafe extern "C" fn item_callback(_item: menuitem) {
        MENU_CALLS.with(|calls| calls.set(calls.get() + 1));
    }

    unsafe extern "C" fn adjust_callback(_control: control) {
        ADJUST_CALLS.with(|calls| calls.set(calls.get() + 1));
    }

    #[test]
    fn popup_creation_installs_dispatchable_items() {
        MENU_CALLS.with(|calls| calls.set(0));
        unsafe {
            objects::init_objects();
            init_menus();

            let item_name = CString::new("Open").unwrap_or_else(|e| panic!("{e}"));
            let mut items = [
                MenuItem {
                    nm: item_name.as_ptr() as *mut libc::c_char,
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
        MENU_CALLS.with(|calls| calls.set(0));
        ADJUST_CALLS.with(|calls| calls.set(0));
        unsafe {
            objects::init_objects();
            init_menus();

            let item_name = CString::new("Save").unwrap_or_else(|e| panic!("{e}"));
            let mut items = [
                MenuItem {
                    nm: item_name.as_ptr() as *mut libc::c_char,
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
}
