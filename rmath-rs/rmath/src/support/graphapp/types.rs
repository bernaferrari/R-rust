#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::missing_safety_doc)]

//! Core types for the GraphApp GUI library.
//!
//! Defines the fundamental data structures: point, rect, rgb, drawstruct,
//! imagedata, objinfo, callinfo, and all the callback function pointer types.

use std::os::raw::{c_char, c_int, c_long, c_uint, c_ulong, c_void};
use std::ptr;

// ============================================================
// Constants
// ============================================================

pub const PI: f64 = 3.14159265359;

// Mouse button state bit-fields
pub const NoButton: c_int = 0x0000;
pub const LeftButton: c_int = 0x0001;
pub const MiddleButton: c_int = 0x0002;
pub const RightButton: c_int = 0x0004;

// ANSI character codes
pub const BELL: c_int = 0x07;
pub const BKSP: c_int = 0x08;
pub const VTAB: c_int = 0x0B;
pub const FF: c_int = 0x0C;
pub const ESC: c_int = 0x1B;

// Edit-key codes
pub const INS: c_int = 0x2041;
pub const DEL: c_int = 0x2326;
pub const HOME: c_int = 0x21B8;
pub const END: c_int = 0x2198;
pub const PGUP: c_int = 0x21DE;
pub const PGDN: c_int = 0x21DF;
pub const ENTER: c_int = 0x2324;

// Cursor-key codes
pub const LEFT: c_int = 0x2190;
pub const UP: c_int = 0x2191;
pub const RIGHT: c_int = 0x2192;
pub const DOWN: c_int = 0x2193;

// Function-key codes
pub const F1: c_int = 0x276C;
pub const F2: c_int = 0x276D;
pub const F3: c_int = 0x276E;
pub const F4: c_int = 0x276F;
pub const F5: c_int = 0x2770;
pub const F6: c_int = 0x2771;
pub const F7: c_int = 0x2772;
pub const F8: c_int = 0x2773;
pub const F9: c_int = 0x2774;
pub const F10: c_int = 0x2775;

// Window creation flags
pub const SimpleWindow: c_ulong = 0x00000000;
pub const Menubar: c_ulong = 0x00000010;
pub const Titlebar: c_ulong = 0x00000020;
pub const Closebox: c_ulong = 0x00000040;
pub const Resize: c_ulong = 0x00000080;
pub const Maximize: c_ulong = 0x00000100;
pub const Minimize: c_ulong = 0x00000200;
pub const HScrollbar: c_ulong = 0x00000400;
pub const VScrollbar: c_ulong = 0x00000800;
pub const CanvasSize: c_ulong = 0x00400000;

pub const Modal: c_ulong = 0x00001000;
pub const Floating: c_ulong = 0x00002000;
pub const Centered: c_ulong = 0x00004000;
pub const Centred: c_ulong = 0x00004000;

pub const Workspace: c_ulong = 0x00010000;
pub const Document: c_ulong = 0x00020000;
pub const ChildWindow: c_ulong = 0x00040000;

pub const TrackMouse: c_ulong = 0x00080000;
pub const UsePalette: c_ulong = 0x00100000;
pub const UseUnicode: c_ulong = 0x00200000;
pub const SetUpCaret: c_ulong = 0x00400000;

pub const StandardWindow: c_ulong = Titlebar | Closebox | Resize | Maximize | Minimize;
pub const Border: c_ulong = 0x10100000;

// Control states
pub const GA_Visible: c_long = 0x0001;
pub const GA_Enabled: c_long = 0x0002;
pub const GA_Checked: c_long = 0x0004;
pub const GA_Highlighted: c_long = 0x0008;
pub const GA_Armed: c_long = 0x0010;
pub const GA_Focus: c_long = 0x0020;

// Keyboard state
pub const AltKey: c_int = 0x0001;
pub const CmdKey: c_int = 0x0001;
pub const CtrlKey: c_int = 0x0002;
pub const OptionKey: c_int = 0x0002;
pub const ShiftKey: c_int = 0x0004;

