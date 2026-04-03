/*
  tre/ast.rs - Abstract syntax tree (AST) definitions and routines

  Ported from tre-ast.c, tre-ast.h, tre-internal.h, tre-compile.h,
  tre-parse.h, tre-match-utils.h, tre-config.h, tre.h
*/

#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_int, c_long, c_void};
use std::ptr;

// ===== Configuration =====
pub const TRE_APPROX: bool = true;
pub const TRE_MULTIBYTE: bool = true;
pub const TRE_WCHAR: bool = true;
pub const TRE_VERSION: &str = "0.8.0";

// ===== tre.h types =====

pub type regoff_t = c_int;

#[repr(C)]
pub struct regex_t {
    pub re_nsub: usize,
    pub value: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct regmatch_t {
    pub rm_so: regoff_t,
    pub rm_eo: regoff_t,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum reg_errcode_t {
    REG_OK = 0,
    REG_NOMATCH = 1,
    REG_BADPAT = 2,
    REG_ECOLLATE = 3,
    REG_ECTYPE = 4,
    REG_EESCAPE = 5,
    REG_ESUBREG = 6,
    REG_EBRACK = 7,
    REG_EPAREN = 8,
    REG_EBRACE = 9,
    REG_BADBR = 10,
    REG_ERANGE = 11,
    REG_ESPACE = 12,
    REG_BADRPT = 13,
}

// POSIX tre_regcomp() flags.
pub const REG_EXTENDED: c_int = 1;
pub const REG_ICASE: c_int = 2;
pub const REG_NEWLINE: c_int = 4;
pub const REG_NOSUB: c_int = 8;

// Extra tre_regcomp() flags.
pub const REG_BASIC: c_int = 0;
pub const REG_LITERAL: c_int = 16;
pub const REG_RIGHT_ASSOC: c_int = 32;
pub const REG_UNGREEDY: c_int = 64;
pub const REG_USEBYTES: c_int = 128;

// POSIX tre_regexec() flags.
pub const REG_NOTBOL: c_int = 1;
pub const REG_NOTEOL: c_int = 2;

// Extra tre_regexec() flags.
pub const REG_APPROX_MATCHER: c_int = 4;
pub const REG_BACKTRACKING_MATCHER: c_int = 8;

// REG_NOSPEC and REG_LITERAL mean the same thing.
pub const REG_NOSPEC: c_int = REG_LITERAL;

// Standalone error code constants for convenience
pub const REG_OK: c_int = reg_errcode_t::REG_OK as c_int;
pub const REG_NOMATCH: c_int = reg_errcode_t::REG_NOMATCH as c_int;
pub const REG_BADPAT: c_int = reg_errcode_t::REG_BADPAT as c_int;
pub const REG_ECOLLATE: c_int = reg_errcode_t::REG_ECOLLATE as c_int;
pub const REG_ECTYPE: c_int = reg_errcode_t::REG_ECTYPE as c_int;
pub const REG_EESCAPE: c_int = reg_errcode_t::REG_EESCAPE as c_int;
pub const REG_ESUBREG: c_int = reg_errcode_t::REG_ESUBREG as c_int;
pub const REG_EBRACK: c_int = reg_errcode_t::REG_EBRACK as c_int;
pub const REG_EPAREN: c_int = reg_errcode_t::REG_EPAREN as c_int;
pub const REG_EBRACE: c_int = reg_errcode_t::REG_EBRACE as c_int;
pub const REG_BADBR: c_int = reg_errcode_t::REG_BADBR as c_int;
pub const REG_ERANGE: c_int = reg_errcode_t::REG_ERANGE as c_int;
pub const REG_ESPACE: c_int = reg_errcode_t::REG_ESPACE as c_int;
pub const REG_BADRPT: c_int = reg_errcode_t::REG_BADRPT as c_int;

// The maximum number of iterations in a bound expression.
pub const RE_DUP_MAX: c_int = 255;

// ===== tre-internal.h types =====

pub type tre_char_t = u32; // wchar_t equivalent
pub type tre_cint_t = u32; // wint_t equivalent

pub const TRE_CHAR_MAX: tre_cint_t = i32::MAX as tre_cint_t;

// Character type - for now we use a function pointer approach
// since we can't use wctype_t directly in Rust.
// We store the ctype as a function pointer or an index.
pub type tre_ctype_t = usize;

pub const ASSERT_AT_BOL: c_int = 1;
pub const ASSERT_AT_EOL: c_int = 2;
pub const ASSERT_CHAR_CLASS: c_int = 4;
pub const ASSERT_CHAR_CLASS_NEG: c_int = 8;
pub const ASSERT_AT_BOW: c_int = 16;
pub const ASSERT_AT_EOW: c_int = 32;
pub const ASSERT_AT_WB: c_int = 64;
pub const ASSERT_AT_WB_NEG: c_int = 128;
pub const ASSERT_BACKREF: c_int = 256;
pub const ASSERT_LAST: c_int = 256;

// Tag directions.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum tre_tag_direction_t {
    TRE_TAG_MINIMIZE = 0,
    TRE_TAG_MAXIMIZE = 1,
}

// Parameters that can be changed dynamically while matching.
pub const TRE_PARAM_COST_INS: usize = 0;
pub const TRE_PARAM_COST_DEL: usize = 1;
pub const TRE_PARAM_COST_SUBST: usize = 2;
pub const TRE_PARAM_COST_MAX: usize = 3;
pub const TRE_PARAM_MAX_INS: usize = 4;
pub const TRE_PARAM_MAX_DEL: usize = 5;
pub const TRE_PARAM_MAX_SUBST: usize = 6;
pub const TRE_PARAM_MAX_ERR: usize = 7;
pub const TRE_PARAM_DEPTH: usize = 8;
pub const TRE_PARAM_LAST: usize = 9;

// Unset matching parameter
pub const TRE_PARAM_UNSET: c_int = -1;
// Signifies the default matching parameter value.
pub const TRE_PARAM_DEFAULT: c_int = -2;

// String type
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum tre_str_type_t {
    STR_WIDE = 0,
    STR_BYTE = 1,
    STR_MBS = 2,
    STR_USER = 3,
}

// ===== tre-ast.h types =====

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum tre_ast_type_t {
    LITERAL = 0,
    CATENATION = 1,
    ITERATION = 2,
    UNION = 3,
}

// Special subtypes of LITERAL.
pub const EMPTY: c_long = -1;
pub const ASSERTION: c_long = -2;
pub const TAG: c_long = -3;
pub const BACKREF: c_long = -4;
pub const PARAMETER: c_long = -5;

#[inline]
pub fn IS_SPECIAL(lit: &tre_literal_t) -> bool {
    lit.code_min < 0
}

#[inline]
pub fn IS_EMPTY(lit: &tre_literal_t) -> bool {
    lit.code_min == EMPTY
}

#[inline]
pub fn IS_ASSERTION(lit: &tre_literal_t) -> bool {
    lit.code_min == ASSERTION
}

#[inline]
pub fn IS_TAG(lit: &tre_literal_t) -> bool {
    lit.code_min == TAG
}

#[inline]
pub fn IS_BACKREF(lit: &tre_literal_t) -> bool {
    lit.code_min == BACKREF
}

#[inline]
pub fn IS_PARAMETER(lit: &tre_literal_t) -> bool {
    lit.code_min == PARAMETER
}

// A generic AST node.
#[repr(C)]
pub struct tre_ast_node_t {
    pub type_: tre_ast_type_t,
    pub obj: *mut c_void,
    pub nullable: c_int,
    pub submatch_id: c_int,
    pub num_submatches: c_int,
    pub num_tags: c_int,
    pub firstpos: *mut tre_pos_and_tags_t,
    pub lastpos: *mut tre_pos_and_tags_t,
}

// A "literal" node.
#[repr(C)]
pub struct tre_literal_t {
    pub code_min: c_long,
    pub code_max: c_long,
    pub position: c_int,
    // Union: class or params
    pub class_or_params: *mut c_void,
    pub neg_classes: *mut tre_ctype_t,
}

impl tre_literal_t {
    #[inline]
    pub fn get_class(&self) -> tre_ctype_t {
        self.class_or_params as tre_ctype_t
    }

