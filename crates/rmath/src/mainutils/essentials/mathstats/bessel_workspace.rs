//! Private Bessel work array admitted to the original runtime's byte ledger.
#![forbid(unsafe_code)]
use crate::sexp::{
    memory::WorkspaceReservation,
    object::{SexpError, SexpResult},
    owner::RuntimeAccess,
};
pub(crate) struct Workspace {
    // Storage must disappear before its original-owner credit is released.
    values: Vec<f64>,
    _reservation: WorkspaceReservation,
}
fn failure() -> SexpError {
    SexpError::EvaluationFailed {
        message: "Bessel workspace exceeds session allocation limit".into(),
    }
}
impl Workspace {
    pub(crate) fn new(access: &RuntimeAccess, length: usize) -> SexpResult<Self> {
        let bytes = length
            .checked_mul(size_of::<f64>())
            .filter(|&n| n <= isize::MAX as usize)
            .ok_or_else(failure)?;
        let mut reservation = access
            .with_arena(|a| a.reserve_workspace(bytes))?
            .ok_or_else(failure)?;
        let mut values = Vec::new();
        values.try_reserve_exact(length).map_err(|_| failure())?;
        if values.capacity() < length
            || values
                .capacity()
                .checked_mul(size_of::<f64>())
                .and_then(|n| reservation.try_resize(n))
                .is_none()
        {
            drop(values);
            return Err(failure());
        }
        access.require_active()?;
        values.resize(length, 0.0);
        Ok(Self {
            values,
            _reservation: reservation,
        })
    }
    pub(crate) fn values(&mut self) -> &mut [f64] {
        &mut self.values
    }
}

// Common vector workloads need few expansions. Keep their descriptors inline;
// any overflow descriptor allocation has its own original-owner reservation.
const INLINE_SEGMENTS: usize = 8;

struct Segment {
    start: usize,
    work: Workspace,
}

pub(crate) struct SegmentedWorkspace {
    inline: [Option<Segment>; INLINE_SEGMENTS],
    tail: Vec<Segment>,
    // The Vec storage must be dropped before its reservation.
    tail_reservation: Option<WorkspaceReservation>,
    length: usize,
}

impl SegmentedWorkspace {
    pub(crate) fn new(access: &RuntimeAccess, length: usize) -> SexpResult<Self> {
        let mut inline = std::array::from_fn(|_| None);
        inline[0] = Some(Segment {
            start: 0,
            work: Workspace::new(access, length)?,
        });
        Ok(Self {
            inline,
            tail: Vec::new(),
            tail_reservation: None,
            length,
        })
    }