// Transfer modes for drawing operations
pub const Zeros: c_int = 0x00;
pub const DnorS: c_int = 0x01;
pub const DandnotS: c_int = 0x02;
pub const notS: c_int = 0x03;
pub const notDandS: c_int = 0x04;
pub const notD: c_int = 0x05;
pub const DxorS: c_int = 0x06;
pub const DnandS: c_int = 0x07;
pub const DandS: c_int = 0x08;
pub const DxnorS: c_int = 0x09;
pub const GA_D: c_int = 0x0A;
pub const DornotS: c_int = 0x0B;
pub const GA_S: c_int = 0x0C;
pub const notDorS: c_int = 0x0D;
pub const DorS: c_int = 0x0E;
pub const Ones: c_int = 0x0F;

// Text styles
pub const Plain: c_int = 0x0000;
pub const Bold: c_int = 0x0001;
pub const Italic: c_int = 0x0002;
pub const BoldItalic: c_int = 0x0003;
pub const SansSerif: c_int = 0x0004;
pub const FixedWidth: c_int = 0x0008;
pub const Wide: c_int = 0x0010;
pub const Narrow: c_int = 0x0020;

// Text alignments
pub const AlignTop: c_int = 0x0000;
pub const AlignBottom: c_int = 0x0100;
pub const VJustify: c_int = 0x0200;
pub const VCenter: c_int = 0x0400;
pub const VCentre: c_int = 0x0400;
pub const AlignLeft: c_int = 0x0000;
pub const AlignRight: c_int = 0x1000;
pub const Justify: c_int = 0x2000;
pub const Center: c_int = 0x4000;
pub const Centre: c_int = 0x4000;
pub const AlignCenter: c_int = 0x4000;
pub const AlignCentre: c_int = 0x4000;
pub const Underline: c_int = 0x0800;

// Dialog return values
pub const YES: c_int = 1;
pub const NO: c_int = -1;
pub const CANCEL: c_int = 0;

// Object type constants (internal)
pub const BaseObject: c_int = 0x4000;
pub const Image8: c_int = 0x0008;
pub const Image32: c_int = 0x0020;
pub const ControlObject: c_int = 0x1000;
pub const WindowObject: c_int = 0x1100;
pub const BitmapObject: c_int = 0x0200;
pub const CursorObject: c_int = 0x0400;
pub const FontObject: c_int = 0x0800;
pub const UserObject: c_int = 0x1080;
pub const LabelObject: c_int = 0x1001;
pub const ButtonObject: c_int = 0x1004;
pub const CheckboxObject: c_int = 0x1005;
pub const RadioObject: c_int = 0x1006;
pub const ScrollbarObject: c_int = 0x1008;
pub const FieldObject: c_int = 0x1011;
pub const TextboxObject: c_int = 0x1012;
pub const ListboxObject: c_int = 0x1020;
pub const MultilistObject: c_int = 0x1021;
pub const DroplistObject: c_int = 0x1022;
pub const DropfieldObject: c_int = 0x1023;
pub const ProgressbarObject: c_int = 0x1024;
pub const MenubarObject: c_int = 0x0041;
pub const MenuObject: c_int = 0x0042;
pub const MenuitemObject: c_int = 0x0048;
pub const RadiogroupObject: c_int = 0x2006;
pub const PrinterObject: c_int = 0x0030;
pub const MetafileObject: c_int = 0x0050;

// DblClick (from ga.h)
pub const DblClick: c_int = 0x0010;

// Menu item markers (from ga.h)
pub const STARTMENU_NUL: c_long = 0;
pub const ENDMENU_NUL: c_long = 0;
pub const STARTSUBMENU_NUL: c_long = 0;
pub const ENDSUBMENU_NUL: c_long = 0;
pub const MDIMENU_NUL: c_long = 0;
pub const LASTMENUITEM_NUL: c_long = 0;