    #[inline]
    pub fn get_params(&self) -> *mut c_int {
        self.class_or_params as *mut c_int
    }

    #[inline]
    pub unsafe fn set_class(&mut self, class: tre_ctype_t) {
        self.class_or_params = class as *mut c_void;
    }

    #[inline]
    pub unsafe fn set_params(&mut self, params: *mut c_int) {
        self.class_or_params = params as *mut c_void;
    }
}

// A "catenation" node.
#[repr(C)]
pub struct tre_catenation_t {
    pub left: *mut tre_ast_node_t,
    pub right: *mut tre_ast_node_t,
}

// An "iteration" node.
#[repr(C)]
pub struct tre_iteration_t {
    pub arg: *mut tre_ast_node_t,
    pub min: c_int,
    pub max: c_int,
    pub minimal: u32, // bitfield
    pub params: *mut c_int,
}

// An "union" node.
#[repr(C)]
pub struct tre_union_t {
    pub left: *mut tre_ast_node_t,
    pub right: *mut tre_ast_node_t,
}

// ===== tre-compile.h types =====

#[repr(C)]
#[derive(Clone, Copy)]
pub struct tre_pos_and_tags_t {
    pub position: c_int,
    pub code_min: c_int,
    pub code_max: c_int,
    pub tags: *mut c_int,
    pub assertions: c_int,
    pub class: tre_ctype_t,
    pub neg_classes: *mut tre_ctype_t,
    pub backref: c_int,
    pub params: *mut c_int,
}

// ===== TNFA types =====

#[repr(C)]
pub struct tre_tnfa_transition_t {
    pub code_min: tre_cint_t,
    pub code_max: tre_cint_t,
    pub state: *mut tre_tnfa_transition_t,
    pub state_id: c_int,
    pub tags: *mut c_int,
    pub params: *mut c_int,
    pub assertions: c_int,
    pub u: tre_transition_u,
    pub neg_classes: *mut tre_ctype_t,
}

#[repr(C)]
pub union tre_transition_u {
    pub class: tre_ctype_t,
    pub backref: c_int,
}

impl tre_transition_u {
    pub unsafe fn get_class(&self) -> tre_ctype_t {
        unsafe { self.class }
    }