    pub(crate) fn ensure_len(&mut self, access: &RuntimeAccess, length: usize) -> SexpResult<()> {
        if length <= self.length {
            return Ok(());
        }
        if self.length == 0 {
            self.inline[0] = Some(Segment {
                start: 0,
                work: Workspace::new(access, length)?,
            });
            self.length = length;
            return Ok(());
        }
        let extra = length.checked_sub(self.length).ok_or_else(failure)?;
        let segment = Segment {
            start: self.length,
            work: Workspace::new(access, extra)?,
        };
        if let Some(slot) = self.inline.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(segment);
        } else {
            self.reserve_tail(access)?;
            self.tail.push(segment);
        }
        self.length = length;
        Ok(())
    }

    pub(crate) fn contiguous_mut(&mut self) -> Option<&mut [f64]> {
        if self.inline[1].is_none() {
            self.inline[0].as_mut().map(|segment| segment.work.values())
        } else {
            None
        }
    }

    fn reserve_tail(&mut self, access: &RuntimeAccess) -> SexpResult<()> {
        if self.tail.len() < self.tail.capacity() {
            return Ok(());
        }
        let capacity = self
            .tail
            .capacity()
            .max(2)
            .checked_mul(2)
            .ok_or_else(failure)?;
        let bytes = capacity
            .checked_mul(size_of::<Segment>())
            .filter(|&n| n <= isize::MAX as usize)
            .ok_or_else(failure)?;
        let mut reservation = access
            .with_arena(|a| a.reserve_workspace(bytes))?
            .ok_or_else(failure)?;
        let mut replacement = Vec::new();
        replacement
            .try_reserve_exact(capacity)
            .map_err(|_| failure())?;
        if replacement
            .capacity()
            .checked_mul(size_of::<Segment>())
            .and_then(|n| reservation.try_resize(n))
            .is_none()
        {
            drop(replacement);
            return Err(failure());
        }
        access.require_active()?;
        replacement.append(&mut self.tail);
        let old = std::mem::replace(&mut self.tail, replacement);
        drop(old);
        self.tail_reservation = Some(reservation);
        Ok(())
    }

    fn segment(&self, index: usize) -> &Segment {
        assert!(index < self.length, "Bessel workspace index out of bounds");
        for segment in self.inline.iter().flatten() {
            if index >= segment.start && index - segment.start < segment.work.values.len() {
                return segment;
            }
        }
        let position = self.tail.partition_point(|segment| segment.start <= index) - 1;
        &self.tail[position]
    }

    fn segment_mut(&mut self, index: usize) -> &mut Segment {
        assert!(index < self.length, "Bessel workspace index out of bounds");
        for segment in self.inline.iter_mut().flatten() {
            if index >= segment.start && index - segment.start < segment.work.values.len() {
                return segment;
            }
        }
        let position = self.tail.partition_point(|segment| segment.start <= index) - 1;
        &mut self.tail[position]
    }
}

impl std::ops::Index<usize> for SegmentedWorkspace {
    type Output = f64;
    fn index(&self, index: usize) -> &Self::Output {
        let segment = self.segment(index);
        &segment.work.values[index - segment.start]
    }
}
impl std::ops::IndexMut<usize> for SegmentedWorkspace {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        let segment = self.segment_mut(index);
        &mut segment.work.values[index - segment.start]
    }
}
impl crate::nmath::special::bessel::BesselWorkspace for SegmentedWorkspace {
    fn len(&self) -> usize {
        self.length
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::{RSession, memory::ArenaBudget, owner::with_runtime};
    #[test]
    fn segmented_kernels_match_contiguous_values_and_status() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let weak = owner.weak_owner().unwrap();
        session.with_active(|| {
            with_runtime(&weak, |access| {
                let mut segmented = SegmentedWorkspace::new(access, 0)?;
                for length in [1, 2, 4, 8, 16, 32, 64, 128, 256, 501] {
                    segmented.ensure_len(access, length)?;
                }
                for x in [0.0, 1e-300, 1.0, 25.0, f64::INFINITY] {
                    for order in [0.0_f64, 0.25, 2.0, 30.0, 500.25] {
                        for scale in [1.0, 2.0, 3.0] {
                            let length = order.floor() as usize + 1;
                            let mut contiguous = vec![0.0; length];
                            for i in 0..segmented.length {
                                segmented[i] = 0.0;
                            }
                            let a = crate::special::bessel_i::bessel_i_positive_status(
                                x,
                                order,
                                scale,
                                &mut contiguous,
                            )
                            .unwrap();
                            let b =
                                crate::special::bessel_i::bessel_i_positive_status_with_workspace(
                                    x,
                                    order,
                                    scale,
                                    &mut segmented,
                                )
                                .unwrap();
                            assert_eq!(
                                (a.value.to_bits(), a.ncalc, a.count),
                                (b.value.to_bits(), b.ncalc, b.count)
                            );
                            let mut contiguous = vec![0.0; length];
                            for i in 0..segmented.length {
                                segmented[i] = 0.0;
                            }
                            let a = crate::special::bessel_k::bessel_k_positive_status(
                                x,
                                order,
                                scale,
                                &mut contiguous,
                            )
                            .unwrap();
                            let b =
                                crate::special::bessel_k::bessel_k_positive_status_with_workspace(
                                    x,
                                    order,
                                    scale,
                                    &mut segmented,
                                )
                                .unwrap();
                            assert_eq!(
                                (a.value.to_bits(), a.ncalc, a.count),
                                (b.value.to_bits(), b.ncalc, b.count)
                            );
                        }
                    }
                }
                Ok::<(), SexpError>(())
            })
            .unwrap()
            .unwrap()
        });
    }