// Line styles (from ga.h)
pub const lSolid: c_int = 0;
pub const lDash: c_int = 5 | (4 << 4);
pub const lShortDash: c_int = 3 | (4 << 4);
pub const lLongDash: c_int = 8 | (4 << 4);
pub const lDot: c_int = 1 | (4 << 4);
pub const lDashDot: c_int = 5 | (4 << 4) | (1 << 8) | (4 << 12);
pub const lShortDashDot: c_int = 3 | (4 << 4) | (1 << 8) | (4 << 12);
pub const lLongDashDot: c_int = 8 | (4 << 4) | (1 << 8) | (4 << 12);
pub const lDashDotDot: c_int = 5 | (4 << 4) | (1 << 8) | (3 << 12) | (1 << 16) | (4 << 20);
pub const lShortDashDotDot: c_int = 3 | (4 << 4) | (1 << 8) | (3 << 12) | (1 << 16) | (4 << 20);
pub const lLongDashDotDot: c_int = 8 | (4 << 4) | (1 << 8) | (3 << 12) | (1 << 16) | (4 << 20);

// Scrollbar types
pub const HWINSB: c_int = 0;
pub const VWINSB: c_int = 1;
pub const CONTROLSB: c_int = 2;

// Internal constants
pub const MinMenuID: c_uint = 0x0100;
pub const MinChildID: c_uint = 0x6000;
pub const MinDocID: c_uint = 0xE000;

// ============================================================
// Type aliases
// ============================================================

pub type GAbyte = u8;
pub type rgb = c_ulong;
pub type objptr = *mut ObjInfo;

pub type font = objptr;
pub type cursor = objptr;
pub type drawing = objptr;
pub type bitmap = objptr;
pub type window = objptr;
pub type control = objptr;

pub type label = objptr;
pub type button = objptr;
pub type checkbox = objptr;
pub type radiobutton = objptr;
pub type radiogroup = objptr;
pub type field = objptr;
pub type textbox = objptr;
pub type scrollbar = objptr;
pub type listbox = objptr;
pub type progressbar = objptr;

pub type menubar = objptr;
pub type menu = objptr;
pub type menuitem = objptr;

pub type printer = objptr;
pub type metafile = objptr;

// ============================================================
// Callback function pointer types
// ============================================================

pub type voidfn = Option<unsafe extern "C" fn()>;
pub type timerfn = Option<unsafe extern "C" fn(data: *mut c_void)>;
pub type actionfn = Option<unsafe extern "C" fn(c: control)>;
pub type drawfn = Option<unsafe extern "C" fn(c: control, r: rect)>;
pub type mousefn = Option<unsafe extern "C" fn(c: control, buttons: c_int, xy: point)>;
pub type intfn = Option<unsafe extern "C" fn(c: control, argument: c_int)>;
pub type keyfn = Option<unsafe extern "C" fn(c: control, key: c_int)>;
pub type menufn = Option<unsafe extern "C" fn(m: menuitem)>;
pub type scrollfn = Option<unsafe extern "C" fn(s: scrollbar, position: c_int)>;
pub type dropfn = Option<unsafe extern "C" fn(c: control, data: *mut c_char)>;
pub type imfn = Option<unsafe extern "C" fn(c: control, f: *mut font, xy: *mut point)>;

// ============================================================
// Structures
// ============================================================

/// A 2D point with integer coordinates.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct point {
    pub x: c_int,
    pub y: c_int,
}

/// A rectangle defined by top-left corner, width, and height.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct rect {
    pub x: c_int,
    pub y: c_int,
    pub width: c_int,
    pub height: c_int,
}

/// The drawing state: current destination, colour, mode, position, etc.
#[repr(C)]
#[derive(Clone, Debug)]
pub struct drawstruct {
    pub dest: drawing,
    pub hue: rgb,
    pub mode: c_int,
    pub p: point,
    pub linewidth: c_int,
    pub fnt: font,
    pub crsr: cursor,
}