    pub unsafe fn get_backref(&self) -> c_int {
        unsafe { self.backref }
    }

    pub unsafe fn set_class(&mut self, class: tre_ctype_t) {
        self.class = class;
    }

    pub unsafe fn set_backref(&mut self, backref: c_int) {
        self.backref = backref;
    }
}

// Submatch data
#[repr(C)]
pub struct tre_submatch_data_t {
    pub so_tag: c_int,
    pub eo_tag: c_int,
    pub parents: *mut c_int,
}

// TNFA definition
#[repr(C)]
pub struct tre_tnfa_t {
    pub transitions: *mut tre_tnfa_transition_t,
    pub num_transitions: u32,
    pub initial: *mut tre_tnfa_transition_t,
    pub final_: *mut tre_tnfa_transition_t,
    pub submatch_data: *mut tre_submatch_data_t,
    pub firstpos_chars: *mut c_char,
    pub first_char: c_int,
    pub num_submatches: u32,
    pub tag_directions: *mut c_int,
    pub minimal_tags: *mut c_int,
    pub num_tags: c_int,
    pub num_minimals: c_int,
    pub end_tag: c_int,
    pub num_states: c_int,
    pub cflags: c_int,
    pub have_backrefs: c_int,
    pub have_approx: c_int,
    pub params_depth: c_int,
}

// ===== tre-parse.h types =====

#[repr(C)]
pub struct tre_parse_ctx_t {
    pub mem: super::mem::tre_mem_t,
    pub stack: *mut super::stack::tre_stack_rec,
    pub result: *mut tre_ast_node_t,
    pub re: *const tre_char_t,
    pub re_start: *const tre_char_t,
    pub re_end: *const tre_char_t,
    pub len: usize,
    pub submatch_id: c_int,
    pub position: c_int,
    pub max_backref: c_int,
    pub have_approx: c_int,
    pub cflags: c_int,
    pub nofirstsub: c_int,
    pub params: [c_int; TRE_PARAM_LAST],
    pub cur_max: c_int,
}

// ===== Approximate matching types =====

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct regaparams_t {
    pub cost_ins: c_int,
    pub cost_del: c_int,
    pub cost_subst: c_int,
    pub max_cost: c_int,
    pub max_ins: c_int,
    pub max_del: c_int,
    pub max_subst: c_int,
    pub max_err: c_int,
}

#[repr(C)]
pub struct regamatch_t {
    pub nmatch: usize,
    pub pmatch: *mut regmatch_t,
    pub cost: c_int,
    pub num_ins: c_int,
    pub num_del: c_int,
    pub num_subst: c_int,
}

// ===== tre_str_source =====

pub type tre_get_next_char_fn =
    unsafe extern "C" fn(c: *mut tre_char_t, pos_add: *mut u32, context: *mut c_void) -> c_int;
pub type tre_rewind_fn = unsafe extern "C" fn(pos: usize, context: *mut c_void);
pub type tre_compare_fn =
    unsafe extern "C" fn(pos1: usize, pos2: usize, len: usize, context: *mut c_void) -> c_int;

#[repr(C)]
pub struct tre_str_source {
    pub get_next_char: Option<tre_get_next_char_fn>,
    pub rewind: Option<tre_rewind_fn>,
    pub compare: Option<tre_compare_fn>,
    pub context: *mut c_void,
}

// ===== Config =====

pub const TRE_CONFIG_APPROX: c_int = 0;
pub const TRE_CONFIG_WCHAR: c_int = 1;
pub const TRE_CONFIG_MULTIBYTE: c_int = 2;
pub const TRE_CONFIG_SYSTEM_ABI: c_int = 3;
pub const TRE_CONFIG_VERSION: c_int = 4;

// ===== Helper functions =====

#[inline]
pub fn MAX(a: c_int, b: c_int) -> c_int {
    if a >= b { a } else { b }
}

#[inline]
pub fn MIN(a: isize, b: isize) -> isize {
    if a <= b { a } else { b }
}

// ===== Tag ordering =====

pub unsafe fn tre_tag_order(
    num_tags: c_int,
    tag_directions: *const c_int,
    t1: *const c_int,
    t2: *const c_int,
) -> c_int {
    unsafe {
        for i in 0..num_tags as isize {
            if *tag_directions.offset(i) == tre_tag_direction_t::TRE_TAG_MINIMIZE as c_int {
                if *t1.offset(i) < *t2.offset(i) {
                    return 1;
                }
                if *t1.offset(i) > *t2.offset(i) {
                    return 0;
                }
            } else {
                if *t1.offset(i) > *t2.offset(i) {
                    return 1;
                }
                if *t1.offset(i) < *t2.offset(i) {
                    return 0;
                }
            }
        }
        0
    }
}

// ===== AST functions =====

use super::mem;

pub unsafe fn tre_ast_new_node(
    mem: mem::tre_mem_t,
    type_: tre_ast_type_t,
    size: usize,
) -> *mut tre_ast_node_t {
    unsafe {
        let node =
            mem::tre_mem_calloc(mem, std::mem::size_of::<tre_ast_node_t>()) as *mut tre_ast_node_t;
        if node.is_null() {
            return ptr::null_mut();
        }
        let obj = mem::tre_mem_calloc(mem, size);
        if obj.is_null() {
            return ptr::null_mut();
        }
        (*node).type_ = type_;
        (*node).nullable = -1;
        (*node).submatch_id = -1;
        (*node).obj = obj;
        node
    }
}

pub unsafe fn tre_ast_new_literal(
    mem: mem::tre_mem_t,
    code_min: c_int,
    code_max: c_int,
    position: c_int,
) -> *mut tre_ast_node_t {
    unsafe {
        let node = tre_ast_new_node(
            mem,
            tre_ast_type_t::LITERAL,
            std::mem::size_of::<tre_literal_t>(),
        );
        if node.is_null() {
            return ptr::null_mut();
        }
        let lit = (*node).obj as *mut tre_literal_t;
        (*lit).code_min = code_min as c_long;
        (*lit).code_max = code_max as c_long;
        (*lit).position = position;
        node
    }
}

pub unsafe fn tre_ast_new_iter(
    mem: mem::tre_mem_t,
    arg: *mut tre_ast_node_t,
    min: c_int,
    max: c_int,
    minimal: c_int,
) -> *mut tre_ast_node_t {
    unsafe {
        let node = tre_ast_new_node(
            mem,
            tre_ast_type_t::ITERATION,
            std::mem::size_of::<tre_iteration_t>(),
        );
        if node.is_null() {
            return ptr::null_mut();
        }
        let iter = (*node).obj as *mut tre_iteration_t;
        (*iter).arg = arg;
        (*iter).min = min;
        (*iter).max = max;
        (*iter).minimal = if minimal != 0 { 1 } else { 0 };
        if !arg.is_null() {
            (*node).num_submatches = (*arg).num_submatches;
        }
        node
    }
}

pub unsafe fn tre_ast_new_union(
    mem: mem::tre_mem_t,
    left: *mut tre_ast_node_t,
    right: *mut tre_ast_node_t,
) -> *mut tre_ast_node_t {
    unsafe {
        let node = tre_ast_new_node(
            mem,
            tre_ast_type_t::UNION,
            std::mem::size_of::<tre_union_t>(),
        );
        if node.is_null() {
            return ptr::null_mut();
        }
        let uni = (*node).obj as *mut tre_union_t;
        (*uni).left = left;
        (*uni).right = right;
        if !left.is_null() && !right.is_null() {
            let lsub = (*left).num_submatches;
            let rsub = (*right).num_submatches;
            // Guard against overflow
            if lsub > 0 && rsub > 0 && lsub > c_int::MAX - rsub {
                (*node).num_submatches = 0;
            } else {
                (*node).num_submatches = lsub + rsub;
            }
        }
        node
    }
}

pub unsafe fn tre_ast_new_catenation(
    mem: mem::tre_mem_t,
    left: *mut tre_ast_node_t,
    right: *mut tre_ast_node_t,
) -> *mut tre_ast_node_t {
    unsafe {
        let node = tre_ast_new_node(
            mem,
            tre_ast_type_t::CATENATION,
            std::mem::size_of::<tre_catenation_t>(),
        );
        if node.is_null() {
            return ptr::null_mut();
        }
        let cat = (*node).obj as *mut tre_catenation_t;
        (*cat).left = left;
        (*cat).right = right;
        if !left.is_null() && !right.is_null() {
            let lsub = (*left).num_submatches;
            let rsub = (*right).num_submatches;
            if lsub > 0 && rsub > 0 && lsub > c_int::MAX - rsub {
                (*node).num_submatches = 0;
            } else {
                (*node).num_submatches = lsub + rsub;
            }
        }
        node
    }
}
