#![allow(dead_code)]

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_uint, c_void};
use std::ptr;

use super::types::*;

pub(crate) struct GraphAppRuntimeState {
    pub app: AppState,
    pub clipboard_text: Vec<u8>,
    pub contexts: Vec<ContextEntry>,
    pub cursors: CursorState,
    pub current_drawstate: drawstruct,
    pub dialogs: DialogState,
    pub events: EventState,
    pub fonts: FontState,
    pub gdraw: GDrawState,
    pub menus: MenuState,
    pub objects: ObjectState,
    pub windows: WindowState,
}

impl Default for GraphAppRuntimeState {
    fn default() -> Self {
        Self {
            app: AppState::default(),
            clipboard_text: Vec::new(),
            contexts: Vec::new(),
            cursors: CursorState::default(),
            current_drawstate: default_drawstate(),
            dialogs: DialogState::default(),
            events: EventState::default(),
            fonts: FontState::default(),
            gdraw: GDrawState::default(),
            menus: MenuState::default(),
            objects: ObjectState::default(),
            windows: WindowState::default(),
        }
    }
}

fn default_drawstate() -> drawstruct {
    drawstruct {
        dest: ptr::null_mut(),
        hue: Black,
        mode: GA_S,
        p: point { x: 0, y: 0 },
        linewidth: 1,
        fnt: ptr::null_mut(),
        crsr: ptr::null_mut(),
    }
}

#[derive(Clone, Copy)]
pub(crate) struct AppState {
    pub initialised: c_int,
    pub name: *mut c_char,
    pub topmost_dialogs: c_int,
    pub mdi_frame: object,
    pub mdi_toolbar: object,
    pub mdi_status: *mut c_void,
    pub hwnd_main: *mut c_void,
    pub hwnd_frame: *mut c_void,
    pub hwnd_client: *mut c_void,
    pub this_instance: *mut c_void,
    pub prev_instance: *mut c_void,
    pub menus_active: c_int,
    pub locale_cp: c_uint,
    pub is_nt: c_int,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            initialised: 0,
            name: ptr::null_mut(),
            topmost_dialogs: 0,
            mdi_frame: ptr::null_mut(),
            mdi_toolbar: ptr::null_mut(),
            mdi_status: ptr::null_mut(),
            hwnd_main: ptr::null_mut(),
            hwnd_frame: ptr::null_mut(),
            hwnd_client: ptr::null_mut(),
            this_instance: ptr::null_mut(),
            prev_instance: ptr::null_mut(),
            menus_active: 1,
            locale_cp: 0,
            is_nt: 1,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ContextEntry {
    pub obj: object,
    pub dc: *mut c_void,
    pub old: *mut c_void,
}

pub(crate) struct DelNode {
    pub obj: object,
    pub next: *mut DelNode,
    pub prev: *mut DelNode,
}

#[derive(Default)]
pub(crate) struct DialogState {
    pub user_filter: Option<CString>,
    pub last_message: Option<(c_int, String)>,
}

#[derive(Clone, Copy)]
pub(crate) struct CursorState {
    pub arrow: cursor,
    pub blank: cursor,
    pub watch: cursor,
    pub caret: cursor,
    pub text: cursor,
    pub hand: cursor,
    pub cross: cursor,
}

impl Default for CursorState {
    fn default() -> Self {
        Self {
            arrow: ptr::null_mut(),
            blank: ptr::null_mut(),
            watch: ptr::null_mut(),
            caret: ptr::null_mut(),
            text: ptr::null_mut(),
            hand: ptr::null_mut(),
            cross: ptr::null_mut(),
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct EventState {
    pub keystate: c_int,
    pub timer: TimerState,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct TimerState {
    pub timeout: timerfn,
    pub data: *mut c_void,
    pub millisec: c_uint,
    pub pending: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct FontState {
    pub fixed: font,
    pub system: font,
    pub times: font,
    pub helvetica: font,
    pub courier: font,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct FontInfo {
    pub height: c_int,
    pub style: c_int,
    pub quality: c_int,
    pub use_points: c_int,
}

#[derive(Default)]
pub(crate) struct DrawingState {
    pub clip: Option<rect>,
    pub pixels: BTreeMap<(c_int, c_int), rgb>,
    pub odd_even_fill: bool,
}

#[derive(Default)]
pub(crate) struct GDrawState {
    pub drawings: HashMap<usize, DrawingState>,
    pub fonts: HashMap<usize, FontInfo>,
}

pub(crate) struct MenuState {
    pub current_menubar: menubar,
    pub current_menu: menu,
    pub next_menu_id: c_int,
    pub actions: HashMap<usize, menufn>,
}

impl Default for MenuState {
    fn default() -> Self {
        Self {
            current_menubar: ptr::null_mut(),
            current_menu: ptr::null_mut(),
            next_menu_id: MinMenuID as c_int,
            actions: HashMap::new(),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ObjectState {
    pub base_object: object,
    pub deletion_base: *mut DelNode,
    pub deletion_level: c_int,
}

impl Default for ObjectState {
    fn default() -> Self {
        Self {
            base_object: ptr::null_mut(),
            deletion_base: ptr::null_mut(),
            deletion_level: 0,
        }
    }
}

impl Default for FontState {
    fn default() -> Self {
        Self {
            fixed: ptr::null_mut(),
            system: ptr::null_mut(),
            times: ptr::null_mut(),
            helvetica: ptr::null_mut(),
            courier: ptr::null_mut(),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct WindowState {
    pub current: window,
    pub active_count: c_int,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            current: ptr::null_mut(),
            active_count: 0,
        }
    }
}

pub(crate) fn with_graphapp_runtime<R>(f: impl FnOnce(&mut GraphAppRuntimeState) -> R) -> R {
    crate::sexp::instance::with_required_current_instance(|inst| f(&mut inst.graphapp_state))
}
