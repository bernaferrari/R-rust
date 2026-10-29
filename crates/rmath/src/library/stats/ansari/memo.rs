//! Lazy counts and traversal storage charged to their original session ledger.
#![forbid(unsafe_code)]
use crate::sexp::{
    memory::WorkspaceReservation,
    owner::{OwnerToken, WeakOwner},
};
pub(super) fn memory_error() -> String {
    "Ansari workspace exceeds session allocation limit".into()
}
#[derive(Clone, Copy)]
pub(super) struct Dimensions {
    pub m: i32,
    pub n: i32,
    pub lower: i32,
    pub upper: i32,
    columns: usize,
    states: usize,
    depth: usize,
}
impl Dimensions {
    pub(super) fn new(m: i32, n: i32) -> Result<Self, String> {
        if m < 0 || n < 0 {
            return Err("Ansari sample sizes must be non-negative".into());
        }
        let total = m
            .checked_add(n)
            .and_then(|v| v.checked_add(1))
            .ok_or_else(memory_error)?;
        let (lower, upper) = support(m, n);
        if upper >= i64::from(i32::MAX) {
            return Err(memory_error());
        }
        let columns = (n as usize).checked_add(1).ok_or_else(memory_error)?;
        let states = (m as usize)
            .checked_add(1)
            .and_then(|v| v.checked_mul(columns))
            .ok_or_else(memory_error)?;
        Ok(Self {
            m,
            n,
            lower: lower as i32,
            upper: upper as i32,
            columns,
            states,
            depth: if m == 0 || n == 0 { 1 } else { total as usize },
        })
    }
}
fn support(m: i32, n: i32) -> (i64, i64) {
    let m = i64::from(m);
    let n = i64::from(n);
    let l = (m + 1) * (m + 1) / 4;
    (l, l + m * n / 2)
}
struct Growth<'a> {
    reservation: &'a mut WorkspaceReservation,
    previous: usize,
    committed: bool,
}
impl Drop for Growth<'_> {
    fn drop(&mut self) {
        if !self.committed {
            let _ = self.reservation.try_resize(self.previous);
        }
    }
}
fn allocate<T>(
    owner: OwnerToken<'_>,
    reservation: &mut WorkspaceReservation,
    length: usize,
    init: impl FnMut() -> T,
) -> Result<Vec<T>, String> {
    allocate_with(owner, reservation, length, init, |n| {
        let mut v = Vec::new();
        v.try_reserve_exact(n).map_err(|_| memory_error())?;
        Ok(v)
    })
}
fn allocate_with<T>(
    owner: OwnerToken<'_>,
    reservation: &mut WorkspaceReservation,
    length: usize,
    init: impl FnMut() -> T,
    allocator: impl FnOnce(usize) -> Result<Vec<T>, String>,
) -> Result<Vec<T>, String> {
    owner.require_active().map_err(|e| e.to_string())?;
    let bytes = length
        .checked_mul(size_of::<T>())
        .filter(|&n| n <= isize::MAX as usize)
        .ok_or_else(memory_error)?;
    let previous = reservation.bytes();
    reservation
        .try_resize(previous.checked_add(bytes).ok_or_else(memory_error)?)
        .ok_or_else(memory_error)?;
    let mut growth = Growth {
        reservation,
        previous,
        committed: false,
    };
    let mut values = allocator(length)?;
    let actual = values
        .capacity()
        .checked_mul(size_of::<T>())
        .and_then(|v| previous.checked_add(v));
    if values.capacity() < length
        || actual
            .and_then(|v| growth.reservation.try_resize(v))
            .is_none()
    {
        drop(values);
        return Err(memory_error());
    }
    owner.require_active().map_err(|e| e.to_string())?;
    values.resize_with(length, init);
    growth.committed = true;
    Ok(values)
}
#[derive(Clone, Copy)]
struct Frame {
    k: i32,
    m: i32,
    n: i32,
    phase: u8,
}
pub(super) struct MemoTable {
    // Storage fields drop before their shared credit. No existing buffer grows.
    slots: Vec<Option<Vec<f64>>>,
    frames: Vec<Frame>,
    reservation: WorkspaceReservation,
    owner: WeakOwner,
    dimensions: Dimensions,
    normalizer: f64,
}
impl MemoTable {
    pub(super) fn new(owner: OwnerToken<'_>, m: i32, n: i32) -> Result<Self, String> {
        let dimensions = Dimensions::new(m, n)?;
        owner.require_active().map_err(|e| e.to_string())?;
        let weak = owner
            .weak_owner()
            .ok_or_else(|| "Ansari owner is unavailable".to_string())?;
        let mut reservation = owner
            .with_arena(|a| a.reserve_workspace(0))
            .map_err(|e| e.to_string())?
            .ok_or_else(memory_error)?;
        let slots = allocate(owner, &mut reservation, dimensions.states, || None)?;
        let mut frames = allocate(owner, &mut reservation, dimensions.depth, || Frame {
            k: 0,
            m: 0,
            n: 0,
            phase: 0,
        })?;
        frames.clear();
        let normalizer =
            crate::nmath::special::choose::choose(f64::from(m) + f64::from(n), f64::from(m));
        owner.require_active().map_err(|e| e.to_string())?;
        Ok(Self {
            slots,
            frames,
            reservation,
            owner: weak,
            dimensions,
            normalizer,
        })
    }
    fn check_owner(&self, owner: OwnerToken<'_>) -> Result<(), String> {
        owner.require_active().map_err(|e| e.to_string())?;
        if !self.owner.is_live()
            || !owner
                .weak_owner()
                .is_some_and(|v| v.same_owner(&self.owner))
        {
            return Err("Ansari owner is unavailable".into());
        }
        Ok(())
    }
    fn index(&self, m: i32, n: i32) -> usize {
        m as usize * self.dimensions.columns + n as usize
    }
    fn cached(&self, k: i32, m: i32, n: i32) -> Option<f64> {
        let (l, u) = support(m, n);
        if i64::from(k) < l || i64::from(k) > u {
            return Some(0.0);
        }
        self.slots[self.index(m, n)].as_ref().and_then(|v| {
            let x = v[k as usize];
            (x >= 0.0).then_some(x)
        })
    }
    fn ensure(&mut self, owner: OwnerToken<'_>, m: i32, n: i32) -> Result<(), String> {
        let index = self.index(m, n);
        if self.slots[index].is_none() {
            self.check_owner(owner)?;
            let (_, u) = support(m, n);
            let len = usize::try_from(u)
                .ok()
                .and_then(|v| v.checked_add(1))
                .ok_or_else(memory_error)?;
            let values = allocate(owner, &mut self.reservation, len, || -1.0)?;
            self.slots[index] = Some(values);
        }
        Ok(())
    }
    fn push(&mut self, k: i32, m: i32, n: i32) -> Result<(), String> {
        if self.frames.len() >= self.dimensions.depth {
            return Err(memory_error());
        }
        self.frames.push(Frame { k, m, n, phase: 0 });
        Ok(())
    }
    fn count(&mut self, owner: OwnerToken<'_>, k: i32, m: i32, n: i32) -> Result<f64, String> {
        self.frames.clear();
        self.push(k, m, n)?;
        while let Some(f) = self.frames.last().copied() {
            if self.cached(f.k, f.m, f.n).is_some() {
                self.frames.pop();
                continue;
            }
            self.ensure(owner, f.m, f.n)?;
            let index = self.index(f.m, f.n);
            if f.m == 0 || f.n == 0 {
                self.slots[index].as_mut().unwrap()[f.k as usize] = 1.0;
                self.frames.pop();
                continue;
            }
            let score = (f.m + f.n + 1) / 2;
            match f.phase {
                0 => {
                    self.frames.last_mut().unwrap().phase = 1;
                    self.push(f.k, f.m, f.n - 1)?;
                }
                1 => {
                    self.frames.last_mut().unwrap().phase = 2;
                    self.push(f.k - score, f.m - 1, f.n)?;
                }
                _ => {
                    let left = self.cached(f.k, f.m, f.n - 1).unwrap();
                    let right = self.cached(f.k - score, f.m - 1, f.n).unwrap();
                    self.slots[index].as_mut().unwrap()[f.k as usize] = left + right;
                    self.frames.pop();
                }
            }
        }
        Ok(self.cached(k, m, n).unwrap())
    }
    pub(super) fn cdf(&mut self, owner: OwnerToken<'_>, q: f64) -> Result<f64, String> {
        self.check_owner(owner)?;
        let q = (q + 1e-7).floor();
        let d = self.dimensions;
        if q < f64::from(d.lower) || q.is_nan() {
            return Ok(0.0);
        }
        if q > f64::from(d.upper) {
            return Ok(1.0);
        }
        let mut sum = 0.0;
        for j in d.lower..=q as i32 {
            sum += self.count(owner, j, d.m, d.n)?;
        }
        Ok(sum / self.normalizer)
    }
    pub(super) fn quantile(&mut self, owner: OwnerToken<'_>, p: f64) -> Result<f64, String> {
        self.check_owner(owner)?;
        if !p.is_finite() || !(0.0..=1.0).contains(&p) {
            return Err("probabilities outside [0,1] in qansari()".into());
        }
        let d = self.dimensions;
        if p == 0.0 {
            return Ok(f64::from(d.lower));
        }
        if p == 1.0 {
            return Ok(f64::from(d.upper));
        }
        let mut sum = 0.0;
        for q in d.lower..=d.upper {
            sum += self.count(owner, q, d.m, d.n)? / self.normalizer;
            if sum >= p {
                return Ok(f64::from(q));
            }
        }
        Err("Ansari probability could not be resolved".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::{RSession, memory::ArenaBudget};
    fn charge(m: &MemoTable) -> usize {
        m.slots.capacity() * size_of::<Option<Vec<f64>>>()
            + m.frames.capacity() * size_of::<Frame>()
            + m.slots
                .iter()
                .flatten()
                .map(|v| v.capacity() * size_of::<f64>())
                .sum::<usize>()
    }
    #[test]
    fn dimensions_and_count_bytes_are_checked() {
        for (m, n) in [
            (-1, 2),
            (2, -1),
            (i32::MAX, 0),
            (0, i32::MAX),
            (100_000, 100_000),
        ] {
            assert!(Dimensions::new(m, n).is_err());
        }
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let mut reservation = owner
            .with_arena(|a| a.reserve_workspace(0))
            .unwrap()
            .unwrap();
        assert!(
            allocate_with::<f64>(
                owner,
                &mut reservation,
                usize::MAX,
                || 0.0,
                |_| panic!("unchecked allocation")
            )
            .is_err()
        );
        assert_eq!(reservation.bytes(), 0);
    }
    #[test]
    fn new_buffers_overlap_existing_capacity_and_failures_roll_back() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let mut reservation = owner
            .with_arena(|a| a.reserve_workspace(0))
            .unwrap()
            .unwrap();
        let old = allocate_with(
            owner,
            &mut reservation,
            3,
            || 0.0,
            |n| {
                let mut v = Vec::new();
                v.try_reserve_exact(n * 2).map_err(|_| memory_error())?;
                Ok(v)
            },
        )
        .unwrap();
        let old_bytes = old.capacity() * size_of::<f64>();
        assert_eq!(reservation.bytes(), old_bytes);
        let allocated = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(allocated + old_bytes, 0)))
            .unwrap();
        assert!(
            allocate_with::<f64>(
                owner,
                &mut reservation,
                1,
                || 0.0,
                |_| panic!("allocation before admission")
            )
            .is_err()
        );
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(allocated + old_bytes + 24, 0)))
            .unwrap();
        assert!(
            allocate_with(
                owner,
                &mut reservation,
                3,
                || 0.0,
                |n| {
                    let mut v = Vec::new();
                    v.try_reserve_exact(n * 2).map_err(|_| memory_error())?;
                    Ok(v)
                }
            )
            .is_err()
        );
        assert_eq!(reservation.bytes(), old_bytes);
        assert!(
            allocate_with::<f64>(owner, &mut reservation, 3, || 0.0, |_| Err(memory_error()))
                .is_err()
        );
        assert_eq!(reservation.bytes(), old_bytes);
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = allocate_with::<f64>(
                    owner,
                    &mut reservation,
                    3,
                    || 0.0,
                    |_| panic!("allocator unwind"),
                );
            }))
            .is_err()
        );
        assert_eq!(reservation.bytes(), old_bytes);
        drop(old);
        drop(reservation);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    }
    #[test]
    fn storage_drops_before_initializer_unwind_returns_credit() {
        struct Spy<'s>(OwnerToken<'s>);
        impl Drop for Spy<'_> {
            fn drop(&mut self) {
                assert!(
                    self.0.with_arena(|a| a.workspace_bytes()).unwrap() >= 3 * size_of::<Self>()
                );
            }
        }
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let mut reservation = owner
            .with_arena(|a| a.reserve_workspace(0))
            .unwrap()
            .unwrap();
        let mut count = 0;
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = allocate(owner, &mut reservation, 3, || {
                    count += 1;
                    if count == 2 {
                        panic!("initializer unwind");
                    }
                    Spy(owner)
                });
            }))
            .is_err()
        );
        assert_eq!(count, 2);
        assert_eq!(reservation.bytes(), 0);
    }
    #[test]
    fn lazy_growth_denial_keeps_live_counts_charged_and_retry_recovers() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let mut memo = MemoTable::new(owner, 4, 6).unwrap();
        let base = charge(&memo);
        assert_eq!(memo.reservation.bytes(), base);
        let allocated = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::new(allocated + base + 19 * 8, 0)))
            .unwrap();
        assert!(memo.cdf(owner, 6.0).is_err());
        assert!(memo.slots[memo.index(4, 6)].is_some());
        assert_eq!(memo.reservation.bytes(), charge(&memo));
        assert!(charge(&memo) > base);
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
            .unwrap();
        assert!((memo.cdf(owner, 6.0).unwrap() - 1.0 / 210.0).abs() < 1e-14);
        assert_eq!(memo.quantile(owner, 0.5).unwrap(), 12.0);
        assert_eq!(memo.reservation.bytes(), charge(&memo));
        drop(memo);
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
    }
    #[test]
    fn partial_constructor_denial_releases_metadata() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let allocated = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        owner
            .with_arena(|a| {
                a.set_budget(ArenaBudget::new(
                    allocated + 35 * size_of::<Option<Vec<f64>>>(),
                    0,
                ))
            })
            .unwrap();
        assert!(MemoTable::new(owner, 4, 6).is_err());
        assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), 0);
        owner
            .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
            .unwrap();
        let mut memo = MemoTable::new(owner, 4, 6).unwrap();
        assert_eq!(memo.quantile(owner, 0.0).unwrap(), 6.0);
    }
    #[test]
    fn teardown_and_replacement_cannot_redirect_original_credit() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let anchor = owner
            .node_factory()
            .character("anchor")
            .unwrap()
            .into_owned()
            .unwrap();
        let ledger = anchor.allocation().unwrap().heap_identity().byte_ledger();
        let mut memo = MemoTable::new(owner, 4, 6).unwrap();
        memo.cdf(owner, 7.0).unwrap();
        let bytes = charge(&memo);
        drop(anchor);
        drop(session);
        let other = RSession::new_for_gc_tests();
        let owner = other.owner_token().unwrap();
        let baseline = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
        assert!(memo.cdf(owner, 8.0).is_err());
        assert_eq!(ledger.workspace_bytes_for_test(), bytes);
        drop(memo);
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
        assert_eq!(
            owner.with_arena(|a| a.total_bytes_allocated()).unwrap(),
            baseline
        );
    }
    #[test]
    fn revocation_during_new_allocation_rolls_back_original_credit() {
        let mut session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let weak = owner.weak_owner().unwrap();
        let anchor = owner
            .node_factory()
            .character("anchor")
            .unwrap()
            .into_owned()
            .unwrap();
        let ledger = anchor.allocation().unwrap().heap_identity().byte_ledger();
        let result = crate::sexp::owner::with_runtime(&weak, |access| {
            access.with_native(|owner| {
                let mut reservation = owner.with_arena(|a| a.reserve_workspace(0))?.unwrap();
                Ok(allocate_with(
                    owner,
                    &mut reservation,
                    3,
                    || 0.0,
                    |n| {
                        session.close();
                        let mut v = Vec::new();
                        v.try_reserve_exact(n).map_err(|_| memory_error())?;
                        Ok(v)
                    },
                ))
            })
        });
        assert!(matches!(
            result,
            Err(crate::sexp::SexpError::RootUnavailable)
        ));
        assert_eq!(ledger.workspace_bytes_for_test(), 0);
    }
}
