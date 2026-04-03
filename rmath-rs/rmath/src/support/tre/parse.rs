/*
  tre/parse.rs - Regexp parser

  Ported from tre-parse.c
*/

#![allow(non_snake_case, non_upper_case_globals, dead_code)]
#![allow(clippy::identity_op, clippy::erasing_op)]

use std::os::raw::{c_int, c_long, c_void};
use std::ptr;

use super::ast::*;
use super::mem;
use super::stack;

// Characters with special meanings in regexp syntax.
const CHAR_PIPE: tre_char_t = '|' as tre_char_t;
const CHAR_LPAREN: tre_char_t = '(' as tre_char_t;
const CHAR_RPAREN: tre_char_t = ')' as tre_char_t;
const CHAR_LBRACE: tre_char_t = '{' as tre_char_t;
const CHAR_RBRACE: tre_char_t = '}' as tre_char_t;
const CHAR_LBRACKET: tre_char_t = '[' as tre_char_t;
const CHAR_RBRACKET: tre_char_t = ']' as tre_char_t;
const CHAR_MINUS: tre_char_t = '-' as tre_char_t;
const CHAR_STAR: tre_char_t = '*' as tre_char_t;
const CHAR_QUESTIONMARK: tre_char_t = '?' as tre_char_t;
const CHAR_PLUS: tre_char_t = '+' as tre_char_t;
const CHAR_PERIOD: tre_char_t = '.' as tre_char_t;
const CHAR_COLON: tre_char_t = ':' as tre_char_t;
const CHAR_EQUAL: tre_char_t = '=' as tre_char_t;
const CHAR_COMMA: tre_char_t = ',' as tre_char_t;
const CHAR_CARET: tre_char_t = '^' as tre_char_t;
const CHAR_DOLLAR: tre_char_t = '$' as tre_char_t;
const CHAR_BACKSLASH: tre_char_t = '\\' as tre_char_t;
const CHAR_HASH: tre_char_t = '#' as tre_char_t;
const CHAR_TILDE: tre_char_t = '~' as tre_char_t;

const CHAR_SPACE: tre_char_t = ' ' as tre_char_t;
const CHAR_LCURLY: tre_char_t = '{' as tre_char_t;
const CHAR_RCURLY: tre_char_t = '}' as tre_char_t;

// Character type functions - simplified for byte mode
#[inline]
pub(crate) fn tre_isalnum(c: tre_cint_t) -> bool {
    (c as u8).is_ascii_alphanumeric()
}

#[inline]
pub(crate) fn tre_isalpha(c: tre_cint_t) -> bool {
    (c as u8).is_ascii_alphabetic()
}

#[inline]
pub(crate) fn tre_isascii(c: tre_cint_t) -> bool {
    (c as u32) < 128
}

#[inline]
pub(crate) fn tre_isblank(c: tre_cint_t) -> bool {
    c == ' ' as tre_cint_t || c == '\t' as tre_cint_t
}

#[inline]
pub(crate) fn tre_iscntrl(c: tre_cint_t) -> bool {
    c <= 0x1f || c == 0x7f
}

#[inline]
pub(crate) fn tre_isdigit(c: tre_cint_t) -> bool {
    (c as u8).is_ascii_digit()
}

#[inline]
pub(crate) fn tre_isgraph(c: tre_cint_t) -> bool {
    c > ' ' as tre_cint_t && c != 0x7f
}

#[inline]
pub(crate) fn tre_islower(c: tre_cint_t) -> bool {
    (c as u8).is_ascii_lowercase()
}

#[inline]
pub(crate) fn tre_isprint(c: tre_cint_t) -> bool {
    c >= ' ' as tre_cint_t && c != 0x7f
}

#[inline]
pub(crate) fn tre_ispunct(c: tre_cint_t) -> bool {
    tre_isgraph(c) && !tre_isalnum(c)
}

#[inline]
pub(crate) fn tre_isspace(c: tre_cint_t) -> bool {
    c == ' ' as tre_cint_t
        || c == '\t' as tre_cint_t
        || c == '\n' as tre_cint_t
        || c == '\r' as tre_cint_t
        || c == 0x0b as tre_cint_t
        || c == 0x0c as tre_cint_t
}

#[inline]
pub(crate) fn tre_isupper(c: tre_cint_t) -> bool {
    (c as u8).is_ascii_uppercase()
}

#[inline]
pub(crate) fn tre_isxdigit(c: tre_cint_t) -> bool {
    (c as u8).is_ascii_hexdigit()
}

#[inline]
pub(crate) fn tre_tolower(c: tre_cint_t) -> tre_cint_t {
    if tre_isupper(c) { c + 32 } else { c }
}

#[inline]
pub(crate) fn tre_toupper(c: tre_cint_t) -> tre_cint_t {
    if tre_islower(c) { c - 32 } else { c }
}

pub(crate) fn tre_isctype(c: tre_cint_t, class: tre_ctype_t) -> bool {
    if class == 0 {
        return false;
    }
    let f: fn(tre_cint_t) -> bool = unsafe { std::mem::transmute(class) };
    f(c)
}

pub(crate) fn tre_neg_char_classes_match(
    classes: *const tre_ctype_t,
    wc: tre_cint_t,
    icase: c_int,
) -> c_int {
    let mut i = 0;
    while !classes.is_null() {
        let cls = unsafe { *classes.offset(i) };
        if cls == 0 {
            return 0;
        }
        if icase == 0 && tre_isctype(wc, cls)
            || (icase != 0
                && (tre_isctype(tre_toupper(wc), cls) || tre_isctype(tre_tolower(wc), cls)))
        {
            return 1;
        }
        i += 1;
    }
    0
}

type tre_ctype_func_t = fn(tre_cint_t) -> bool;

fn tre_ctype(name: &str) -> tre_ctype_t {
    let func: Option<tre_ctype_func_t> = match name {
        "alnum" => Some(tre_isalnum as tre_ctype_func_t),
        "alpha" => Some(tre_isalpha as tre_ctype_func_t),
        "ascii" => Some(tre_isascii as tre_ctype_func_t),
        "blank" => Some(tre_isblank as tre_ctype_func_t),
        "cntrl" => Some(tre_iscntrl as tre_ctype_func_t),
        "digit" => Some(tre_isdigit as tre_ctype_func_t),
        "graph" => Some(tre_isgraph as tre_ctype_func_t),
        "lower" => Some(tre_islower as tre_ctype_func_t),
        "print" => Some(tre_isprint as tre_ctype_func_t),
        "punct" => Some(tre_ispunct as tre_ctype_func_t),
        "space" => Some(tre_isspace as tre_ctype_func_t),
        "upper" => Some(tre_isupper as tre_ctype_func_t),
        "xdigit" => Some(tre_isxdigit as tre_ctype_func_t),
        _ => return 0,
    };
    match func {
        Some(f) => unsafe { std::mem::transmute(f as *const ()) },
        None => 0,
    }
}

// Macros for expanding \w, \s, etc.
struct tre_macro {
    c: char,
    expansion: &'static [u8],
}

