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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ComplexSvdScratch {
    pub rwork_len: usize,
    pub iwork_len: usize,
    pub bytes: usize,
}

/// Bytes and lengths for the complex SVD copy, `rwork`, and `iwork`.
///
/// `job_n` is LAPACK job `'N'`. `mn0` is `min(n, p)` and `mn1` is `max(n, p)`.
/// `rwork` is `7 * mn0` for job `'N'`, otherwise `mn0 * max(5 * mn1 + 7, 2 * mn1 + 2 * mn0 + 1)`.
/// `iwork` is eight times `rwork`. Overflow is `None`.
pub(crate) fn complex_svd_scratch(
    len: usize,
    job_n: bool,
    mn0: usize,
    mn1: usize,
    complex_bytes: usize,
) -> Option<ComplexSvdScratch> {
    let rwork_len = if job_n {
        mn0.checked_mul(7)?
    } else {
        let wide = mn1.checked_mul(5)?.checked_add(7)?;
        let square = mn1
            .checked_mul(2)?
            .checked_add(mn0.checked_mul(2)?)?
            .checked_add(1)?;
        mn0.checked_mul(wide.max(square))?
    };
    let iwork_len = rwork_len.checked_mul(8)?;
    let bytes = len
        .checked_mul(complex_bytes)?
        .checked_add(rwork_len.checked_mul(size_of::<f64>())?)?
        .checked_add(iwork_len.checked_mul(size_of::<c_int>())?)?;
    Some(ComplexSvdScratch {
        rwork_len,
        iwork_len,
        bytes,
    })
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

#[cfg(test)]
mod tests {
    use super::complex_svd_scratch;
    use std::ffi::c_int;

    #[test]
    fn complex_svd_counts_rwork_and_rejects_overflow() {
        let narrow = complex_svd_scratch(4, true, 2, usize::MAX, 16).unwrap();
        assert_eq!(narrow.rwork_len, 14);
        assert_eq!(narrow.iwork_len, 112);
        assert_eq!(
            narrow.bytes,
            4 * 16 + 14 * size_of::<f64>() + 112 * size_of::<c_int>()
        );

        let wide = complex_svd_scratch(6, false, 2, 3, 16).unwrap();
        assert_eq!(wide.rwork_len, 44);
        assert_eq!(wide.iwork_len, 352);
        assert_eq!(
            wide.bytes,
            6 * 16 + 44 * size_of::<f64>() + 352 * size_of::<c_int>()
        );

        assert!(complex_svd_scratch(8, false, 3, usize::MAX / 4, 16).is_none());
        assert!(complex_svd_scratch(usize::MAX, true, 1, 1, 16).is_none());
        assert!(complex_svd_scratch(1, true, usize::MAX / 6, 1, 16).is_none());
    }
}