    #[test]
    fn segmented_growth_preserves_values_under_final_size_budget() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let weak = owner.weak_owner().unwrap();
        session.with_active(|| {
            with_runtime(&weak, |access| {
                let allocated = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
                let baseline = owner.with_arena(|a| a.workspace_bytes()).unwrap();
                let mut work = SegmentedWorkspace::new(access, 300).unwrap();
                for i in 0..300 {
                    work[i] = i as f64;
                }
                owner
                    .with_arena(|a| {
                        a.set_budget(ArenaBudget::new(
                            allocated + baseline + 501 * size_of::<f64>(),
                            0,
                        ))
                    })
                    .unwrap();
                work.ensure_len(access, 501).unwrap();
                for i in 0..300 {
                    assert_eq!(work[i], i as f64);
                }
                for i in 300..501 {
                    assert_eq!(work[i], 0.0);
                }
                let charged = owner.with_arena(|a| a.workspace_bytes()).unwrap();
                assert_eq!(charged, baseline + 501 * size_of::<f64>());
                assert!(work.ensure_len(access, 502).is_err());
                assert_eq!(work.length, 501);
                assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), charged);
                assert_eq!(work[299], 299.0);
                owner
                    .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
                    .unwrap();
                for length in 502..520 {
                    work.ensure_len(access, length).unwrap();
                    work[length - 1] = length as f64;
                }
                for length in 502..520 {
                    assert_eq!(work[length - 1], length as f64);
                }
                assert!(work.tail.len() > 0);
                assert!(work.tail_reservation.is_some());
                drop(work);
                assert_eq!(owner.with_arena(|a| a.workspace_bytes()).unwrap(), baseline);
                Ok::<(), SexpError>(())
            })
            .unwrap()
            .unwrap()
        });
    }

    #[test]
    fn bessel_workspace_admission_denial_overflow_and_recovery() {
        let session = RSession::new_for_gc_tests();
        let owner = session.owner_token().unwrap();
        let weak = owner.weak_owner().unwrap();
        session.with_active(|| {
            with_runtime(&weak, |access| {
                let before = owner.with_arena(|a| a.total_bytes_allocated()).unwrap();
                let before_workspace = owner.with_arena(|a| a.workspace_bytes()).unwrap();
                assert!(Workspace::new(access, usize::MAX).is_err());
                owner
                    .with_arena(|a| {
                        a.set_budget(ArenaBudget::new(before + before_workspace + 7, 0))
                    })
                    .unwrap();
                assert!(Workspace::new(access, 1).is_err());
                assert_eq!(
                    owner.with_arena(|a| a.total_bytes_allocated()).unwrap(),
                    before
                );
                owner
                    .with_arena(|a| a.set_budget(ArenaBudget::unlimited()))
                    .unwrap();
                let mut values = Workspace::new(access, 3).unwrap();
                assert_eq!(values.values(), [0.0; 3]);
                assert!(
                    owner.with_arena(|a| a.workspace_bytes()).unwrap() >= before_workspace + 24
                );
                drop(values);
                assert_eq!(
                    owner.with_arena(|a| a.workspace_bytes()).unwrap(),
                    before_workspace
                );
                assert_eq!(
                    owner.with_arena(|a| a.total_bytes_allocated()).unwrap(),
                    before
                );
            })
            .unwrap()
        });
    }
}
