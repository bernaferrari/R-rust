#![allow(unused_variables)]
#![allow(unused_variables)]
#![allow(unused_assignments)]
#![allow(unused_assignments)]
/*
  tre/match_parallel.rs - TRE parallel regex matching engine

  Ported from tre-match-parallel.c
*/
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use std::os::raw::{c_int, c_void};
use std::ptr;

use super::ast::*;

#[repr(C)]
struct tre_tnfa_reach_t {
    state: *mut tre_tnfa_transition_t,
    tags: *mut c_int,
}

#[repr(C)]
struct tre_reach_pos_t {
    pos: c_int,
    tags: *mut *mut c_int,
}

pub(crate) unsafe fn tre_isalnum(c: tre_cint_t) -> bool {
    super::parse::tre_isalnum(c)
}

#[inline]
fn IS_WORD_CHAR(c: tre_cint_t) -> bool {
    c == '_' as tre_cint_t || unsafe { tre_isalnum(c) }
}

#[inline]
fn CHECK_ASSERTIONS(
    assertions: c_int,
    pos: c_int,
    prev_c: tre_cint_t,
    next_c: tre_cint_t,
    reg_notbol: c_int,
    reg_noteol: c_int,
    reg_newline: c_int,
) -> bool {
    ((assertions & ASSERT_AT_BOL) != 0
        && (pos > 0 || reg_notbol != 0)
        && (prev_c != '\n' as tre_cint_t || reg_newline == 0))
        || ((assertions & ASSERT_AT_EOL) != 0
            && (next_c != 0 || reg_noteol != 0)
            && (next_c != '\n' as tre_cint_t || reg_newline == 0))
        || ((assertions & ASSERT_AT_BOW) != 0 && (IS_WORD_CHAR(prev_c) || !IS_WORD_CHAR(next_c)))
        || ((assertions & ASSERT_AT_EOW) != 0 && (!IS_WORD_CHAR(prev_c) || IS_WORD_CHAR(next_c)))
        || ((assertions & ASSERT_AT_WB) != 0
            && (pos != 0 && next_c != 0 && IS_WORD_CHAR(prev_c) == IS_WORD_CHAR(next_c)))
        || ((assertions & ASSERT_AT_WB_NEG) != 0
            && (pos == 0 || next_c == 0 || IS_WORD_CHAR(prev_c) != IS_WORD_CHAR(next_c)))
}