static TRE_MACROS: &[tre_macro] = &[
    tre_macro {
        c: 't',
        expansion: b"\t",
    },
    tre_macro {
        c: 'n',
        expansion: b"\n",
    },
    tre_macro {
        c: 'r',
        expansion: b"\r",
    },
    tre_macro {
        c: 'f',
        expansion: b"\x0c",
    },
    tre_macro {
        c: 'a',
        expansion: b"\x07",
    },
    tre_macro {
        c: 'e',
        expansion: b"\x1b",
    },
    tre_macro {
        c: 'w',
        expansion: b"[[:alnum:]_]",
    },
    tre_macro {
        c: 'W',
        expansion: b"[^[:alnum:]_]",
    },
    tre_macro {
        c: 's',
        expansion: b"[[:space:]]",
    },
    tre_macro {
        c: 'S',
        expansion: b"[^[:space:]]",
    },
    tre_macro {
        c: 'd',
        expansion: b"[[:digit:]]",
    },
    tre_macro {
        c: 'D',
        expansion: b"[^[:digit:]]",
    },
];

fn tre_expand_macro(regex: &[tre_char_t], regex_end: usize, buf: &mut [tre_char_t]) {
    buf[0] = 0;
    if regex.is_empty() || regex_end == 0 {
        return;
    }
    let c = regex[0] as u8 as char;
    for macro_item in TRE_MACROS.iter() {
        if macro_item.c == c {
            for (j, &b) in macro_item.expansion.iter().enumerate() {
                if j >= buf.len() - 1 {
                    break;
                }
                buf[j] = b as tre_char_t;
            }
            buf[macro_item.expansion.len().min(buf.len() - 1)] = 0;
            break;
        }
    }
}

const MAX_NEG_CLASSES: usize = 64;

unsafe fn tre_new_item(
    mem: mem::tre_mem_t,
    min: c_int,
    max: c_int,
    i: &mut c_int,
    max_i: &mut c_int,
    items: &mut *mut *mut tre_ast_node_t,
) -> c_int {
    unsafe {
        if *i >= *max_i {
            if *max_i > 1024 {
                return REG_ESPACE;
            }
            *max_i *= 2;
            let new_items = mem::xrealloc(
                *items as *mut c_void,
                std::mem::size_of::<*mut tre_ast_node_t>() * (*max_i as usize),
            ) as *mut *mut tre_ast_node_t;
            if new_items.is_null() {
                return REG_ESPACE;
            }
            *items = new_items;
        }
        let array = *items;
        let node = tre_ast_new_literal(mem, min, max, -1);
        if node.is_null() {
            return REG_ESPACE;
        }
        *array.offset(*i as isize) = node;
        *i += 1;
        REG_OK
    }
}

unsafe fn tre_expand_ctype(
    mem: mem::tre_mem_t,
    class: tre_ctype_t,
    items: &mut *mut *mut tre_ast_node_t,
    i: &mut c_int,
    max_i: &mut c_int,
    cflags: c_int,
) -> c_int {
    unsafe {
        let mut status: c_int = REG_OK;
        let mut min: c_int = -1;
        let mut max: c_int = 0;

        for j in 0..256i32 {
            let c = j as tre_cint_t;
            if tre_isctype(c, class)
                || ((cflags & REG_ICASE) != 0
                    && (tre_isctype(tre_tolower(c), class) || tre_isctype(tre_toupper(c), class)))
            {
                if min < 0 {
                    min = c as c_int;
                }
                max = c as c_int;
            } else if min >= 0 {
                status = tre_new_item(mem, min, max, i, max_i, items);
                min = -1;
            }
        }
        if min >= 0 && status == REG_OK {
            status = tre_new_item(mem, min, max, i, max_i, items);
        }
        status
    }
}

unsafe fn tre_parse_bracket_items(
    ctx: &mut tre_parse_ctx_t,
    negate: c_int,
    neg_classes: &mut [tre_ctype_t],
    num_neg_classes: &mut c_int,
    items: &mut *mut *mut tre_ast_node_t,
    num_items: &mut c_int,
    items_size: &mut c_int,
) -> c_int {
    unsafe {
        let re_start = ctx.re;
        let re_end = ctx.re_end;
        let mut re = re_start;
        let mut status: c_int = REG_OK;
        let mut i = *num_items;
        let mut max_i = *items_size;

        loop {
            if status != REG_OK {
                break;
            }

            if re >= re_end {
                status = REG_EBRACK;
            } else if *re == CHAR_RBRACKET && re > re_start {
                re = re.offset(1);
                break;
            } else {
                let mut min: tre_cint_t = 0;
                let mut max: tre_cint_t = 0;
                let mut class: tre_ctype_t = 0;
                let mut skip: c_int = 0;

                if re.offset(2) < re_end
                    && *re.offset(1) == CHAR_MINUS
                    && *re.offset(2) != CHAR_RBRACKET
                {
                    min = *re;
                    max = *re.offset(2);
                    re = re.offset(3);
                    if min > max {
                        status = REG_ERANGE;
                    }
                } else if re.offset(1) < re_end
                    && *re == CHAR_LBRACKET
                    && *re.offset(1) == CHAR_PERIOD
                {
                    status = REG_ECOLLATE;
                } else if re.offset(1) < re_end
                    && *re == CHAR_LBRACKET
                    && *re.offset(1) == CHAR_EQUAL
                {
                    status = REG_ECOLLATE;
                } else if re.offset(1) < re_end
                    && *re == CHAR_LBRACKET
                    && *re.offset(1) == CHAR_COLON
                {
                    let mut endptr = re.offset(2);
                    while endptr < re_end && *endptr != CHAR_COLON {
                        endptr = endptr.offset(1);
                    }
                    if endptr != re_end {
                        let len = MIN(
                            (endptr as isize - re as isize)
                                / std::mem::size_of::<tre_char_t>() as isize
                                - 2,
                            63,
                        );
                        let mut tmp_str = [0u8; 64];
                        for j in 0..len as usize {
                            tmp_str[j] = *re.offset(2 + j as isize) as u8;
                        }
                        tmp_str[len as usize] = 0;

                        let name_end = tmp_str.iter().position(|&b| b == 0).unwrap_or(len as usize);
                        let name = std::str::from_utf8(&tmp_str[..name_end]).unwrap_or("");

                        class = tre_ctype(name);
                        if class == 0 {
                            status = REG_ECTYPE;
                        }
                        if status == REG_OK && (*ctx).cur_max == 1 {
                            status = tre_expand_ctype(
                                ctx.mem,
                                class,
                                items,
                                &mut i,
                                &mut max_i,
                                (*ctx).cflags,
                            );
                            class = 0;
                            skip = 1;
                        }
                        re = endptr.offset(2);
                    } else {
                        status = REG_ECTYPE;
                    }
                    min = 0;
                    max = TRE_CHAR_MAX as tre_cint_t;
                } else {
                    if *re == CHAR_MINUS
                        && re.offset(1) < re_end
                        && *re.offset(1) != CHAR_RBRACKET
                        && re > re_start
                    {
                        status = REG_ERANGE;
                    }
                    min = *re;
                    max = *re;
                    re = re.offset(1);
                }

                if status != REG_OK {
                    break;
                }

                if class != 0 && negate != 0 {
                    if *num_neg_classes >= MAX_NEG_CLASSES as c_int {
                        status = REG_ESPACE;
                    } else {
                        neg_classes[*num_neg_classes as usize] = class;
                        *num_neg_classes += 1;
                    }
                } else if skip == 0 {
                    status = tre_new_item(
                        ctx.mem,
                        min as c_int,
                        max as c_int,
                        &mut i,
                        &mut max_i,
                        items,
                    );
                    if status != REG_OK {
                        break;
                    }
                    let lit = (**items.offset((i - 1) as isize)).obj as *mut tre_literal_t;
                    (*lit).set_class(class);
                }

                if (*ctx).cflags & REG_ICASE != 0 && class == 0 && status == REG_OK && skip == 0 {
                    let mut m = min;
                    while m <= max {
                        if tre_islower(m) {
                            let cmin = tre_toupper(m);
                            let mut ccurr = cmin;
                            m += 1;
                            while m <= max && tre_islower(m) && tre_toupper(m) == ccurr + 1 {
                                ccurr = tre_toupper(m);
                                m += 1;
                            }
                            status = tre_new_item(
                                ctx.mem,
                                cmin as c_int,
                                ccurr as c_int,
                                &mut i,
                                &mut max_i,
                                items,
                            );
                        } else if tre_isupper(m) {
                            let cmin = tre_tolower(m);
                            let mut ccurr = cmin;
                            m += 1;
                            while m <= max && tre_isupper(m) && tre_tolower(m) == ccurr + 1 {
                                ccurr = tre_tolower(m);
                                m += 1;
                            }
                            status = tre_new_item(
                                ctx.mem,
                                cmin as c_int,
                                ccurr as c_int,
                                &mut i,
                                &mut max_i,
                                items,
                            );
                        } else {
                            m += 1;
                        }
                        if status != REG_OK {
                            break;
                        }
                    }
                    if status != REG_OK {
                        break;
                    }
                }
            }
        }
        *num_items = i;
        *items_size = max_i;
        (*ctx).re = re;
        status
    }
}

