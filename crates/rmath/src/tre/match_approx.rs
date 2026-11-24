#![allow(unused_variables)]
#![allow(unused_assignments)]
/*
  tre/match_approx.rs - TRE approximate regex matching engine

  Ported from tre-match-approx.c
*/
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use std::os::raw::{c_int, c_void};
use std::ptr::{self, NonNull};

use super::ast::*;
use super::mem;
use crate::mainutils::grep::{RegexWorkspace, WorkspaceBuffer};
use crate::sexp::memory::WorkspaceReservation;

const TRE_M_COST: usize = 0;
const TRE_M_NUM_INS: usize = 1;
const TRE_M_NUM_DEL: usize = 2;
const TRE_M_NUM_SUBST: usize = 3;
const TRE_M_NUM_ERR: usize = 4;
const TRE_M_LAST: usize = 5;
const TRE_M_MAX_DEPTH: usize = 3;

#[repr(C)]
struct tre_tnfa_approx_reach_t {
    state: *mut tre_tnfa_transition_t,
    pos: c_int,
    tags: *mut c_int,
    params: regaparams_t,
    depth: c_int,
    costs: [[c_int; TRE_M_LAST]; TRE_M_MAX_DEPTH + 1],
}

struct ApproxScratch {
    payload: NonNull<c_void>,
    _charge: WorkspaceReservation,
}

impl ApproxScratch {
    fn new(workspace: &RegexWorkspace, bytes: usize) -> Result<Self, ()> {
        let charge = workspace.try_reservation(mem::xallocation_size(bytes).ok_or(())?)?;
        let payload = NonNull::new(mem::xmalloc(bytes)).ok_or(())?;
        Ok(Self {
            payload,
            _charge: charge,
        })
    }
}

impl Drop for ApproxScratch {
    fn drop(&mut self) {
        // SAFETY: this owner holds the unique allocation from xmalloc.
        unsafe { mem::xfree(self.payload.as_ptr()) };
    }
}

const INITIAL_DELETION_SLOTS: usize = 32;

struct DeletionQueue {
    slots: WorkspaceBuffer<usize>,
    head: usize,
    len: usize,
}

impl DeletionQueue {
    fn new(workspace: &RegexWorkspace) -> Result<Self, ()> {
        Ok(Self {
            slots: workspace.try_buffer(0)?,
            head: 0,
            len: 0,
        })
    }

    fn push(&mut self, workspace: &RegexWorkspace, state: usize) -> Result<(), ()> {
        if self.len == self.slots.len() {
            let capacity = self
                .len
                .checked_mul(2)
                .ok_or(())?
                .max(INITIAL_DELETION_SLOTS);
            let mut replacement = workspace.try_buffer(capacity)?;
            replacement.try_resize(capacity, 0)?;
            // Growth occurs only when full: copy both occupied spans in FIFO order.
            let (wrapped, front) = self.slots.split_at(self.head);
            replacement[..front.len()].copy_from_slice(front);
            replacement[front.len()..self.len].copy_from_slice(wrapped);
            self.slots = replacement;
            self.head = 0;
        }
        let tail = (self.head + self.len) % self.slots.len();
        self.slots[tail] = state;
        self.len += 1;
        Ok(())
    }

    fn pop(&mut self) -> Option<usize> {
        if self.len == 0 {
            return None;
        }
        let state = self.slots[self.head];
        self.head = (self.head + 1) % self.slots.len();
        self.len -= 1;
        Some(state)
    }
}