/// Platform-independent image data.
#[repr(C)]
#[derive(Clone, Debug)]
pub struct imagedata {
    pub depth: c_int,
    pub width: c_int,
    pub height: c_int,
    pub cmapsize: c_int,
    pub cmap: *mut rgb,
    pub pixels: *mut GAbyte,
}

pub type image = *mut imagedata;
pub type drawstate = *mut drawstruct;

/// Internal callback information structure.
#[repr(C)]
#[derive(Clone, Debug)]
pub struct callinfo {
    pub die: actionfn,
    pub close: actionfn,
    pub redraw: drawfn,
    pub resize: drawfn,
    pub keydown: keyfn,
    pub keyaction: keyfn,
    pub mousedown: mousefn,
    pub mouseup: mousefn,
    pub mousemove: mousefn,
    pub mousedrag: mousefn,
    pub mouserepeat: mousefn,
    pub drop: dropfn,
    pub im: imfn,
    pub focus: actionfn,
}

/// The internal object information structure.
///
/// This is the heart of GraphApp's object system. Every window, control,
/// bitmap, font, cursor, etc. is represented as an ObjInfo.
#[repr(C)]
#[derive(Clone, Debug)]
pub struct ObjInfo {
    pub kind: c_int,
    pub refcount: c_int,
    pub handle: *mut c_void,
    pub menubar: object,
    pub popup: object,
    pub toolbar: object,
    pub status: [c_char; 256],
    pub next: object,
    pub prev: object,
    pub parent: object,
    pub child: object,
    pub die: actionfn,
    pub rect: rect,
    pub depth: c_int,
    pub drawstate: drawstate,
    pub img: image,
    pub id: c_int,
    pub state: c_long,
    pub flags: c_long,
    pub data: *mut c_void,
    pub text: *mut c_char,
    pub fg: rgb,
    pub bg: rgb,
    pub action: actionfn,
    pub dble: actionfn,
    pub hit: intfn,
    pub value: c_int,
    pub key: c_int,
    pub shortcut: c_int,
    pub max: c_int,
    pub size: c_int,
    pub xmax: c_int,
    pub xsize: c_int,
    pub call: *mut callinfo,
    pub extra: *mut c_void,
    pub winproc: *const c_void,
    pub caretwidth: c_int,
    pub caretheight: c_int,
    pub caretshowing: c_int,
    pub caretexists: c_int,
    pub caretx: c_int,
    pub carety: c_int,
    pub edit_winproc: *const c_void,
}

/// Object pointer type (same as objptr).
pub type object = objptr;

/// MenuItem structure for creating menus from arrays.
#[repr(C)]
#[derive(Clone, Debug)]
pub struct MenuItem {
    pub nm: *mut c_char,
    pub fn_: menufn,
    pub key: c_int,
    pub m: menuitem,
}

// ============================================================
// Inline helper functions for rgb
// ============================================================

/// Construct an rgb value from red, green, blue components.
#[inline]
pub const fn rgb_make(r: c_ulong, g: c_ulong, b: c_ulong) -> rgb {
    (r << 16) | (g << 8) | b
}

/// Extract the alpha component from an rgb value.
#[inline]
pub const fn getalpha(col: rgb) -> c_ulong {
    (col >> 24) & 0x00FF
}

/// Extract the red component from an rgb value.
#[inline]
pub const fn getred(col: rgb) -> c_ulong {
    (col >> 16) & 0x00FF
}

/// Extract the green component from an rgb value.
#[inline]
pub const fn getgreen(col: rgb) -> c_ulong {
    (col >> 8) & 0x00FF
}

/// Extract the blue component from an rgb value.
#[inline]
pub const fn getblue(col: rgb) -> c_ulong {
    col & 0x00FF
}