unsafe fn tre_parse_bracket(ctx: &mut tre_parse_ctx_t, result: &mut *mut tre_ast_node_t) -> c_int {
    unsafe {
        let mut node: *mut tre_ast_node_t = ptr::null_mut();
        let mut negate: c_int = 0;
        let mut i: c_int = 0;
        let mut max_i: c_int = 32;
        let mut neg_classes = [0usize; MAX_NEG_CLASSES];
        let mut num_neg_classes: c_int = 0;

        let items: *mut *mut tre_ast_node_t =
            mem::xmalloc(std::mem::size_of::<*mut tre_ast_node_t>() * max_i as usize)
                as *mut *mut tre_ast_node_t;
        if items.is_null() {
            return REG_ESPACE;
        }

        if *ctx.re == CHAR_CARET {
            negate = 1;
            ctx.re = ctx.re.offset(1);
        }

        let mut items_mut = items;
        let mut status = tre_parse_bracket_items(
            ctx,
            negate,
            &mut neg_classes,
            &mut num_neg_classes,
            &mut items_mut,
            &mut i,
            &mut max_i,
        );
        let items = items_mut;

        if status != REG_OK {
            mem::xfree(items as *mut c_void);
            ctx.position += 1;
            *result = node;
            return status;
        }

        if negate != 0 {
            // Simple insertion sort
            let arr = std::slice::from_raw_parts_mut(items, i as usize);
            for j in 1..arr.len() {
                let key_ptr = arr[j];
                let key_min = (&(*((*key_ptr).obj as *mut tre_literal_t))).code_min;
                let mut k = j as isize - 1;
                while k >= 0 {
                    let k_min = (&(*((*arr[k as usize]).obj as *mut tre_literal_t))).code_min;
                    if k_min <= key_min {
                        break;
                    }
                    arr[(k + 1) as usize] = arr[k as usize];
                    k -= 1;
                }
                arr[(k + 1) as usize] = key_ptr;
            }
        }

        let mut curr_max: c_int = 0;
        let mut curr_min: c_int = 0;

        for j in 0..i as isize {
            if status != REG_OK {
                break;
            }
            let l = (**items.offset(j)).obj as *mut tre_literal_t;
            let min = (*l).code_min as c_int;
            let max = (*l).code_max as c_int;
            let mut use_item = true;

            if negate != 0 {
                if min < curr_max {
                    curr_max = MAX(max + 1, curr_max);
                    use_item = false;
                } else {
                    curr_max = min - 1;
                    if curr_max >= curr_min {
                        (*l).code_min = curr_min as c_long;
                        (*l).code_max = curr_max as c_long;
                    } else {
                        use_item = false;
                    }
                    curr_min = max + 1;
                    curr_max = max + 1;
                }
            }

            if use_item {
                (*l).position = ctx.position;
                if num_neg_classes > 0 {
                    (*l).neg_classes = mem::tre_mem_alloc(
                        ctx.mem,
                        std::mem::size_of::<tre_ctype_t>() * (num_neg_classes as usize + 1),
                    ) as *mut tre_ctype_t;
                    if (*l).neg_classes.is_null() {
                        status = REG_ESPACE;
                        break;
                    }
                    for k in 0..num_neg_classes as isize {
                        *(*l).neg_classes.offset(k) = neg_classes[k as usize];
                    }
                    *(*l).neg_classes.offset(num_neg_classes as isize) = 0;
                } else {
                    (*l).neg_classes = ptr::null_mut();
                }
                if node.is_null() {
                    node = *items.offset(j);
                } else {
                    let u = tre_ast_new_union(ctx.mem, node, *items.offset(j));
                    if u.is_null() {
                        status = REG_ESPACE;
                    }
                    node = u;
                }
            }
        }

        if status == REG_OK && negate != 0 {
            let n = tre_ast_new_literal(ctx.mem, curr_min, TRE_CHAR_MAX as c_int, ctx.position);
            if n.is_null() {
                status = REG_ESPACE;
            } else {
                let l = (*n).obj as *mut tre_literal_t;
                if num_neg_classes > 0 {
                    (*l).neg_classes = mem::tre_mem_alloc(
                        ctx.mem,
                        std::mem::size_of::<tre_ctype_t>() * (num_neg_classes as usize + 1),
                    ) as *mut tre_ctype_t;
                    if (*l).neg_classes.is_null() {
                        status = REG_ESPACE;
                    } else {
                        for k in 0..num_neg_classes as isize {
                            *(*l).neg_classes.offset(k) = neg_classes[k as usize];
                        }
                        *(*l).neg_classes.offset(num_neg_classes as isize) = 0;
                    }
                }
                if status == REG_OK {
                    if node.is_null() {
                        node = n;
                    } else {
                        let u = tre_ast_new_union(ctx.mem, node, n);
                        if u.is_null() {
                            status = REG_ESPACE;
                        } else {
                            node = u;
                        }
                    }
                }
            }
        }

        mem::xfree(items as *mut c_void);
        ctx.position += 1;
        *result = node;
        status
    }
}

unsafe fn tre_parse_int(regex: &mut *const tre_char_t, regex_end: *const tre_char_t) -> c_int {
    unsafe {
        let mut num: c_int = -1;
        let mut overflow = 0;
        let mut r = *regex;
        while r < regex_end && *r >= '0' as tre_char_t && *r <= '9' as tre_char_t {
            if num < 0 {
                num = 0;
            }
            if num <= (c_int::MAX - 9) / 10 {
                num = num * 10 + (*r - '0' as tre_char_t) as c_int;
            } else {
                overflow = 1;
            }
            r = r.offset(1);
        }
        *regex = r;
        if overflow != 0 { -1 } else { num }
    }
}