#[inline]
fn CHECK_CHAR_CLASSES(
    trans_i: *const tre_tnfa_transition_t,
    tnfa: *const tre_tnfa_t,
    eflags: c_int,
    prev_c: tre_cint_t,
) -> bool {
    unsafe {
        let assertions = (*trans_i).assertions;
        let icase = (*tnfa).cflags & REG_ICASE;
        ((assertions & ASSERT_CHAR_CLASS) != 0
            && icase == 0
            && !super::parse::tre_isctype(prev_c, (*trans_i).u.get_class()))
            || ((assertions & ASSERT_CHAR_CLASS) != 0
                && icase != 0
                && !super::parse::tre_isctype(
                    super::parse::tre_tolower(prev_c),
                    (*trans_i).u.get_class(),
                )
                && !super::parse::tre_isctype(
                    super::parse::tre_toupper(prev_c),
                    (*trans_i).u.get_class(),
                ))
            || ((assertions & ASSERT_CHAR_CLASS_NEG) != 0
                && super::parse::tre_neg_char_classes_match((*trans_i).neg_classes, prev_c, icase)
                    != 0)
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn tre_tnfa_run_parallel(
    tnfa: *const tre_tnfa_t,
    string: *const c_void,
    len: c_int,
    type_: tre_str_type_t,
    match_tags: *mut c_int,
    eflags: c_int,
    match_end_ofs: *mut c_int,
) -> c_int {
    unsafe {
        let str_byte = string as *const u8;
        let mut pos: c_int = -1;
        let mut pos_add_next: u32 = 1;
        let reg_notbol = eflags & REG_NOTBOL;
        let reg_noteol = eflags & REG_NOTEOL;
        let reg_newline = (*tnfa).cflags & REG_NEWLINE;

        let mut prev_c: tre_cint_t = 0;
        let mut next_c: tre_cint_t = 0;

        let num_tags = if match_tags.is_null() {
            0
        } else {
            (*tnfa).num_tags
        };

        // Allocate temporary buffers
        let tbytes = std::mem::size_of::<c_int>() * num_tags as usize;
        let rbytes = std::mem::size_of::<tre_tnfa_reach_t>() * ((*tnfa).num_states as usize + 1);
        let pbytes = std::mem::size_of::<tre_reach_pos_t>() * (*tnfa).num_states as usize;
        let xbytes = std::mem::size_of::<c_int>() * num_tags as usize;

        let total_bytes = rbytes * 2 + xbytes * (*tnfa).num_states as usize * 2 + tbytes + pbytes;
        let buf = mem::xmalloc(total_bytes);
        if buf.is_null() {
            return REG_ESPACE as c_int;
        }
        ptr::write_bytes(buf as *mut u8, 0, total_bytes);

        let mut buf_offset = buf as *mut u8;
        let mut tmp_tags = buf_offset as *mut c_int;
        buf_offset = buf_offset.add(tbytes);
        buf_offset = align_ptr(buf_offset);
        let reach_next = buf_offset as *mut tre_tnfa_reach_t;
        buf_offset = buf_offset.add(rbytes);
        buf_offset = align_ptr(buf_offset);
        let reach = buf_offset as *mut tre_tnfa_reach_t;
        buf_offset = buf_offset.add(rbytes);
        buf_offset = align_ptr(buf_offset);
        let reach_pos = buf_offset as *mut tre_reach_pos_t;
        buf_offset = buf_offset.add(pbytes);
        buf_offset = align_ptr(buf_offset);

        for i in 0..(*tnfa).num_states {
            *reach.offset(i as isize) = std::mem::zeroed();
            (*reach.offset(i as isize)).tags = buf_offset as *mut c_int;
            buf_offset = buf_offset.add(xbytes);
            *reach_next.offset(i as isize) = std::mem::zeroed();
            (*reach_next.offset(i as isize)).tags = buf_offset as *mut c_int;
            buf_offset = buf_offset.add(xbytes);
        }

        for i in 0..(*tnfa).num_states {
            (*reach_pos.offset(i as isize)).pos = -1;
        }

        // First character optimization
        if (*tnfa).first_char >= 0 && type_ == tre_str_type_t::STR_BYTE && !str_byte.is_null() {
            let first = (*tnfa).first_char as u8;
            let mut found = false;
            let limit = if len >= 0 { len as usize } else { usize::MAX };
            let mut j = 0;
            while j < limit {
                if *str_byte.add(j) == first {
                    found = true;
                    break;
                }
                j += 1;
            }
            if !found {
                mem::xfree(buf);
                return REG_NOMATCH as c_int;
            }
            if j >= 1 {
                prev_c = *str_byte.add(j - 1) as tre_cint_t;
            }
            next_c = *str_byte.add(j) as tre_cint_t;
            pos = j as c_int;
            if len < 0 || pos < len {
                // str_byte already advanced
            }
        } else {
            // GET_NEXT_WCHAR - simplified for STR_BYTE
            prev_c = next_c;
            pos = 0;
            if len >= 0 && pos >= len {
                next_c = 0;
            } else if !str_byte.is_null() {
                next_c = *str_byte as tre_cint_t;
            } else {
                next_c = 0;
            }
        }

        let mut match_eo: c_int = -1;
        let mut new_match: c_int = 0;
        let mut reach_next_i = reach_next;

        loop {
            // Add initial states to reach_next if no match found yet
            if match_eo < 0 {
                let mut trans_i = (*tnfa).initial;
                while !(*trans_i).state.is_null() {
                    if (*reach_pos.offset((*trans_i).state_id as isize)).pos < pos {
                        if (*trans_i).assertions != 0
                            && CHECK_ASSERTIONS(
                                (*trans_i).assertions,
                                pos,
                                prev_c,
                                next_c,
                                reg_notbol,
                                reg_noteol,
                                reg_newline,
                            )
                        {
                            trans_i = trans_i.add(1);
                            continue;
                        }

                        (*reach_next_i).state = (*trans_i).state;
                        for i in 0..num_tags {
                            *(*reach_next_i).tags.offset(i as isize) = -1;
                        }
                        let mut tag_i = (*trans_i).tags;
                        if !tag_i.is_null() {
                            while *tag_i >= 0 {
                                if *tag_i < num_tags {
                                    *(*reach_next_i).tags.offset(*tag_i as isize) = pos;
                                }
                                tag_i = tag_i.add(1);
                            }
                        }
                        if (*reach_next_i).state == (*tnfa).final_ {
                            match_eo = pos;
                            new_match = 1;
                            for i in 0..num_tags {
                                *match_tags.offset(i as isize) =
                                    *(*reach_next_i).tags.offset(i as isize);
                            }
                        }
                        (*reach_pos.offset((*trans_i).state_id as isize)).pos = pos;
                        (*reach_pos.offset((*trans_i).state_id as isize)).tags =
                            &mut (*reach_next_i).tags;
                        reach_next_i = reach_next_i.add(1);
                    }
                    trans_i = trans_i.add(1);
                }
                (*reach_next_i).state = ptr::null_mut();
            } else {
                if num_tags == 0 || reach_next_i == reach_next {
                    break;
                }
            }

            // Check for end of string
            if len < 0 {
                if next_c == 0 {
                    break;
                }
            } else {
                if pos >= len {
                    break;
                }
            }

            // GET_NEXT_WCHAR
            prev_c = next_c;
            pos += 1;
            if len >= 0 && pos >= len {
                next_c = 0;
            } else if type_ == tre_str_type_t::STR_BYTE && !str_byte.is_null() {
                next_c = *str_byte.add(pos as usize) as tre_cint_t;
            } else {
                next_c = 0;
            }

            // Swap reach and reach_next
            let tmp = reach;
            let reach = reach_next;
            let reach_next = tmp;

            // Weed out states that don't fulfill minimal matching conditions
            if (*tnfa).num_minimals != 0 && new_match != 0 {
                new_match = 0;
                reach_next_i = reach_next;
                let mut reach_i = reach;
                while !(*reach_i).state.is_null() {
                    let mut skip = false;
                    let mut mi: c_int = 0;
                    while *(*tnfa).minimal_tags.offset(mi as isize) >= 0 {
                        let end = *(*tnfa).minimal_tags.offset(mi as isize);
                        let start = *(*tnfa).minimal_tags.offset((mi + 1) as isize);
                        if end >= num_tags {
                            skip = true;
                            break;
                        } else if *(*reach_i).tags.offset(start as isize)
                            == *match_tags.offset(start as isize)
                            && *(*reach_i).tags.offset(end as isize)
                                < *match_tags.offset(end as isize)
                        {
                            skip = true;
                            break;
                        }
                        mi += 2;
                    }
                    if !skip {
                        (*reach_next_i).state = (*reach_i).state;
                        let tmp_iptr = (*reach_next_i).tags;
                        (*reach_next_i).tags = (*reach_i).tags;
                        (*reach_i).tags = tmp_iptr;
                        reach_next_i = reach_next_i.add(1);
                    }
                    reach_i = reach_i.add(1);
                }
                (*reach_next_i).state = ptr::null_mut();

                // Swap again
                let tmp2 = reach;
                let reach = reach_next;
                let reach_next = tmp2;
            }

            // For each state in reach, find transitions
            reach_next_i = reach_next;
            let mut reach_i = reach;
            while !(*reach_i).state.is_null() {
                let mut trans_i = (*reach_i).state;
                while !(*trans_i).state.is_null() {
                    if (*trans_i).code_min <= prev_c && (*trans_i).code_max >= prev_c {
                        if (*trans_i).assertions != 0
                            && (CHECK_ASSERTIONS(
                                (*trans_i).assertions,
                                pos,
                                prev_c,
                                next_c,
                                reg_notbol,
                                reg_noteol,
                                reg_newline,
                            ) || CHECK_CHAR_CLASSES(trans_i, tnfa, eflags, prev_c))
                        {
                            trans_i = trans_i.add(1);
                            continue;
                        }

                        // Compute tags
                        for i in 0..num_tags {
                            *tmp_tags.offset(i as isize) = *(*reach_i).tags.offset(i as isize);
                        }
                        let mut tag_i = (*trans_i).tags;
                        if !tag_i.is_null() {
                            while *tag_i >= 0 {
                                if *tag_i < num_tags {
                                    *tmp_tags.offset(*tag_i as isize) = pos;
                                }
                                tag_i = tag_i.add(1);
                            }
                        }

                        if (*reach_pos.offset((*trans_i).state_id as isize)).pos < pos {
                            (*reach_next_i).state = (*trans_i).state;
                            let tmp_iptr = (*reach_next_i).tags;
                            (*reach_next_i).tags = tmp_tags;
                            tmp_tags = tmp_iptr;
                            (*reach_pos.offset((*trans_i).state_id as isize)).pos = pos;
                            (*reach_pos.offset((*trans_i).state_id as isize)).tags =
                                &mut (*reach_next_i).tags;

                            if (*reach_next_i).state == (*tnfa).final_
                                && (match_eo == -1
                                    || (num_tags > 0 && *(*reach_next_i).tags <= *match_tags))
                            {
                                match_eo = pos;
                                new_match = 1;
                                for i in 0..num_tags {
                                    *match_tags.offset(i as isize) =
                                        *(*reach_next_i).tags.offset(i as isize);
                                }
                            }
                            reach_next_i = reach_next_i.add(1);
                        } else {
                            // Another path reached this state - choose winner
                            if tre_tag_order(
                                num_tags,
                                (*tnfa).tag_directions,
                                tmp_tags,
                                *(*reach_pos.offset((*trans_i).state_id as isize)).tags,
                            ) != 0
                            {
                                let tmp_iptr =
                                    *(*reach_pos.offset((*trans_i).state_id as isize)).tags;
                                *(*reach_pos.offset((*trans_i).state_id as isize)).tags = tmp_tags;
                                if (*trans_i).state == (*tnfa).final_ {
                                    match_eo = pos;
                                    new_match = 1;
                                    for i in 0..num_tags {
                                        *match_tags.offset(i as isize) =
                                            *tmp_tags.offset(i as isize);
                                    }
                                }
                                tmp_tags = tmp_iptr;
                            }
                        }
                    }
                    trans_i = trans_i.add(1);
                }
                reach_i = reach_i.add(1);
            }
            (*reach_next_i).state = ptr::null_mut();
        }

        mem::xfree(buf);
        *match_end_ofs = match_eo;
        if match_eo >= 0 {
            REG_OK as c_int
        } else {
            REG_NOMATCH as c_int
        }
    }
}

fn align_ptr(p: *mut u8) -> *mut u8 {
    let align = std::mem::size_of::<usize>();
    let addr = p as usize;
    let rem = addr % align;
    if rem == 0 {
        p
    } else {
        unsafe { p.add(align - rem) }
    }
}

use super::mem;