// Predefined colours
pub const gaRed: rgb = 0x00FF0000;
pub const gaGreen: rgb = 0x0000FF00;
pub const gaBlue: rgb = 0x000000FF;
pub const Transparent: rgb = 0xFFFFFFFF;
pub const Black: rgb = 0x00000000;
pub const White: rgb = 0x00FFFFFF;
pub const Yellow: rgb = 0x00FFFF00;
pub const Magenta: rgb = 0x00FF00FF;
pub const Cyan: rgb = 0x0000FFFF;
pub const Grey: rgb = 0x00808080;
pub const Gray: rgb = 0x00808080;
pub const LightGrey: rgb = 0x00C0C0C0;
pub const LightGray: rgb = 0x00C0C0C0;
pub const DarkGrey: rgb = 0x00404040;
pub const DarkGray: rgb = 0x00404040;
pub const DarkBlue: rgb = 0x00000080;
pub const DarkGreen: rgb = 0x00008000;
pub const DarkRed: rgb = 0x008B0000;
pub const LightBlue: rgb = 0x0080C0FF;
pub const LightGreen: rgb = 0x0080FF80;
pub const LightRed: rgb = 0x00FFC0FF;
pub const Pink: rgb = 0x00FFAFAF;
pub const Brown: rgb = 0x00603000;
pub const Orange: rgb = 0x00FF8000;
pub const Purple: rgb = 0x00C000FF;
pub const Lime: rgb = 0x0080FF00;

// ============================================================
// Point and rectangle arithmetic
// ============================================================

pub unsafe fn newpoint(x: c_int, y: c_int) -> point {
    point { x, y }
}

pub unsafe fn newrect(left: c_int, top: c_int, width: c_int, height: c_int) -> rect {
    rect {
        x: left,
        y: top,
        width,
        height,
    }
}

pub unsafe fn rpt(min: point, max: point) -> rect {
    rect {
        x: min.x,
        y: min.y,
        width: max.x - min.x,
        height: max.y - min.y,
    }
}

pub unsafe fn topleft(r: rect) -> point {
    point { x: r.x, y: r.y }
}

pub unsafe fn bottomright(r: rect) -> point {
    point {
        x: r.x + r.width,
        y: r.y + r.height,
    }
}

pub unsafe fn topright(r: rect) -> point {
    point {
        x: r.x + r.width,
        y: r.y,
    }
}

pub unsafe fn bottomleft(r: rect) -> point {
    point {
        x: r.x,
        y: r.y + r.height,
    }
}

pub unsafe fn addpt(p1: point, p2: point) -> point {
    point {
        x: p1.x + p2.x,
        y: p1.y + p2.y,
    }
}

pub unsafe fn subpt(p1: point, p2: point) -> point {
    point {
        x: p1.x - p2.x,
        y: p1.y - p2.y,
    }
}

pub unsafe fn midpt(p1: point, p2: point) -> point {
    point {
        x: (p1.x + p2.x) / 2,
        y: (p1.y + p2.y) / 2,
    }
}

pub unsafe fn mulpt(p1: point, i: c_int) -> point {
    point {
        x: p1.x * i,
        y: p1.y * i,
    }
}

pub unsafe fn divpt(p1: point, i: c_int) -> point {
    point {
        x: p1.x / i,
        y: p1.y / i,
    }
}

pub unsafe fn rmove(r: rect, p: point) -> rect {
    rect {
        x: r.x + p.x,
        y: r.y + p.y,
        ..r
    }
}

pub unsafe fn raddpt(r: rect, p: point) -> rect {
    unsafe { rmove(r, p) }
}

pub unsafe fn rsubpt(r: rect, p: point) -> rect {
    rect {
        x: r.x - p.x,
        y: r.y - p.y,
        ..r
    }
}

pub unsafe fn rmul(r: rect, i: c_int) -> rect {
    rect {
        x: r.x * i,
        y: r.y * i,
        width: r.width * i,
        height: r.height * i,
    }
}