#[inline]
fn IS_WORD_CHAR(c: tre_cint_t) -> bool {
    c == '_' as tre_cint_t || super::parse::tre_isalnum(c)
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
/// # Safety
///
/// `trans_i.neg_classes` must be null or point to a live 0-terminated class array (as built by
/// `tre_compile`).
unsafe fn CHECK_CHAR_CLASSES(
    trans_i: &tre_tnfa_transition_t,
    tnfa: &tre_tnfa_t,
    eflags: c_int,
    prev_c: tre_cint_t,
) -> bool {
    let assertions = trans_i.assertions;
    let icase = tnfa.cflags & REG_ICASE;
    ((assertions & ASSERT_CHAR_CLASS) != 0
        && icase == 0
        && !super::parse::tre_isctype(prev_c, trans_i.u.get_class()))
        || ((assertions & ASSERT_CHAR_CLASS) != 0
            && icase != 0
            && !super::parse::tre_isctype(super::parse::tre_tolower(prev_c), trans_i.u.get_class())
            && !super::parse::tre_isctype(super::parse::tre_toupper(prev_c), trans_i.u.get_class()))
        || ((assertions & ASSERT_CHAR_CLASS_NEG) != 0
            // SAFETY: pointers are live TRE objects forwarded under the enclosing function's `# Safety` contract.
            && unsafe { super::parse::tre_neg_char_classes_match(trans_i.neg_classes, prev_c, icase) } != 0)
}

fn tre_set_params(reach: &mut tre_tnfa_approx_reach_t, pa: &[c_int], default_params: regaparams_t) {
    let value = pa[TRE_PARAM_DEPTH];
    if value > reach.depth {
        for i in (reach.depth as usize + 1)..=value as usize {
            for j in 0..TRE_M_LAST {
                reach.costs[i][j] = 0;
            }
        }
    }
    reach.depth = value;

    let mut v = pa[TRE_PARAM_COST_INS];
    if v == TRE_PARAM_DEFAULT {
        reach.params.cost_ins = default_params.cost_ins;
    } else if v != TRE_PARAM_UNSET {
        reach.params.cost_ins = v;
    }

    v = pa[TRE_PARAM_COST_DEL];
    if v == TRE_PARAM_DEFAULT {
        reach.params.cost_del = default_params.cost_del;
    } else if v != TRE_PARAM_UNSET {
        reach.params.cost_del = v;
    }

    v = pa[TRE_PARAM_COST_SUBST];
    if v == TRE_PARAM_DEFAULT {
        reach.params.cost_subst = default_params.cost_subst;
    } else {
        reach.params.cost_subst = v;
    }

    v = pa[TRE_PARAM_COST_MAX];
    if v == TRE_PARAM_DEFAULT {
        reach.params.max_cost = default_params.max_cost;
    } else if v != TRE_PARAM_UNSET {
        reach.params.max_cost = v;
    }

    v = pa[TRE_PARAM_MAX_INS];
    if v == TRE_PARAM_DEFAULT {
        reach.params.max_ins = default_params.max_ins;
    } else if v != TRE_PARAM_UNSET {
        reach.params.max_ins = v;
    }

    v = pa[TRE_PARAM_MAX_DEL];
    if v == TRE_PARAM_DEFAULT {
        reach.params.max_del = default_params.max_del;
    } else if v != TRE_PARAM_UNSET {
        reach.params.max_del = v;
    }

    v = pa[TRE_PARAM_MAX_SUBST];
    if v == TRE_PARAM_DEFAULT {
        reach.params.max_subst = default_params.max_subst;
    } else if v != TRE_PARAM_UNSET {
        reach.params.max_subst = v;
    }

    v = pa[TRE_PARAM_MAX_ERR];
    if v == TRE_PARAM_DEFAULT {
        reach.params.max_err = default_params.max_err;
    } else if v != TRE_PARAM_UNSET {
        reach.params.max_err = v;
    }
}

/// # Safety
///
/// Every raw-pointer argument must point to a live, properly initialized TRE object (or a buffer
/// sized per the accompanying length/count arguments) valid for the reads and writes performed
/// here; null is accepted only where the original C TRE API accepts it.
pub unsafe fn tre_tnfa_run_approx(
    tnfa: *const tre_tnfa_t,
    string: *const c_void,
    len: c_int,
    type_: tre_str_type_t,
    match_tags: *mut c_int,
    amatch: *mut regamatch_t,
    default_params: regaparams_t,
    eflags: c_int,
    match_end_ofs: *mut c_int,
) -> c_int {
    // SAFETY: forwards the legacy caller's pointer and buffer contracts.
    unsafe {
        run_approx(
            tnfa,
            string,
            len,
            type_,
            match_tags,
            amatch,
            default_params,
            eflags,
            match_end_ofs,
            &RegexWorkspace::unbounded(),
        )
    }
}

#[allow(clippy::manual_memcpy)]
/// # Safety
///
/// Every raw-pointer argument must point to a live, properly initialized TRE object (or a buffer
/// sized per the accompanying length/count arguments) valid for the reads and writes performed
/// here; null is accepted only where the original C TRE API accepts it.
pub(super) unsafe fn run_approx(
    tnfa: *const tre_tnfa_t,
    string: *const c_void,
    len: c_int,
    type_: tre_str_type_t,
    match_tags: *mut c_int,
    amatch: *mut regamatch_t,
    default_params: regaparams_t,
    eflags: c_int,
    match_end_ofs: *mut c_int,
    workspace: &RegexWorkspace,
) -> c_int {
    unsafe {
        let str_byte = string as *const u8;
        let mut pos: c_int = -1;
        let pos_add_next: u32 = 1;
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
        let mut prev_pos: c_int;

        let mut match_eo: c_int = -1;
        let mut match_costs = [c_int::MAX; TRE_M_LAST];

        let Ok(state_count) = usize::try_from((*tnfa).num_states) else {
            return REG_ESPACE;
        };
        let Ok(tag_count) = usize::try_from(num_tags) else {
            return REG_ESPACE;
        };
        let sizes = || {
            let tag_bytes = std::mem::size_of::<c_int>().checked_mul(tag_count)?;
            let reach_bytes =
                std::mem::size_of::<tre_tnfa_approx_reach_t>().checked_mul(state_count)?;
            let tag_rows = state_count.checked_mul(2)?.checked_add(1)?;
            let total_bytes = reach_bytes
                .checked_mul(2)?
                .checked_add(tag_rows.checked_mul(tag_bytes)?)?
                .checked_add(64)?;
            Some((tag_bytes, reach_bytes, total_bytes))
        };
        let Some((tag_bytes, reach_bytes, total_bytes)) = sizes() else {
            return REG_ESPACE;
        };
        let Ok(scratch) = ApproxScratch::new(workspace, total_bytes) else {
            return REG_ESPACE;
        };
        let Ok(mut deletions) = DeletionQueue::new(workspace) else {
            return REG_ESPACE;
        };
        let buf = scratch.payload.as_ptr().cast::<u8>();
        ptr::write_bytes(buf, 0, total_bytes);

        let mut buf_offset = buf;
        let tmp_tags = buf_offset as *mut c_int;
        buf_offset = buf_offset.add(tag_bytes);
        buf_offset = align_ptr_approx(buf_offset);

        let reach = buf_offset as *mut tre_tnfa_approx_reach_t;
        buf_offset = buf_offset.add(reach_bytes);
        buf_offset = align_ptr_approx(buf_offset);

        let reach_next = buf_offset as *mut tre_tnfa_approx_reach_t;
        buf_offset = buf_offset.add(reach_bytes);
        buf_offset = align_ptr_approx(buf_offset);

        for i in 0..(*tnfa).num_states {
            (*reach.offset(i as isize)).tags = buf_offset as *mut c_int;
            buf_offset = buf_offset.add(tag_bytes);
            (*reach_next.offset(i as isize)).tags = buf_offset as *mut c_int;
            buf_offset = buf_offset.add(tag_bytes);
        }

        for i in 0..(*tnfa).num_states {
            (*reach.offset(i as isize)).pos = -2;
            (*reach_next.offset(i as isize)).pos = -2;
        }

        prev_pos = pos;
        // GET_NEXT_WCHAR
        prev_c = next_c;
        pos = 0;
        if len >= 0 && pos >= len {
            next_c = 0;
        } else if type_ == tre_str_type_t::STR_BYTE && !str_byte.is_null() {
            next_c = *str_byte as tre_cint_t;
        } else {
            next_c = 0;
        }

        loop {
            // Add initial states
            if match_costs[TRE_M_COST] > 0 {
                let mut trans = (*tnfa).initial;
                while !(*trans).state.is_null() {
                    let stateid = (*trans).state_id;
                    if (*reach_next.offset(stateid as isize)).pos < pos {
                        if (*trans).assertions != 0
                            && CHECK_ASSERTIONS(
                                (*trans).assertions,
                                pos,
                                prev_c,
                                next_c,
                                reg_notbol,
                                reg_noteol,
                                reg_newline,
                            )
                        {
                            trans = trans.add(1);
                            continue;
                        }
                        (*reach_next.offset(stateid as isize)).state = (*trans).state;
                        (*reach_next.offset(stateid as isize)).pos = pos;

                        for i in 0..num_tags {
                            *(*reach_next.offset(stateid as isize))
                                .tags
                                .offset(i as isize) = -1;
                        }
                        if !(*trans).tags.is_null() {
                            let mut ti: c_int = 0;
                            while *(*trans).tags.offset(ti as isize) >= 0 {
                                if *(*trans).tags.offset(ti as isize) < num_tags {
                                    *(*reach_next.offset(stateid as isize))
                                        .tags
                                        .offset(*(*trans).tags.offset(ti as isize) as isize) = pos;
                                }
                                ti += 1;
                            }
                        }

                        (*reach_next.offset(stateid as isize)).params = default_params;
                        (*reach_next.offset(stateid as isize)).depth = 0;
                        for j in 0..TRE_M_LAST {
                            (*reach_next.offset(stateid as isize)).costs[0][j] = 0;
                        }
                        if !(*trans).params.is_null() {
                            tre_set_params(
                                &mut *reach_next.offset(stateid as isize),
                                std::slice::from_raw_parts((*trans).params, TRE_PARAM_LAST),
                                default_params,
                            );
                        }

                        if (*trans).state == (*tnfa).final_ {
                            match_eo = pos;
                            for i in 0..num_tags {
                                *match_tags.offset(i as isize) = *(*reach_next
                                    .offset(stateid as isize))
                                .tags
                                .offset(i as isize);
                            }
                            for j in 0..TRE_M_LAST {
                                match_costs[j] = 0;
                            }
                        }
                    }
                    trans = trans.add(1);
                }
            }

            // Handle inserts
            for id in 0..(*tnfa).num_states {
                if (*reach.offset(id as isize)).pos != prev_pos {
                    continue;
                }
                let depth = (*reach.offset(id as isize)).depth as usize;

                let mut cost = (*reach.offset(id as isize)).costs[depth][TRE_M_COST];
                if (*reach.offset(id as isize)).params.cost_ins != TRE_PARAM_UNSET {
                    cost += (*reach.offset(id as isize)).params.cost_ins;
                }
                if cost > (*reach.offset(id as isize)).params.max_cost {
                    continue;
                }
                if (*reach.offset(id as isize)).costs[depth][TRE_M_NUM_INS] + 1
                    > (*reach.offset(id as isize)).params.max_ins
                {
                    continue;
                }
                if (*reach.offset(id as isize)).costs[depth][TRE_M_NUM_ERR] + 1
                    > (*reach.offset(id as isize)).params.max_err
                {
                    continue;
                }

                let mut cost0 = cost;
                if depth > 0 {
                    cost0 = (*reach.offset(id as isize)).costs[0][TRE_M_COST];
                    if (*reach.offset(id as isize)).params.cost_ins != TRE_PARAM_UNSET {
                        cost0 += (*reach.offset(id as isize)).params.cost_ins;
                    } else {
                        cost0 += default_params.cost_ins;
                    }
                }

                if (*reach_next.offset(id as isize)).pos == pos
                    && cost0 >= (*reach_next.offset(id as isize)).costs[0][TRE_M_COST]
                {
                    continue;
                }

                (*reach_next.offset(id as isize)).state = (*reach.offset(id as isize)).state;
                (*reach_next.offset(id as isize)).pos = pos;
                for i in 0..num_tags {
                    *(*reach_next.offset(id as isize)).tags.offset(i as isize) =
                        *(*reach.offset(id as isize)).tags.offset(i as isize);
                }
                (*reach_next.offset(id as isize)).params = (*reach.offset(id as isize)).params;
                (*reach_next.offset(id as isize)).depth = (*reach.offset(id as isize)).depth;

                let copy_len = TRE_M_LAST * (depth + 1);
                for j in 0..copy_len {
                    let d = j / TRE_M_LAST;
                    let c = j % TRE_M_LAST;
                    (*reach_next.offset(id as isize)).costs[d][c] =
                        (*reach.offset(id as isize)).costs[d][c];
                }
                (*reach_next.offset(id as isize)).costs[depth][TRE_M_COST] = cost;
                (*reach_next.offset(id as isize)).costs[depth][TRE_M_NUM_INS] += 1;
                (*reach_next.offset(id as isize)).costs[depth][TRE_M_NUM_ERR] += 1;
                if depth > 0 {
                    (*reach_next.offset(id as isize)).costs[0][TRE_M_COST] = cost0;
                    (*reach_next.offset(id as isize)).costs[0][TRE_M_NUM_INS] += 1;
                    (*reach_next.offset(id as isize)).costs[0][TRE_M_NUM_ERR] += 1;
                }
            }

            // Seed deletion closure in state order, then visit successors FIFO.
            {
                for id in 0..state_count {
                    if (*reach_next.add(id)).pos == pos
                        && deletions.push(workspace, id).is_err()
                    {
                        return REG_ESPACE;
                    }
                }

                while let Some(id) = deletions.pop() {
                    // Queued ids are bounded: only `0..state_count` seeds and
                    // checked transition targets below are ever pushed.
                    let reach_p = reach_next.add(id);
                    let depth = (*reach_p).depth as usize;

                    let mut cost = (*reach_p).costs[depth][TRE_M_COST];
                    if (*reach_p).params.cost_del != TRE_PARAM_UNSET {
                        cost += (*reach_p).params.cost_del;
                    }

                    if cost > (*reach_p).params.max_cost
                        || (*reach_p).costs[depth][TRE_M_NUM_DEL] + 1 > (*reach_p).params.max_del
                        || (*reach_p).costs[depth][TRE_M_NUM_ERR] + 1 > (*reach_p).params.max_err
                    {
                        continue;
                    }

                    let mut cost0 = cost;
                    if depth > 0 {
                        cost0 = (*reach_p).costs[0][TRE_M_COST];
                        if (*reach_p).params.cost_del != TRE_PARAM_UNSET {
                            cost0 += (*reach_p).params.cost_del;
                        } else {
                            cost0 += default_params.cost_del;
                        }
                    }

                    let mut trans = (*reach_p).state;
                    while !(*trans).state.is_null() {
                        let Ok(dest_id) = usize::try_from((*trans).state_id) else {
                            return REG_ESPACE;
                        };
                        if dest_id >= state_count {
                            return REG_ESPACE;
                        }

                        if (*trans).assertions != 0
                            && CHECK_ASSERTIONS(
                                (*trans).assertions,
                                pos,
                                prev_c,
                                next_c,
                                reg_notbol,
                                reg_noteol,
                                reg_newline,
                            )
                        {
                            trans = trans.add(1);
                            continue;
                        }

                        for i in 0..num_tags {
                            *tmp_tags.offset(i as isize) = *(*reach_p).tags.offset(i as isize);
                        }
                        if !(*trans).tags.is_null() {
                            let mut ti: c_int = 0;
                            while *(*trans).tags.offset(ti as isize) >= 0 {
                                if *(*trans).tags.offset(ti as isize) < num_tags {
                                    *tmp_tags.offset(*(*trans).tags.offset(ti as isize) as isize) =
                                        pos;
                                }
                                ti += 1;
                            }
                        }

                        if (*reach_next.add(dest_id)).pos == pos
                            && (cost0 > (*reach_next.add(dest_id)).costs[0][TRE_M_COST]
                                || (cost0
                                    == (*reach_next.add(dest_id)).costs[0][TRE_M_COST]
                                    && (match_tags.is_null()
                                        || tre_tag_order(
                                            std::slice::from_raw_parts(
                                                ptr_or_dangling((*tnfa).tag_directions),
                                                num_tags as usize,
                                            ),
                                            std::slice::from_raw_parts(
                                                ptr_or_dangling(tmp_tags),
                                                num_tags as usize,
                                            ),
                                            std::slice::from_raw_parts(
                                                ptr_or_dangling(
                                                    (*reach_next.add(dest_id)).tags,
                                                ),
                                                num_tags as usize,
                                            ),
                                        ) == 0)))
                        {
                            trans = trans.add(1);
                            continue;
                        }

                        (*reach_next.add(dest_id)).state = (*trans).state;
                        (*reach_next.add(dest_id)).pos = pos;
                        for i in 0..num_tags {
                            *(*reach_next.add(dest_id))
                                .tags
                                .offset(i as isize) = *tmp_tags.offset(i as isize);
                        }

                        (*reach_next.add(dest_id)).params = (*reach_p).params;
                        if !(*trans).params.is_null() {
                            tre_set_params(
                                &mut *reach_next.add(dest_id),
                                std::slice::from_raw_parts((*trans).params, TRE_PARAM_LAST),
                                default_params,
                            );
                        }

                        (*reach_next.add(dest_id)).depth = (*reach_p).depth;
                        let copy_len = TRE_M_LAST * (depth + 1);
                        for j in 0..copy_len {
                            let d = j / TRE_M_LAST;
                            let c = j % TRE_M_LAST;
                            (*reach_next.add(dest_id)).costs[d][c] =
                                (*reach_p).costs[d][c];
                        }
                        (*reach_next.add(dest_id)).costs[depth][TRE_M_COST] = cost;
                        (*reach_next.add(dest_id)).costs[depth][TRE_M_NUM_DEL] += 1;
                        (*reach_next.add(dest_id)).costs[depth][TRE_M_NUM_ERR] += 1;
                        if depth > 0 {
                            (*reach_next.add(dest_id)).costs[0][TRE_M_COST] = cost0;
                            (*reach_next.add(dest_id)).costs[0][TRE_M_NUM_DEL] += 1;
                            (*reach_next.add(dest_id)).costs[0][TRE_M_NUM_ERR] += 1;
                        }

                        if (*trans).state == (*tnfa).final_
                            && (match_eo < 0
                                || match_costs[TRE_M_COST] > cost0
                                || (match_costs[TRE_M_COST] == cost0
                                    && num_tags > 0
                                    && *tmp_tags <= *match_tags))
                        {
                            match_eo = pos;
                            for j in 0..TRE_M_LAST {
                                match_costs[j] = (*reach_next.add(dest_id)).costs[0][j];
                            }
                            for i in 0..num_tags {
                                *match_tags.offset(i as isize) = *tmp_tags.offset(i as isize);
                            }
                        }

                        if deletions.push(workspace, dest_id).is_err() {
                            return REG_ESPACE;
                        }

                        trans = trans.add(1);
                    }
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

            prev_pos = pos;
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

            // Handle exact matches and substitutions
            for id in 0..(*tnfa).num_states {
                if (*reach.offset(id as isize)).pos < prev_pos {
                    continue;
                }
                let mut trans = (*reach.offset(id as isize)).state;
                while !(*trans).state.is_null() {
                    let dest_id = (*trans).state_id;
                    let depth = (*reach.offset(id as isize)).depth as usize;

                    let mut cost = (*reach.offset(id as isize)).costs[depth][TRE_M_COST];
                    let mut cost0 = (*reach.offset(id as isize)).costs[0][TRE_M_COST];
                    let mut err: c_int = 0;

                    if (*trans).assertions != 0
                        && (CHECK_ASSERTIONS(
                            (*trans).assertions,
                            pos,
                            prev_c,
                            next_c,
                            reg_notbol,
                            reg_noteol,
                            reg_newline,
                        ) || CHECK_CHAR_CLASSES(&*trans, &*tnfa, eflags, prev_c))
                    {
                        trans = trans.add(1);
                        continue;
                    }

                    if (*trans).code_min > prev_c || (*trans).code_max < prev_c {
                        err = 1;
                        cost = (*reach.offset(id as isize)).costs[depth][TRE_M_COST];
                        if (*reach.offset(id as isize)).params.cost_subst != TRE_PARAM_UNSET {
                            cost += (*reach.offset(id as isize)).params.cost_subst;
                        }
                        if cost > (*reach.offset(id as isize)).params.max_cost {
                            trans = trans.add(1);
                            continue;
                        }
                        if (*reach.offset(id as isize)).costs[depth][TRE_M_NUM_SUBST] + 1
                            > (*reach.offset(id as isize)).params.max_subst
                        {
                            trans = trans.add(1);
                            continue;
                        }
                        if (*reach.offset(id as isize)).costs[depth][TRE_M_NUM_ERR] + 1
                            > (*reach.offset(id as isize)).params.max_err
                        {
                            trans = trans.add(1);
                            continue;
                        }
                        cost0 = cost;
                        if depth > 0 {
                            cost0 = (*reach.offset(id as isize)).costs[0][TRE_M_COST];
                            if (*reach.offset(id as isize)).params.cost_subst != TRE_PARAM_UNSET {
                                cost0 += (*reach.offset(id as isize)).params.cost_subst;
                            } else {
                                cost0 += default_params.cost_subst;
                            }
                        }
                    }

                    for i in 0..num_tags {
                        *tmp_tags.offset(i as isize) =
                            *(*reach.offset(id as isize)).tags.offset(i as isize);
                    }
                    if !(*trans).tags.is_null() {
                        let mut ti: c_int = 0;
                        while *(*trans).tags.offset(ti as isize) >= 0 {
                            if *(*trans).tags.offset(ti as isize) < num_tags {
                                *tmp_tags.offset(*(*trans).tags.offset(ti as isize) as isize) = pos;
                            }
                            ti += 1;
                        }
                    }

                    if (*reach_next.offset(dest_id as isize)).pos == pos
                        && (cost0 > (*reach_next.offset(dest_id as isize)).costs[0][TRE_M_COST]
                            || (cost0
                                == (*reach_next.offset(dest_id as isize)).costs[0][TRE_M_COST]
                                && tre_tag_order(
                                    std::slice::from_raw_parts(
                                        ptr_or_dangling((*tnfa).tag_directions),
                                        num_tags as usize,
                                    ),
                                    std::slice::from_raw_parts(
                                        ptr_or_dangling(tmp_tags),
                                        num_tags as usize,
                                    ),
                                    std::slice::from_raw_parts(
                                        ptr_or_dangling(
                                            (*reach_next.offset(dest_id as isize)).tags,
                                        ),
                                        num_tags as usize,
                                    ),
                                ) == 0))
                    {
                        trans = trans.add(1);
                        continue;
                    }

                    (*reach_next.offset(dest_id as isize)).state = (*trans).state;
                    (*reach_next.offset(dest_id as isize)).pos = pos;
                    for i in 0..num_tags {
                        *(*reach_next.offset(dest_id as isize))
                            .tags
                            .offset(i as isize) = *tmp_tags.offset(i as isize);
                    }
                    (*reach_next.offset(dest_id as isize)).depth =
                        (*reach.offset(id as isize)).depth;

                    (*reach_next.offset(dest_id as isize)).params =
                        (*reach.offset(id as isize)).params;
                    if !(*trans).params.is_null() {
                        tre_set_params(
                            &mut *reach_next.offset(dest_id as isize),
                            std::slice::from_raw_parts((*trans).params, TRE_PARAM_LAST),
                            default_params,
                        );
                    }

                    let copy_len = TRE_M_LAST * (depth + 1);
                    for j in 0..copy_len {
                        let d = j / TRE_M_LAST;
                        let c = j % TRE_M_LAST;
                        (*reach_next.offset(dest_id as isize)).costs[d][c] =
                            (*reach.offset(id as isize)).costs[d][c];
                    }
                    (*reach_next.offset(dest_id as isize)).costs[depth][TRE_M_COST] = cost;
                    (*reach_next.offset(dest_id as isize)).costs[depth][TRE_M_NUM_SUBST] += err;
                    (*reach_next.offset(dest_id as isize)).costs[depth][TRE_M_NUM_ERR] += err;
                    if depth > 0 {
                        (*reach_next.offset(dest_id as isize)).costs[0][TRE_M_COST] = cost0;
                        (*reach_next.offset(dest_id as isize)).costs[0][TRE_M_NUM_SUBST] += err;
                        (*reach_next.offset(dest_id as isize)).costs[0][TRE_M_NUM_ERR] += err;
                    }

                    if (*trans).state == (*tnfa).final_
                        && (match_eo < 0
                            || cost0 < match_costs[TRE_M_COST]
                            || (cost0 == match_costs[TRE_M_COST]
                                && num_tags > 0
                                && *tmp_tags <= *match_tags))
                    {
                        match_eo = pos;
                        for j in 0..TRE_M_LAST {
                            match_costs[j] = (*reach_next.offset(dest_id as isize)).costs[0][j];
                        }
                        for i in 0..num_tags {
                            *match_tags.offset(i as isize) = *tmp_tags.offset(i as isize);
                        }
                    }

                    trans = trans.add(1);
                }
            }
        }

        (*amatch).cost = match_costs[TRE_M_COST];
        (*amatch).num_ins = match_costs[TRE_M_NUM_INS];
        (*amatch).num_del = match_costs[TRE_M_NUM_DEL];
        (*amatch).num_subst = match_costs[TRE_M_NUM_SUBST];
        *match_end_ofs = match_eo;

        if match_eo >= 0 {
            REG_OK as c_int
        } else {
            REG_NOMATCH as c_int
        }
    }
}

fn align_ptr_approx(p: *mut u8) -> *mut u8 {
    p.map_addr(|addr| addr.next_multiple_of(std::mem::align_of::<usize>()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deletion_queue_preserves_wrapped_order_through_growth() {
        let workspace = RegexWorkspace::unbounded();
        let mut queue = DeletionQueue::new(&workspace).unwrap();
        for id in 0..32 {
            queue.push(&workspace, id).unwrap();
        }
        for id in 0..20 {
            assert_eq!(queue.pop(), Some(id));
        }
        for id in 32..400 {
            queue.push(&workspace, id).unwrap();
        }
        for id in 20..400 {
            assert_eq!(queue.pop(), Some(id));
        }
        assert_eq!(queue.pop(), None);
        queue.push(&workspace, 9).unwrap();
        assert_eq!(queue.pop(), Some(9));
    }

    #[test]
    fn deletion_queue_denial_keeps_entries_and_releases_workspace() {
        use crate::sexp::{memory::ArenaBudget, session::RSession};
        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let owner = session.owner_token().unwrap();
            let workspace = crate::mainutils::grep::regex_workspace();
            let baseline = owner.with_arena(|arena| arena.workspace_bytes()).unwrap();
            let mut queue = DeletionQueue::new(&workspace).unwrap();
            for id in 0..32 {
                queue.push(&workspace, id).unwrap();
            }
            let charged = owner
                .with_arena(|arena| {
                    let charged = arena.workspace_bytes();
                    let limit = arena.total_bytes_allocated() + charged
                        + 64 * std::mem::size_of::<usize>()
                        - 1;
                    arena.set_budget(ArenaBudget::new(limit, 0));
                    charged
                })
                .unwrap();
            assert_eq!(queue.push(&workspace, 32), Err(()));
            assert_eq!(
                owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
                charged
            );
            assert_eq!(queue.pop(), Some(0));
            queue.push(&workspace, 32).unwrap();
            for id in 1..=32 {
                assert_eq!(queue.pop(), Some(id));
            }
            assert_eq!(queue.pop(), None);
            drop(queue);
            assert_eq!(
                owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
                baseline
            );
        });
    }
}
