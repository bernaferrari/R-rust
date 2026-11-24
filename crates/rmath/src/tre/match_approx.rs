#![allow(unused_variables)]
#![allow(unused_assignments)]
/*
  tre/match_approx.rs - TRE approximate regex matching engine

  Ported from tre-match-approx.c
*/
#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use std::os::raw::{c_int, c_void};
use std::ptr;

use super::ast::*;
use super::match_input::MatchInput;
use super::match_parallel::{TagRows, apply_tags, filled};
use crate::mainutils::grep::{RegexWorkspace, WorkspaceBuffer};

const TRE_M_COST: usize = 0;
const TRE_M_NUM_INS: usize = 1;
const TRE_M_NUM_DEL: usize = 2;
const TRE_M_NUM_SUBST: usize = 3;
const TRE_M_NUM_ERR: usize = 4;
const TRE_M_LAST: usize = 5;
const TRE_M_MAX_DEPTH: usize = 3;

#[derive(Clone)]
struct Reach {
    state: *mut tre_tnfa_transition_t,
    pos: c_int,
    params: regaparams_t,
    depth: c_int,
    costs: [[c_int; TRE_M_LAST]; TRE_M_MAX_DEPTH + 1],
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

fn tre_set_params(reach: &mut Reach, pa: &[c_int], default_params: regaparams_t) {
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
    // SAFETY: the raw matcher contract supplies the input extent or terminator.
    let input = match unsafe { MatchInput::from_raw(string, len, type_) } {
        Ok(input) => input,
        Err(status) => return status,
    };
    let input_len = input.len() as c_int;
    unsafe {
        let mut pos: c_int = -1;
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
        let scratch = || {
            let tmp_row = state_count.checked_mul(2).ok_or(())?;
            let rows = tmp_row.checked_add(1).ok_or(())?;
            let empty = Reach {
                state: ptr::null_mut(),
                pos: -2,
                params: regaparams_t {
                    cost_ins: 0,
                    cost_del: 0,
                    cost_subst: 0,
                    max_cost: 0,
                    max_ins: 0,
                    max_del: 0,
                    max_subst: 0,
                    max_err: 0,
                },
                depth: 0,
                costs: [[0; TRE_M_LAST]; TRE_M_MAX_DEPTH + 1],
            };
            let reach = filled(workspace, state_count, empty.clone())?;
            let reach_next = filled(workspace, state_count, empty)?;
            let tags = TagRows::new(workspace, rows, tag_count)?;
            let deletions = DeletionQueue::new(workspace)?;
            Ok::<_, ()>((reach, reach_next, tags, deletions, tmp_row))
        };
        let Ok((mut reach, mut reach_next, mut tags, mut deletions, tmp_row)) = scratch()
        else {
            return REG_ESPACE;
        };
        let (mut reach_tag_base, mut next_tag_base) = (0, state_count);
        let directions = if tag_count == 0 {
            &[][..]
        } else {
            // SAFETY: the live compiled descriptor owns this many entries.
            std::slice::from_raw_parts((*tnfa).tag_directions, tag_count)
        };

        prev_pos = pos;
        // GET_NEXT_WCHAR
        prev_c = next_c;
        pos = 0;
        next_c = input.at(pos);

        loop {
            // Add initial states
            if match_costs[TRE_M_COST] > 0 {
                let mut trans = (*tnfa).initial;
                while !(*trans).state.is_null() {
                    let Ok(stateid) = usize::try_from((*trans).state_id) else {
                        return REG_ESPACE;
                    };
                    let Some(next) = reach_next.get_mut(stateid) else {
                        return REG_ESPACE;
                    };
                    if next.pos < pos {
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
                        next.state = (*trans).state;
                        next.pos = pos;
                        let row = tags.row_mut(next_tag_base + stateid);
                        row.fill(-1);
                        apply_tags((*trans).tags, row, pos);

                        next.params = default_params;
                        next.depth = 0;
                        next.costs[0].fill(0);
                        if !(*trans).params.is_null() {
                            tre_set_params(
                                next,
                                std::slice::from_raw_parts((*trans).params, TRE_PARAM_LAST),
                                default_params,
                            );
                        }

                        if (*trans).state == (*tnfa).final_ {
                            match_eo = pos;
                            if !row.is_empty() {
                                ptr::copy_nonoverlapping(row.as_ptr(), match_tags, row.len());
                            }
                            match_costs.fill(0);
                        }
                    }
                    trans = trans.add(1);
                }
            }

            // Handle inserts
            for (id, (source, next)) in reach.iter().zip(reach_next.iter_mut()).enumerate() {
                if source.pos != prev_pos {
                    continue;
                }
                let depth = source.depth as usize;

                let mut cost = source.costs[depth][TRE_M_COST];
                if source.params.cost_ins != TRE_PARAM_UNSET {
                    cost += source.params.cost_ins;
                }
                if cost > source.params.max_cost {
                    continue;
                }
                if source.costs[depth][TRE_M_NUM_INS] + 1 > source.params.max_ins {
                    continue;
                }
                if source.costs[depth][TRE_M_NUM_ERR] + 1 > source.params.max_err {
                    continue;
                }

                let mut cost0 = cost;
                if depth > 0 {
                    cost0 = source.costs[0][TRE_M_COST];
                    if source.params.cost_ins != TRE_PARAM_UNSET {
                        cost0 += source.params.cost_ins;
                    } else {
                        cost0 += default_params.cost_ins;
                    }
                }

                if next.pos == pos && cost0 >= next.costs[0][TRE_M_COST] {
                    continue;
                }

                next.state = source.state;
                next.pos = pos;
                tags.copy_row(reach_tag_base + id, next_tag_base + id);
                next.params = source.params;
                next.depth = source.depth;
                next.costs[..=depth].copy_from_slice(&source.costs[..=depth]);
                next.costs[depth][TRE_M_COST] = cost;
                next.costs[depth][TRE_M_NUM_INS] += 1;
                next.costs[depth][TRE_M_NUM_ERR] += 1;
                if depth > 0 {
                    next.costs[0][TRE_M_COST] = cost0;
                    next.costs[0][TRE_M_NUM_INS] += 1;
                    next.costs[0][TRE_M_NUM_ERR] += 1;
                }
            }

            // Seed deletion closure in state order, then visit successors FIFO.
            {
                for (id, next) in reach_next.iter().enumerate() {
                    if next.pos == pos && deletions.push(workspace, id).is_err() {
                        return REG_ESPACE;
                    }
                }

                while let Some(id) = deletions.pop() {
                    // Queued ids are bounded: only `0..state_count` seeds and
                    // checked transition targets below are ever pushed.
                    let depth = reach_next[id].depth as usize;

                    let mut cost = reach_next[id].costs[depth][TRE_M_COST];
                    if reach_next[id].params.cost_del != TRE_PARAM_UNSET {
                        cost += reach_next[id].params.cost_del;
                    }

                    if cost > reach_next[id].params.max_cost
                        || reach_next[id].costs[depth][TRE_M_NUM_DEL] + 1
                            > reach_next[id].params.max_del
                        || reach_next[id].costs[depth][TRE_M_NUM_ERR] + 1
                            > reach_next[id].params.max_err
                    {
                        continue;
                    }

                    let mut cost0 = cost;
                    if depth > 0 {
                        cost0 = reach_next[id].costs[0][TRE_M_COST];
                        if reach_next[id].params.cost_del != TRE_PARAM_UNSET {
                            cost0 += reach_next[id].params.cost_del;
                        } else {
                            cost0 += default_params.cost_del;
                        }
                    }

                    let mut trans = reach_next[id].state;
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

                        tags.copy_row(next_tag_base + id, tmp_row);
                        apply_tags((*trans).tags, tags.row_mut(tmp_row), pos);

                        if reach_next[dest_id].pos == pos
                            && (cost0 > reach_next[dest_id].costs[0][TRE_M_COST]
                                || (cost0 == reach_next[dest_id].costs[0][TRE_M_COST]
                                    && (tag_count == 0
                                        || tre_tag_order(
                                            directions,
                                            tags.row(tmp_row),
                                            tags.row(next_tag_base + dest_id),
                                        ) == 0)))
                        {
                            trans = trans.add(1);
                            continue;
                        }

                        reach_next[dest_id].state = (*trans).state;
                        reach_next[dest_id].pos = pos;
                        tags.copy_row(tmp_row, next_tag_base + dest_id);
                        reach_next[dest_id].params = reach_next[id].params;
                        if !(*trans).params.is_null() {
                            tre_set_params(
                                &mut reach_next[dest_id],
                                std::slice::from_raw_parts((*trans).params, TRE_PARAM_LAST),
                                default_params,
                            );
                        }

                        // A self-transition can update the source depth above.
                        reach_next[dest_id].depth = reach_next[id].depth;
                        if dest_id != id {
                            let costs = reach_next[id].costs;
                            reach_next[dest_id].costs[..=depth]
                                .copy_from_slice(&costs[..=depth]);
                        }
                        let next = &mut reach_next[dest_id];
                        next.costs[depth][TRE_M_COST] = cost;
                        next.costs[depth][TRE_M_NUM_DEL] += 1;
                        next.costs[depth][TRE_M_NUM_ERR] += 1;
                        if depth > 0 {
                            next.costs[0][TRE_M_COST] = cost0;
                            next.costs[0][TRE_M_NUM_DEL] += 1;
                            next.costs[0][TRE_M_NUM_ERR] += 1;
                        }

                        if (*trans).state == (*tnfa).final_
                            && (match_eo < 0
                                || match_costs[TRE_M_COST] > cost0
                                || (match_costs[TRE_M_COST] == cost0
                                    && tag_count > 0
                                    && tags.row(tmp_row)[0] <= *match_tags))
                        {
                            match_eo = pos;
                            match_costs = next.costs[0];
                            let row = tags.row(tmp_row);
                            if !row.is_empty() {
                                ptr::copy_nonoverlapping(row.as_ptr(), match_tags, row.len());
                            }
                        }

                        if deletions.push(workspace, dest_id).is_err() {
                            return REG_ESPACE;
                        }

                        trans = trans.add(1);
                    }
                }
            }

            if pos >= input_len {
                break;
            }
            prev_pos = pos;
            prev_c = next_c;
            pos += 1;
            next_c = input.at(pos);

            // The current position becomes the source for exact matches and substitutions.
            // Keep each capture-tag row paired with its reach table.
            std::mem::swap(&mut reach, &mut reach_next);
            std::mem::swap(&mut reach_tag_base, &mut next_tag_base);
            for (id, source) in reach.iter().enumerate() {
                if source.pos < prev_pos {
                    continue;
                }
                let mut trans = source.state;
                while !(*trans).state.is_null() {
                    let Ok(dest_id) = usize::try_from((*trans).state_id) else {
                        return REG_ESPACE;
                    };
                    let Some(next) = reach_next.get_mut(dest_id) else {
                        return REG_ESPACE;
                    };
                    let depth = source.depth as usize;

                    let mut cost = source.costs[depth][TRE_M_COST];
                    let mut cost0 = source.costs[0][TRE_M_COST];
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
                        cost = source.costs[depth][TRE_M_COST];
                        if source.params.cost_subst != TRE_PARAM_UNSET {
                            cost += source.params.cost_subst;
                        }
                        if cost > source.params.max_cost {
                            trans = trans.add(1);
                            continue;
                        }
                        if source.costs[depth][TRE_M_NUM_SUBST] + 1 > source.params.max_subst {
                            trans = trans.add(1);
                            continue;
                        }
                        if source.costs[depth][TRE_M_NUM_ERR] + 1 > source.params.max_err {
                            trans = trans.add(1);
                            continue;
                        }
                        cost0 = cost;
                        if depth > 0 {
                            cost0 = source.costs[0][TRE_M_COST];
                            if source.params.cost_subst != TRE_PARAM_UNSET {
                                cost0 += source.params.cost_subst;
                            } else {
                                cost0 += default_params.cost_subst;
                            }
                        }
                    }

                    tags.copy_row(reach_tag_base + id, tmp_row);
                    apply_tags((*trans).tags, tags.row_mut(tmp_row), pos);

                    if next.pos == pos
                        && (cost0 > next.costs[0][TRE_M_COST]
                            || (cost0 == next.costs[0][TRE_M_COST]
                                && tre_tag_order(
                                    directions,
                                    tags.row(tmp_row),
                                    tags.row(next_tag_base + dest_id),
                                ) == 0))
                    {
                        trans = trans.add(1);
                        continue;
                    }

                    next.state = (*trans).state;
                    next.pos = pos;
                    tags.copy_row(tmp_row, next_tag_base + dest_id);
                    next.depth = source.depth;
                    next.params = source.params;
                    if !(*trans).params.is_null() {
                        tre_set_params(
                            next,
                            std::slice::from_raw_parts((*trans).params, TRE_PARAM_LAST),
                            default_params,
                        );
                    }

                    next.costs[..=depth].copy_from_slice(&source.costs[..=depth]);
                    next.costs[depth][TRE_M_COST] = cost;
                    next.costs[depth][TRE_M_NUM_SUBST] += err;
                    next.costs[depth][TRE_M_NUM_ERR] += err;
                    if depth > 0 {
                        next.costs[0][TRE_M_COST] = cost0;
                        next.costs[0][TRE_M_NUM_SUBST] += err;
                        next.costs[0][TRE_M_NUM_ERR] += err;
                    }

                    if (*trans).state == (*tnfa).final_
                        && (match_eo < 0
                            || cost0 < match_costs[TRE_M_COST]
                            || (cost0 == match_costs[TRE_M_COST]
                                && tag_count > 0
                                && tags.row(tmp_row)[0] <= *match_tags))
                    {
                        match_eo = pos;
                        match_costs = next.costs[0];
                        let row = tags.row(tmp_row);
                        if !row.is_empty() {
                            ptr::copy_nonoverlapping(row.as_ptr(), match_tags, row.len());
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

    #[test]
    fn approximate_scratch_denial_releases_partial_workspace() {
        use crate::sexp::{memory::ArenaBudget, session::RSession};
        use crate::tre::regapi::{tre_regcomp, tre_regfree};

        let mut regex = regex_t {
            re_nsub: 0,
            value: ptr::null_mut(),
        };
        // SAFETY: the pattern outlives the call and `regex` is writable output.
        let rc = unsafe { tre_regcomp(&mut regex, c"^abc$".as_ptr(), REG_EXTENDED) };
        assert_eq!(rc, REG_OK);
        let tnfa = regex.value.cast::<tre_tnfa_t>();
        // SAFETY: `tre_regcomp` produced a live compiled descriptor.
        let (state_count, tag_count) = unsafe {
            (
                usize::try_from((*tnfa).num_states).unwrap(),
                usize::try_from((*tnfa).num_tags).unwrap(),
            )
        };

        let session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let owner = session.owner_token().unwrap();
            let workspace = crate::mainutils::grep::regex_workspace();
            let baseline = owner
                .with_arena(|arena| arena.workspace_bytes())
                .unwrap();
            owner
                .with_arena(|arena| {
                    arena.set_budget(ArenaBudget::new(
                        arena.total_bytes_allocated()
                            + baseline
                            + state_count * std::mem::size_of::<Reach>(),
                        0,
                    ));
                })
                .unwrap();

            let mut tags = vec![0; tag_count];
            let tags_ptr = if tags.is_empty() {
                ptr::null_mut()
            } else {
                tags.as_mut_ptr()
            };
            let mut amatch = regamatch_t {
                nmatch: 0,
                pmatch: ptr::null_mut(),
                cost: -1,
                num_ins: -1,
                num_del: -1,
                num_subst: -1,
            };
            let mut end = -1;
            let params = regaparams_t {
                cost_ins: 1,
                cost_del: 1,
                cost_subst: 1,
                max_cost: 3,
                max_ins: 0,
                max_del: 3,
                max_subst: 0,
                max_err: 3,
            };

            // SAFETY: the descriptor, input, and output storage remain live.
            let status = unsafe {
                run_approx(
                    tnfa,
                    c"".as_ptr().cast(),
                    0,
                    tre_str_type_t::STR_BYTE,
                    tags_ptr,
                    &mut amatch,
                    params,
                    0,
                    &mut end,
                    &workspace,
                )
            };
            assert_eq!(status, REG_ESPACE);
            assert_eq!(end, -1);
            assert_eq!(
                owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
                baseline
            );

            owner
                .with_arena(|arena| arena.set_budget(ArenaBudget::unlimited()))
                .unwrap();
            // SAFETY: the descriptor, input, and output storage remain live.
            let status = unsafe {
                run_approx(
                    tnfa,
                    c"".as_ptr().cast(),
                    0,
                    tre_str_type_t::STR_BYTE,
                    tags_ptr,
                    &mut amatch,
                    params,
                    0,
                    &mut end,
                    &workspace,
                )
            };
            assert_eq!(status, REG_OK);
            assert_eq!((end, amatch.cost, amatch.num_del), (0, 3, 3));
            assert_eq!(
                owner.with_arena(|arena| arena.workspace_bytes()).unwrap(),
                baseline
            );
        });
        // SAFETY: the regex was compiled above and has no remaining borrowers.
        unsafe { tre_regfree(&mut regex) };
    }
}