pub unsafe fn rdiv(r: rect, i: c_int) -> rect {
    rect {
        x: r.x / i,
        y: r.y / i,
        width: r.width / i,
        height: r.height / i,
    }
}

pub unsafe fn growr(r: rect, w: c_int, h: c_int) -> rect {
    rect {
        x: r.x - w,
        y: r.y - h,
        width: r.width + 2 * w,
        height: r.height + 2 * h,
    }
}

pub unsafe fn insetr(r: rect, i: c_int) -> rect {
    unsafe { growr(r, -i, -i) }
}

pub unsafe fn rcenter(r1: rect, r2: rect) -> rect {
    rect {
        x: r2.x + (r2.width - r1.width) / 2,
        y: r2.y + (r2.height - r1.height) / 2,
        ..r1
    }
}

pub unsafe fn ptinr(p: point, r: rect) -> c_int {
    if p.x >= r.x && p.x < r.x + r.width && p.y >= r.y && p.y < r.y + r.height {
        1
    } else {
        0
    }
}

pub unsafe fn rinr(r1: rect, r2: rect) -> c_int {
    if r1.x >= r2.x
        && r1.y >= r2.y
        && r1.x + r1.width <= r2.x + r2.width
        && r1.y + r1.height <= r2.y + r2.height
    {
        1
    } else {
        0
    }
}

pub unsafe fn rxr(r1: rect, r2: rect) -> c_int {
    if r1.x < r2.x + r2.width
        && r1.y < r2.y + r2.height
        && r1.x + r1.width > r2.x
        && r1.y + r1.height > r2.y
    {
        1
    } else {
        0
    }
}

pub unsafe fn equalpt(p1: point, p2: point) -> c_int {
    if p1.x == p2.x && p1.y == p2.y { 1 } else { 0 }
}

pub unsafe fn equalr(r1: rect, r2: rect) -> c_int {
    if r1.x == r2.x && r1.y == r2.y && r1.width == r2.width && r1.height == r2.height {
        1
    } else {
        0
    }
}

pub unsafe fn clipr(r1: rect, r2: rect) -> rect {
    let x1 = r1.x.max(r2.x);
    let y1 = r1.y.max(r2.y);
    let x2 = (r1.x + r1.width).min(r2.x + r2.width);
    let y2 = (r1.y + r1.height).min(r2.y + r2.height);
    if x2 > x1 && y2 > y1 {
        rect {
            x: x1,
            y: y1,
            width: x2 - x1,
            height: y2 - y1,
        }
    } else {
        rect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        }
    }
}

pub unsafe fn rcanon(r: rect) -> rect {
    let (x, y, w, h) = if r.width < 0 {
        (r.x + r.width, r.y, -r.width, r.height)
    } else {
        (r.x, r.y, r.width, r.height)
    };
    let (x, y, w, h) = if h < 0 {
        (x, y + h, w, -h)
    } else {
        (x, y, w, h)
    };
    rect {
        x,
        y,
        width: w,
        height: h,
    }
}

// ============================================================
// Object property accessors
// ============================================================

pub unsafe fn objdepth(obj: objptr) -> c_int {
    unsafe { if obj.is_null() { 0 } else { (*obj).depth } }
}

pub unsafe fn objrect(obj: objptr) -> rect {
    unsafe {
        if obj.is_null() {
            rect::default()
        } else {
            (*obj).rect
        }
    }
}

pub unsafe fn objwidth(obj: objptr) -> c_int {
    unsafe { if obj.is_null() { 0 } else { (*obj).rect.width } }
}

pub unsafe fn objheight(obj: objptr) -> c_int {
    unsafe { if obj.is_null() { 0 } else { (*obj).rect.height } }
}

/// Null pointer constant for convenience.
pub const NULL_HANDLE: *mut c_void = ptr::null_mut();
