//! Admission sizes for native QR and SVD scratch, before any buffer is allocated.
//! Overflow is `None`. The callers turn that into the historical resource-limit error.

use std::ffi::c_int;

pub(crate) fn qr_scratch_bytes(m: usize, n: usize) -> Option<usize> {
    let len = m.checked_mul(n)?;
    let min_mn = m.min(n);
    len.checked_mul(size_of::<f64>())?
        .checked_add(n.checked_mul(size_of::<c_int>())?)?
        .checked_add(min_mn.checked_mul(size_of::<f64>())?)
}

pub(crate) fn svd_scratch_bytes(len: usize, min_np: usize) -> Option<usize> {
    let iwork_len = min_np.checked_mul(8)?;
    len.checked_mul(size_of::<f64>())?
        .checked_add(iwork_len.checked_mul(size_of::<c_int>())?)
}

pub(crate) fn complex_svd_scratch_bytes(
    len: usize,
    min_np: usize,
    complex_bytes: usize,
) -> Option<usize> {
    let iwork_len = min_np.checked_mul(8)?;
    len.checked_mul(complex_bytes)?
        .checked_add(min_np.checked_mul(size_of::<f64>())?)?
        .checked_add(iwork_len.checked_mul(size_of::<c_int>())?)
}

#[cfg(kani)]
mod kani_proofs {
    use super::{qr_scratch_bytes, svd_scratch_bytes};

    fn fits_u128(value: Option<usize>) -> bool {
        value.is_some()
    }

    #[kani::proof]
    fn qr_scratch_rejects_overflow() {
        let m: usize = kani::any();
        let n: usize = kani::any();
        kani::assume(m <= 4 && n <= 4);
        let got = qr_scratch_bytes(m, n);
        let len = (m as u128) * (n as u128);
        let min_mn = m.min(n) as u128;
        let expect = len * 8 + (n as u128) * 4 + min_mn * 8;
        assert_eq!(got, Some(expect as usize));
        assert!(fits_u128(got));
        assert!(qr_scratch_bytes(usize::MAX, 2).is_none());
        assert!(qr_scratch_bytes(2, usize::MAX).is_none());
        kani::cover(m == 0 || n == 0, "empty");
        kani::cover(m == 4 && n == 4, "square");
    }

    #[kani::proof]
    fn svd_scratch_rejects_overflow() {
        let len: usize = kani::any();
        let min_np: usize = kani::any();
        kani::assume(len <= 8 && min_np <= 4);
        let got = svd_scratch_bytes(len, min_np);
        let expect = (len as u128) * 8 + (min_np as u128) * 8 * 4;
        assert_eq!(got, Some(expect as usize));
        assert!(svd_scratch_bytes(usize::MAX, 1).is_none());
        assert!(svd_scratch_bytes(1, usize::MAX).is_none());
        kani::cover(len == 0, "empty");
        kani::cover(got.is_some(), "admits");
    }
}