unsafe fn tre_parse_bound(ctx: &mut tre_parse_ctx_t, result: &mut *mut tre_ast_node_t) -> c_int {
    unsafe {
        let mut min: c_int;
        let mut max: c_int;
        let _i: c_int;
        let mut cost_ins: c_int = TRE_PARAM_UNSET;
        let mut cost_del: c_int = TRE_PARAM_UNSET;
        let mut cost_subst: c_int = TRE_PARAM_UNSET;
        let mut cost_max: c_int = TRE_PARAM_UNSET;
        let mut limit_ins: c_int = TRE_PARAM_UNSET;
        let mut limit_del: c_int = TRE_PARAM_UNSET;
        let mut limit_subst: c_int = TRE_PARAM_UNSET;
        let mut limit_err: c_int = TRE_PARAM_UNSET;
        let r_start = ctx.re;
        let mut minimal: c_int = if ctx.cflags & REG_UNGREEDY != 0 { 1 } else { 0 };
        let mut approx: c_int = 0;
        let mut costs_set: c_int = 0;
        let mut counts_set: c_int = 0;

        min = -1;
        if ctx.re < ctx.re_end && *ctx.re >= '0' as tre_char_t && *ctx.re <= '9' as tre_char_t {
            min = tre_parse_int(&mut ctx.re, ctx.re_end);
        }

        max = min;
        if ctx.re < ctx.re_end && *ctx.re == CHAR_COMMA {
            ctx.re = ctx.re.offset(1);
            max = tre_parse_int(&mut ctx.re, ctx.re_end);
        }

        if (max >= 0 && min > max) || max > RE_DUP_MAX {
            return REG_BADBR;
        }

        loop {
            let start = ctx.re;
            let mut done = false;

            if counts_set == 0 {
                while ctx.re.offset(1) < ctx.re_end && !done {
                    let c = *ctx.re;
                    if c == CHAR_PLUS {
                        ctx.re = ctx.re.offset(1);
                        limit_ins = tre_parse_int(&mut ctx.re, ctx.re_end);
                        if limit_ins < 0 {
                            limit_ins = c_int::MAX;
                        }
                        counts_set = 1;
                    } else if c == CHAR_MINUS {
                        ctx.re = ctx.re.offset(1);
                        limit_del = tre_parse_int(&mut ctx.re, ctx.re_end);
                        if limit_del < 0 {
                            limit_del = c_int::MAX;
                        }
                        counts_set = 1;
                    } else if c == CHAR_HASH {
                        ctx.re = ctx.re.offset(1);
                        limit_subst = tre_parse_int(&mut ctx.re, ctx.re_end);
                        if limit_subst < 0 {
                            limit_subst = c_int::MAX;
                        }
                        counts_set = 1;
                    } else if c == CHAR_TILDE {
                        ctx.re = ctx.re.offset(1);
                        limit_err = tre_parse_int(&mut ctx.re, ctx.re_end);
                        if limit_err < 0 {
                            limit_err = c_int::MAX;
                        }
                        approx = 1;
                    } else if c == CHAR_COMMA {
                        ctx.re = ctx.re.offset(1);
                    } else if c == CHAR_SPACE {
                        ctx.re = ctx.re.offset(1);
                    } else if c == CHAR_RCURLY {
                        done = true;
                    } else {
                        done = true;
                    }
                }
            }

            done = false;
            if costs_set == 0 {
                while ctx.re.offset(1) < ctx.re_end && !done {
                    let c = *ctx.re;
                    if c == CHAR_PLUS || c == CHAR_SPACE {
                        ctx.re = ctx.re.offset(1);
                    } else if c == '<' as tre_char_t {
                        ctx.re = ctx.re.offset(1);
                        while ctx.re < ctx.re_end && *ctx.re == CHAR_SPACE {
                            ctx.re = ctx.re.offset(1);
                        }
                        cost_max = tre_parse_int(&mut ctx.re, ctx.re_end);
                        if cost_max < 0 {
                            cost_max = c_int::MAX;
                        } else {
                            cost_max -= 1;
                        }
                        approx = 1;
                    } else if c == CHAR_COMMA {
                        ctx.re = ctx.re.offset(1);
                        done = true;
                    } else if c >= '0' as tre_char_t && c <= '9' as tre_char_t {
                        let cost = tre_parse_int(&mut ctx.re, ctx.re_end);
                        let next_c = *ctx.re;
                        if next_c == 'i' as tre_char_t {
                            ctx.re = ctx.re.offset(1);
                            cost_ins = cost;
                            costs_set = 1;
                        } else if next_c == 'd' as tre_char_t {
                            ctx.re = ctx.re.offset(1);
                            cost_del = cost;
                            costs_set = 1;
                        } else if next_c == 's' as tre_char_t {
                            ctx.re = ctx.re.offset(1);
                            cost_subst = cost;
                            costs_set = 1;
                        } else {
                            return REG_BADBR;
                        }
                    } else {
                        done = true;
                    }
                }
            }

            if ctx.re == start {
                break;
            }
        }

        if ctx.re >= ctx.re_end {
            return REG_EBRACE;
        }
        if ctx.re == r_start {
            return REG_BADBR;
        }

        if ctx.cflags & REG_EXTENDED != 0 {
            if ctx.re >= ctx.re_end || *ctx.re != CHAR_RBRACE {
                return REG_BADBR;
            }
            ctx.re = ctx.re.offset(1);
        } else {
            if ctx.re.offset(1) >= ctx.re_end
                || *ctx.re != CHAR_BACKSLASH
                || *ctx.re.offset(1) != CHAR_RBRACE
            {
                return REG_BADBR;
            }
            ctx.re = ctx.re.offset(2);
        }

        if ctx.re < ctx.re_end {
            if *ctx.re == CHAR_QUESTIONMARK {
                minimal = if ctx.cflags & REG_UNGREEDY != 0 { 0 } else { 1 };
                ctx.re = ctx.re.offset(1);
            } else if *ctx.re == CHAR_STAR || *ctx.re == CHAR_PLUS {
                return REG_BADRPT;
            }
        }

        if min == 0 && max == 0 {
            *result = tre_ast_new_literal(ctx.mem, EMPTY as c_int, -1, -1);
            if result.is_null() {
                return REG_ESPACE;
            }
        } else {
            if min < 0 && max < 0 {
                min = 1;
                max = 1;
            }

            *result = tre_ast_new_iter(ctx.mem, *result, min, max, minimal);
            if result.is_null() {
                return REG_ESPACE;
            }

            if approx != 0 || costs_set != 0 || counts_set != 0 {
                if costs_set != 0 || counts_set != 0 {
                    if limit_ins == TRE_PARAM_UNSET {
                        limit_ins = if cost_ins == TRE_PARAM_UNSET {
                            0
                        } else {
                            c_int::MAX
                        };
                    }
                    if limit_del == TRE_PARAM_UNSET {
                        limit_del = if cost_del == TRE_PARAM_UNSET {
                            0
                        } else {
                            c_int::MAX
                        };
                    }
                    if limit_subst == TRE_PARAM_UNSET {
                        limit_subst = if cost_subst == TRE_PARAM_UNSET {
                            0
                        } else {
                            c_int::MAX
                        };
                    }
                }

                if cost_max == TRE_PARAM_UNSET {
                    cost_max = c_int::MAX;
                }
                if limit_err == TRE_PARAM_UNSET {
                    limit_err = c_int::MAX;
                }

                ctx.have_approx = 1;
                let params =
                    mem::tre_mem_alloc(ctx.mem, std::mem::size_of::<c_int>() * TRE_PARAM_LAST)
                        as *mut c_int;
                if params.is_null() {
                    return REG_ESPACE;
                }
                for i in 0..TRE_PARAM_LAST {
                    *params.offset(i as isize) = TRE_PARAM_UNSET;
                }
                *params.offset(TRE_PARAM_COST_INS as isize) = cost_ins;
                *params.offset(TRE_PARAM_COST_DEL as isize) = cost_del;
                *params.offset(TRE_PARAM_COST_SUBST as isize) = cost_subst;
                *params.offset(TRE_PARAM_COST_MAX as isize) = cost_max;
                *params.offset(TRE_PARAM_MAX_INS as isize) = limit_ins;
                *params.offset(TRE_PARAM_MAX_DEL as isize) = limit_del;
                *params.offset(TRE_PARAM_MAX_SUBST as isize) = limit_subst;
                *params.offset(TRE_PARAM_MAX_ERR as isize) = limit_err;
                let iter = (**result).obj as *mut tre_iteration_t;
                (*iter).params = params;
            }
        }

        ctx.re = r_start;
        REG_OK
    }
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum tre_parse_re_stack_symbol_t {
    PARSE_RE = 0,
    PARSE_ATOM,
    PARSE_MARK_FOR_SUBMATCH,
    PARSE_BRANCH,
    PARSE_PIECE,
    PARSE_CATENATION,
    PARSE_POST_CATENATION,
    PARSE_UNION,
    PARSE_POST_UNION,
    PARSE_POSTFIX,
    PARSE_RESTORE_CFLAGS,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tre_parse(ctx: *mut tre_parse_ctx_t) -> c_int {
    unsafe {
        let ctx = &mut *ctx;
        let mut result: *mut tre_ast_node_t = ptr::null_mut();
        let mut status: c_int = REG_OK;
        let stack = ctx.stack;
        let bottom = stack::tre_stack_num_objects(stack);
        let mut depth: c_int = 0;
        let mut temporary_cflags: c_int = 0;

        if ctx.nofirstsub == 0 {
            stack::tre_stack_push_int(stack, ctx.submatch_id);
            stack::tre_stack_push_int(
                stack,
                tre_parse_re_stack_symbol_t::PARSE_MARK_FOR_SUBMATCH as c_int,
            );
            ctx.submatch_id += 1;
        }
        stack::tre_stack_push_int(stack, tre_parse_re_stack_symbol_t::PARSE_RE as c_int);
        ctx.re_start = ctx.re;
        ctx.re_end = ctx.re.offset(ctx.len as isize);

        while stack::tre_stack_num_objects(stack) > bottom && status == REG_OK {
            let symbol = stack::tre_stack_pop_int(stack);
            let sym_val = symbol;

            if sym_val == tre_parse_re_stack_symbol_t::PARSE_RE as c_int {
                if !(ctx.cflags & REG_LITERAL) != 0 && ctx.cflags & REG_EXTENDED != 0 {
                    stack::tre_stack_push_int(
                        stack,
                        tre_parse_re_stack_symbol_t::PARSE_UNION as c_int,
                    );
                }
                stack::tre_stack_push_int(
                    stack,
                    tre_parse_re_stack_symbol_t::PARSE_BRANCH as c_int,
                );
            } else if sym_val == tre_parse_re_stack_symbol_t::PARSE_BRANCH as c_int {
                stack::tre_stack_push_int(
                    stack,
                    tre_parse_re_stack_symbol_t::PARSE_CATENATION as c_int,
                );
                stack::tre_stack_push_int(stack, tre_parse_re_stack_symbol_t::PARSE_PIECE as c_int);
            } else if sym_val == tre_parse_re_stack_symbol_t::PARSE_PIECE as c_int {
                if ctx.cflags & REG_LITERAL == 0 {
                    stack::tre_stack_push_int(
                        stack,
                        tre_parse_re_stack_symbol_t::PARSE_POSTFIX as c_int,
                    );
                }
                stack::tre_stack_push_int(stack, tre_parse_re_stack_symbol_t::PARSE_ATOM as c_int);
            } else if sym_val == tre_parse_re_stack_symbol_t::PARSE_CATENATION as c_int {
                if ctx.re >= ctx.re_end {
                    continue;
                }
                let c = *ctx.re;
                if ctx.cflags & REG_LITERAL == 0 {
                    if ctx.cflags & REG_EXTENDED != 0 && c == CHAR_PIPE {
                        continue;
                    }
                    if (ctx.cflags & REG_EXTENDED != 0 && c == CHAR_RPAREN && depth > 0)
                        || (ctx.cflags & REG_EXTENDED == 0
                            && c == CHAR_BACKSLASH
                            && ctx.re.offset(1) < ctx.re_end
                            && *ctx.re.offset(1) == CHAR_RPAREN)
                    {
                        if ctx.cflags & REG_EXTENDED == 0 && depth == 0 {
                            status = REG_EPAREN;
                        }
                        depth -= 1;
                        if ctx.cflags & REG_EXTENDED == 0 {
                            ctx.re = ctx.re.offset(2);
                        }
                        continue;
                    }
                }

                stack::tre_stack_push_int(
                    stack,
                    tre_parse_re_stack_symbol_t::PARSE_CATENATION as c_int,
                );
                stack::tre_stack_push_voidptr(stack, result as *mut c_void);
                stack::tre_stack_push_int(
                    stack,
                    tre_parse_re_stack_symbol_t::PARSE_POST_CATENATION as c_int,
                );
                stack::tre_stack_push_int(stack, tre_parse_re_stack_symbol_t::PARSE_PIECE as c_int);
            } else if sym_val == tre_parse_re_stack_symbol_t::PARSE_POST_CATENATION as c_int {
                let tree = stack::tre_stack_pop_voidptr(stack) as *mut tre_ast_node_t;
                let tmp_node = tre_ast_new_catenation(ctx.mem, tree, result);
                if tmp_node.is_null() {
                    return REG_ESPACE;
                }
                result = tmp_node;
            } else if sym_val == tre_parse_re_stack_symbol_t::PARSE_UNION as c_int {
                if ctx.re >= ctx.re_end {
                    continue;
                }
                if ctx.cflags & REG_LITERAL != 0 {
                    continue;
                }
                let c = *ctx.re;
                if c == CHAR_PIPE {
                    stack::tre_stack_push_int(
                        stack,
                        tre_parse_re_stack_symbol_t::PARSE_UNION as c_int,
                    );
                    stack::tre_stack_push_voidptr(stack, result as *mut c_void);
                    stack::tre_stack_push_int(
                        stack,
                        tre_parse_re_stack_symbol_t::PARSE_POST_UNION as c_int,
                    );
                    stack::tre_stack_push_int(
                        stack,
                        tre_parse_re_stack_symbol_t::PARSE_BRANCH as c_int,
                    );
                    ctx.re = ctx.re.offset(1);
                } else if c == CHAR_RPAREN {
                    ctx.re = ctx.re.offset(1);
                }
            } else if sym_val == tre_parse_re_stack_symbol_t::PARSE_POST_UNION as c_int {
                let tree = stack::tre_stack_pop_voidptr(stack) as *mut tre_ast_node_t;
                let tmp_node = tre_ast_new_union(ctx.mem, tree, result);
                if tmp_node.is_null() {
                    return REG_ESPACE;
                }
                result = tmp_node;
            } else if sym_val == tre_parse_re_stack_symbol_t::PARSE_POSTFIX as c_int {
                if ctx.re >= ctx.re_end {
                    continue;
                }
                if ctx.cflags & REG_LITERAL != 0 {
                    continue;
                }
                let c = *ctx.re;
                if c == CHAR_PLUS || c == CHAR_QUESTIONMARK {
                    if ctx.cflags & REG_EXTENDED == 0 {
                        continue;
                    }
                    let mut minimal = if ctx.cflags & REG_UNGREEDY != 0 { 1 } else { 0 };
                    let mut rep_min: c_int = 0;
                    let mut rep_max: c_int = -1;

                    if c == CHAR_PLUS {
                        rep_min = 1;
                    }
                    if c == CHAR_QUESTIONMARK {
                        rep_max = 1;
                    }

                    if ctx.re.offset(1) < ctx.re_end {
                        if *ctx.re.offset(1) == CHAR_QUESTIONMARK {
                            minimal = if ctx.cflags & REG_UNGREEDY != 0 { 0 } else { 1 };
                            ctx.re = ctx.re.offset(1);
                        } else if *ctx.re.offset(1) == CHAR_STAR || *ctx.re.offset(1) == CHAR_PLUS {
                            return REG_BADRPT;
                        }
                    }

                    ctx.re = ctx.re.offset(1);
                    let tmp_node = tre_ast_new_iter(ctx.mem, result, rep_min, rep_max, minimal);
                    if tmp_node.is_null() {
                        return REG_ESPACE;
                    }
                    result = tmp_node;
                    stack::tre_stack_push_int(
                        stack,
                        tre_parse_re_stack_symbol_t::PARSE_POSTFIX as c_int,
                    );
                } else if c == CHAR_STAR {
                    let mut minimal = if ctx.cflags & REG_UNGREEDY != 0 { 1 } else { 0 };
                    let rep_min: c_int = 0;
                    let rep_max: c_int = -1;

                    if ctx.re.offset(1) < ctx.re_end {
                        if *ctx.re.offset(1) == CHAR_QUESTIONMARK {
                            minimal = if ctx.cflags & REG_UNGREEDY != 0 { 0 } else { 1 };
                            ctx.re = ctx.re.offset(1);
                        } else if *ctx.re.offset(1) == CHAR_STAR || *ctx.re.offset(1) == CHAR_PLUS {
                            return REG_BADRPT;
                        }
                    }

                    ctx.re = ctx.re.offset(1);
                    let tmp_node = tre_ast_new_iter(ctx.mem, result, rep_min, rep_max, minimal);
                    if tmp_node.is_null() {
                        return REG_ESPACE;
                    }
                    result = tmp_node;
                    stack::tre_stack_push_int(
                        stack,
                        tre_parse_re_stack_symbol_t::PARSE_POSTFIX as c_int,
                    );
                } else if c == CHAR_BACKSLASH {
                    if ctx.cflags & REG_EXTENDED == 0
                        && ctx.re.offset(1) < ctx.re_end
                        && *ctx.re.offset(1) == CHAR_LBRACE
                    {
                        ctx.re = ctx.re.offset(1);
                        ctx.re = ctx.re.offset(1);
                        status = tre_parse_bound(ctx, &mut result);
                        if status != REG_OK {
                            return status;
                        }
                        stack::tre_stack_push_int(
                            stack,
                            tre_parse_re_stack_symbol_t::PARSE_POSTFIX as c_int,
                        );
                    }
                } else if c == CHAR_LBRACE {
                    if ctx.cflags & REG_EXTENDED == 0 {
                        continue;
                    }
                    ctx.re = ctx.re.offset(1);
                    status = tre_parse_bound(ctx, &mut result);
                    if status != REG_OK {
                        return status;
                    }
                    stack::tre_stack_push_int(
                        stack,
                        tre_parse_re_stack_symbol_t::PARSE_POSTFIX as c_int,
                    );
                }
            } else if sym_val == tre_parse_re_stack_symbol_t::PARSE_ATOM as c_int {
                if ctx.re >= ctx.re_end {
                    // parse_literal fallthrough
                    result = parse_literal(ctx, &mut temporary_cflags, &mut result, &mut status);
                    if status != REG_OK {
                        return status;
                    }
                    continue;
                }

                if ctx.cflags & REG_LITERAL != 0 {
                    result = parse_literal(ctx, &mut temporary_cflags, &mut result, &mut status);
                    if status != REG_OK {
                        return status;
                    }
                    continue;
                }

                let c = *ctx.re;
                if c == CHAR_LPAREN {
                    if ctx.cflags & REG_EXTENDED != 0
                        && ctx.re.offset(1) < ctx.re_end
                        && *ctx.re.offset(1) == CHAR_QUESTIONMARK
                    {
                        let mut new_cflags = ctx.cflags;
                        let mut bit: c_int = 1;
                        ctx.re = ctx.re.offset(2);
                        loop {
                            let ac = *ctx.re;
                            if ac == 'i' as tre_char_t {
                                if bit != 0 {
                                    new_cflags |= REG_ICASE;
                                } else {
                                    new_cflags &= !REG_ICASE;
                                }
                                ctx.re = ctx.re.offset(1);
                            } else if ac == 'n' as tre_char_t {
                                if bit != 0 {
                                    new_cflags |= REG_NEWLINE;
                                } else {
                                    new_cflags &= !REG_NEWLINE;
                                }
                                ctx.re = ctx.re.offset(1);
                            } else if ac == 'U' as tre_char_t {
                                if bit != 0 {
                                    new_cflags |= REG_UNGREEDY;
                                } else {
                                    new_cflags &= !REG_UNGREEDY;
                                }
                                ctx.re = ctx.re.offset(1);
                            } else if ac == CHAR_MINUS {
                                ctx.re = ctx.re.offset(1);
                                bit = 0;
                            } else if ac == CHAR_COLON {
                                ctx.re = ctx.re.offset(1);
                                depth += 1;
                                break;
                            } else if ac == CHAR_HASH {
                                while ctx.re < ctx.re_end && *ctx.re != CHAR_RPAREN {
                                    ctx.re = ctx.re.offset(1);
                                }
                                if ctx.re < ctx.re_end && *ctx.re == CHAR_RPAREN {
                                    ctx.re = ctx.re.offset(1);
                                    break;
                                } else {
                                    return REG_BADPAT;
                                }
                            } else if ac == CHAR_RPAREN {
                                ctx.re = ctx.re.offset(1);
                                break;
                            } else {
                                return REG_BADPAT;
                            }
                        }
                        stack::tre_stack_push_int(stack, ctx.cflags);
                        stack::tre_stack_push_int(
                            stack,
                            tre_parse_re_stack_symbol_t::PARSE_RESTORE_CFLAGS as c_int,
                        );
                        stack::tre_stack_push_int(
                            stack,
                            tre_parse_re_stack_symbol_t::PARSE_RE as c_int,
                        );
                        ctx.cflags = new_cflags;
                    } else if ctx.cflags & REG_EXTENDED != 0
                        || (ctx.re > ctx.re_start && *ctx.re.offset(-1) == CHAR_BACKSLASH)
                    {
                        depth += 1;
                        if ctx.re.offset(2) < ctx.re_end
                            && *ctx.re.offset(1) == CHAR_QUESTIONMARK
                            && *ctx.re.offset(2) == CHAR_COLON
                        {
                            ctx.re = ctx.re.offset(3);
                            stack::tre_stack_push_int(
                                stack,
                                tre_parse_re_stack_symbol_t::PARSE_RE as c_int,
                            );
                        } else {
                            stack::tre_stack_push_int(stack, ctx.submatch_id);
                            stack::tre_stack_push_int(
                                stack,
                                tre_parse_re_stack_symbol_t::PARSE_MARK_FOR_SUBMATCH as c_int,
                            );
                            stack::tre_stack_push_int(
                                stack,
                                tre_parse_re_stack_symbol_t::PARSE_RE as c_int,
                            );
                            ctx.submatch_id += 1;
                            ctx.re = ctx.re.offset(1);
                        }
                    } else {
                        result =
                            parse_literal(ctx, &mut temporary_cflags, &mut result, &mut status);
                        if status != REG_OK {
                            return status;
                        }
                    }
                } else if c == CHAR_RPAREN {
                    if (ctx.cflags & REG_EXTENDED != 0 && depth > 0)
                        || (ctx.cflags & REG_EXTENDED == 0
                            && ctx.re > ctx.re_start
                            && *ctx.re.offset(-1) == CHAR_BACKSLASH)
                    {
                        result = tre_ast_new_literal(ctx.mem, EMPTY as c_int, -1, -1);
                        if result.is_null() {
                            return REG_ESPACE;
                        }
                        if ctx.cflags & REG_EXTENDED == 0 {
                            ctx.re = ctx.re.offset(-1);
                        }
                    } else {
                        result =
                            parse_literal(ctx, &mut temporary_cflags, &mut result, &mut status);
                        if status != REG_OK {
                            return status;
                        }
                    }
                } else if c == CHAR_LBRACKET {
                    ctx.re = ctx.re.offset(1);
                    status = tre_parse_bracket(ctx, &mut result);
                    if status != REG_OK {
                        return status;
                    }
                } else if c == CHAR_BACKSLASH {
                    if ctx.cflags & REG_EXTENDED == 0
                        && ctx.re.offset(1) < ctx.re_end
                        && (*ctx.re.offset(1) == CHAR_LPAREN || *ctx.re.offset(1) == CHAR_RPAREN)
                    {
                        ctx.re = ctx.re.offset(1);
                        stack::tre_stack_push_int(
                            stack,
                            tre_parse_re_stack_symbol_t::PARSE_ATOM as c_int,
                        );
                    } else {
                        // Check for macro expansion
                        let mut buf = [0u32; 64];
                        if ctx.re.offset(1) < ctx.re_end {
                            let remaining_len = ctx.re_end.offset_from(ctx.re.offset(1)) as usize;
                            let remaining =
                                std::slice::from_raw_parts(ctx.re.offset(1), remaining_len);
                            tre_expand_macro(remaining, remaining.len(), &mut buf);
                            if buf[0] != 0 {
                                let mut subctx: tre_parse_ctx_t = std::mem::zeroed();
                                subctx.mem = ctx.mem;
                                subctx.stack = ctx.stack;
                                subctx.re = buf.as_ptr();
                                let mut blen = 0;
                                while blen < 64 && buf[blen] != 0 {
                                    blen += 1;
                                }
                                subctx.len = blen;
                                subctx.nofirstsub = 1;
                                subctx.cflags = ctx.cflags;
                                subctx.max_backref = ctx.max_backref;
                                subctx.cur_max = ctx.cur_max;
                                subctx.position = ctx.position;
                                subctx.submatch_id = ctx.submatch_id;
                                subctx.have_approx = ctx.have_approx;
                                status = tre_parse(&mut subctx);
                                if status != REG_OK {
                                    return status;
                                }
                                ctx.re = ctx.re.offset(2);
                                ctx.position = subctx.position;
                                result = subctx.result;
                                continue;
                            }
                        }

                        if ctx.re.offset(1) >= ctx.re_end {
                            return REG_EESCAPE;
                        }

                        // \Q literal mode
                        if *ctx.re.offset(1) == 'Q' as tre_char_t {
                            ctx.cflags |= REG_LITERAL;
                            temporary_cflags |= REG_LITERAL;
                            ctx.re = ctx.re.offset(2);
                            stack::tre_stack_push_int(
                                stack,
                                tre_parse_re_stack_symbol_t::PARSE_ATOM as c_int,
                            );
                            continue;
                        }

                        ctx.re = ctx.re.offset(1);
                        let ac = *ctx.re;
                        if ac == 'b' as tre_char_t {
                            result = tre_ast_new_literal(
                                ctx.mem,
                                ASSERTION as c_int,
                                ASSERT_AT_WB as c_int,
                                -1,
                            );
                            ctx.re = ctx.re.offset(1);
                        } else if ac == 'B' as tre_char_t {
                            result = tre_ast_new_literal(
                                ctx.mem,
                                ASSERTION as c_int,
                                ASSERT_AT_WB_NEG as c_int,
                                -1,
                            );
                            ctx.re = ctx.re.offset(1);
                        } else if ac == '<' as tre_char_t {
                            result = tre_ast_new_literal(
                                ctx.mem,
                                ASSERTION as c_int,
                                ASSERT_AT_BOW as c_int,
                                -1,
                            );
                            ctx.re = ctx.re.offset(1);
                        } else if ac == '>' as tre_char_t {
                            result = tre_ast_new_literal(
                                ctx.mem,
                                ASSERTION as c_int,
                                ASSERT_AT_EOW as c_int,
                                -1,
                            );
                            ctx.re = ctx.re.offset(1);
                        } else if ac == 'x' as tre_char_t {
                            ctx.re = ctx.re.offset(1);
                            if ctx.re < ctx.re_end && *ctx.re != CHAR_LBRACE {
                                let mut tmp = [0u8; 3];
                                let mut idx = 0;
                                if ctx.re < ctx.re_end && tre_isxdigit(*ctx.re) {
                                    tmp[idx] = *ctx.re as u8;
                                    idx += 1;
                                    ctx.re = ctx.re.offset(1);
                                }
                                if ctx.re < ctx.re_end && tre_isxdigit(*ctx.re) {
                                    tmp[idx] = *ctx.re as u8;
                                    idx += 1;
                                    ctx.re = ctx.re.offset(1);
                                }
                                let val = i32::from_str_radix(
                                    std::str::from_utf8(&tmp[..idx]).unwrap_or("0"),
                                    16,
                                )
                                .unwrap_or(0);
                                result = tre_ast_new_literal(ctx.mem, val, val, ctx.position);
                                ctx.position += 1;
                            } else if ctx.re < ctx.re_end {
                                let mut tmp = [0u8; 32];
                                let mut idx = 0;
                                ctx.re = ctx.re.offset(1);
                                while ctx.re < ctx.re_end {
                                    if *ctx.re == CHAR_RBRACE {
                                        break;
                                    }
                                    if tre_isxdigit(*ctx.re) {
                                        tmp[idx] = *ctx.re as u8;
                                        idx += 1;
                                        ctx.re = ctx.re.offset(1);
                                    } else {
                                        return REG_EBRACE;
                                    }
                                }
                                ctx.re = ctx.re.offset(1);
                                let val = i32::from_str_radix(
                                    std::str::from_utf8(&tmp[..idx]).unwrap_or("0"),
                                    16,
                                )
                                .unwrap_or(0);
                                result = tre_ast_new_literal(ctx.mem, val, val, ctx.position);
                                ctx.position += 1;
                            }
                        } else if tre_isdigit(*ctx.re) {
                            let val = (*ctx.re - '0' as tre_char_t) as c_int;
                            result =
                                tre_ast_new_literal(ctx.mem, BACKREF as c_int, val, ctx.position);
                            if result.is_null() {
                                return REG_ESPACE;
                            }
                            ctx.position += 1;
                            ctx.max_backref = MAX(val, ctx.max_backref);
                            ctx.re = ctx.re.offset(1);
                        } else {
                            result = tre_ast_new_literal(
                                ctx.mem,
                                *ctx.re as c_int,
                                *ctx.re as c_int,
                                ctx.position,
                            );
                            if result.is_null() {
                                return REG_ESPACE;
                            }
                            ctx.position += 1;
                            ctx.re = ctx.re.offset(1);
                        }
                        if result.is_null() {
                            return REG_ESPACE;
                        }
                    }
                } else if c == CHAR_PERIOD {
                    if ctx.cflags & REG_NEWLINE != 0 {
                        let tmp1 = tre_ast_new_literal(
                            ctx.mem,
                            0,
                            ('\n' as tre_char_t - 1) as c_int,
                            ctx.position,
                        );
                        if tmp1.is_null() {
                            return REG_ESPACE;
                        }
                        let tmp2 = tre_ast_new_literal(
                            ctx.mem,
                            ('\n' as tre_char_t + 1) as c_int,
                            TRE_CHAR_MAX as c_int,
                            ctx.position + 1,
                        );
                        if tmp2.is_null() {
                            return REG_ESPACE;
                        }
                        result = tre_ast_new_union(ctx.mem, tmp1, tmp2);
                        if result.is_null() {
                            return REG_ESPACE;
                        }
                        ctx.position += 2;
                    } else {
                        result =
                            tre_ast_new_literal(ctx.mem, 0, TRE_CHAR_MAX as c_int, ctx.position);
                        if result.is_null() {
                            return REG_ESPACE;
                        }
                        ctx.position += 1;
                    }
                    ctx.re = ctx.re.offset(1);
                } else if c == CHAR_CARET {
                    if ctx.cflags & REG_EXTENDED != 0
                        || (ctx.re.offset(-2) >= ctx.re_start
                            && *ctx.re.offset(-2) == CHAR_BACKSLASH
                            && *ctx.re.offset(-1) == CHAR_LPAREN)
                        || ctx.re == ctx.re_start
                    {
                        result = tre_ast_new_literal(
                            ctx.mem,
                            ASSERTION as c_int,
                            ASSERT_AT_BOL as c_int,
                            -1,
                        );
                        if result.is_null() {
                            return REG_ESPACE;
                        }
                        ctx.re = ctx.re.offset(1);
                    } else {
                        result =
                            parse_literal(ctx, &mut temporary_cflags, &mut result, &mut status);
                        if status != REG_OK {
                            return status;
                        }
                    }
                } else if c == CHAR_DOLLAR {
                    if ctx.cflags & REG_EXTENDED != 0
                        || (ctx.re.offset(2) < ctx.re_end
                            && *ctx.re.offset(1) == CHAR_BACKSLASH
                            && *ctx.re.offset(2) == CHAR_RPAREN)
                        || ctx.re.offset(1) == ctx.re_end
                    {
                        result = tre_ast_new_literal(
                            ctx.mem,
                            ASSERTION as c_int,
                            ASSERT_AT_EOL as c_int,
                            -1,
                        );
                        if result.is_null() {
                            return REG_ESPACE;
                        }
                        ctx.re = ctx.re.offset(1);
                    } else {
                        result =
                            parse_literal(ctx, &mut temporary_cflags, &mut result, &mut status);
                        if status != REG_OK {
                            return status;
                        }
                    }
                } else {
                    result = parse_literal(ctx, &mut temporary_cflags, &mut result, &mut status);
                    if status != REG_OK {
                        return status;
                    }
                }
            } else if sym_val == tre_parse_re_stack_symbol_t::PARSE_MARK_FOR_SUBMATCH as c_int {
                let submatch_id = stack::tre_stack_pop_int(stack);
                if !result.is_null() {
                    if (*result).submatch_id >= 0 {
                        let n = tre_ast_new_literal(ctx.mem, EMPTY as c_int, -1, -1);
                        if n.is_null() {
                            return REG_ESPACE;
                        }
                        let tmp_node = tre_ast_new_catenation(ctx.mem, n, result);
                        if tmp_node.is_null() {
                            return REG_ESPACE;
                        }
                        (*tmp_node).num_submatches = (*result).num_submatches;
                        result = tmp_node;
                    }
                    (*result).submatch_id = submatch_id;
                    (*result).num_submatches += 1;
                }
            } else if sym_val == tre_parse_re_stack_symbol_t::PARSE_RESTORE_CFLAGS as c_int {
                ctx.cflags = stack::tre_stack_pop_int(stack);
            }
        }

        if depth > 0 {
            return REG_EPAREN;
        }

        if status == REG_OK {
            (*ctx).result = result;
        }

        status
    }
}

unsafe fn parse_literal(
    ctx: &mut tre_parse_ctx_t,
    temporary_cflags: &mut c_int,
    _result: &mut *mut tre_ast_node_t,
    status: &mut c_int,
) -> *mut tre_ast_node_t {
    unsafe {
        // Check for \E (end of literal mode)
        if *temporary_cflags != 0
            && ctx.re.offset(1) < ctx.re_end
            && *ctx.re == CHAR_BACKSLASH
            && *ctx.re.offset(1) == 'E' as tre_char_t
        {
            ctx.cflags &= !*temporary_cflags;
            *temporary_cflags = 0;
            ctx.re = ctx.re.offset(2);
            stack::tre_stack_push_int(ctx.stack, tre_parse_re_stack_symbol_t::PARSE_PIECE as c_int);
            return ptr::null_mut();
        }

        // Check for empty expression
        if ctx.cflags & REG_LITERAL == 0 {
            if ctx.re >= ctx.re_end
                || *ctx.re == CHAR_STAR
                || (ctx.cflags & REG_EXTENDED != 0
                    && (*ctx.re == CHAR_PIPE
                        || *ctx.re == CHAR_LBRACE
                        || *ctx.re == CHAR_PLUS
                        || *ctx.re == CHAR_QUESTIONMARK))
                || (ctx.cflags & REG_EXTENDED == 0
                    && ctx.re.offset(1) < ctx.re_end
                    && *ctx.re == CHAR_BACKSLASH
                    && *ctx.re.offset(1) == CHAR_LBRACE)
            {
                let node = tre_ast_new_literal(ctx.mem, EMPTY as c_int, -1, -1);
                if node.is_null() {
                    *status = REG_ESPACE;
                    return ptr::null_mut();
                }
                return node;
            }
        }

        // R change: literal empty
        if (ctx.cflags & REG_LITERAL != 0) && *ctx.re == 0 {
            let node = tre_ast_new_literal(ctx.mem, EMPTY as c_int, -1, -1);
            if node.is_null() {
                *status = REG_ESPACE;
                return ptr::null_mut();
            }
            return node;
        }

        // Normal literal
        if ctx.cflags & REG_ICASE != 0 && (tre_isupper(*ctx.re) || tre_islower(*ctx.re)) {
            let tmp1 = tre_ast_new_literal(
                ctx.mem,
                tre_toupper(*ctx.re) as c_int,
                tre_toupper(*ctx.re) as c_int,
                ctx.position,
            );
            if tmp1.is_null() {
                *status = REG_ESPACE;
                return ptr::null_mut();
            }
            let tmp2 = tre_ast_new_literal(
                ctx.mem,
                tre_tolower(*ctx.re) as c_int,
                tre_tolower(*ctx.re) as c_int,
                ctx.position,
            );
            if tmp2.is_null() {
                *status = REG_ESPACE;
                return ptr::null_mut();
            }
            let node = tre_ast_new_union(ctx.mem, tmp1, tmp2);
            if node.is_null() {
                *status = REG_ESPACE;
                return ptr::null_mut();
            }
            ctx.position += 1;
            ctx.re = ctx.re.offset(1);
            node
        } else {
            let node =
                tre_ast_new_literal(ctx.mem, *ctx.re as c_int, *ctx.re as c_int, ctx.position);
            if node.is_null() {
                *status = REG_ESPACE;
                return ptr::null_mut();
            }
            ctx.position += 1;
            ctx.re = ctx.re.offset(1);
            node
        }
    }
}
