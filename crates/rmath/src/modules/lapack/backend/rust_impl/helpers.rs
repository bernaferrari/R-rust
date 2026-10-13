use super::*;

/// Reserve the temporary buffers used by a numerical kernel when an R
/// session is active. Standalone low-level LAPACK tests intentionally run
/// without an ambient session and therefore retain the historical unlimited
/// behavior.
pub(crate) unsafe fn reserve_native_workspace(
    bytes: usize,
) -> Result<Option<TransientReservation>, ()> {
    match with_current_instance(|instance| unsafe {
        with_arena_in(instance, |arena| arena.try_reserve_transient(bytes))
    }) {
        None => Ok(None),
        Some(Some(reservation)) => Ok(Some(reservation)),
        Some(None) => Err(()),
    }
}

/// Read a column-major matrix (with optional lda stride) into a faer Mat.
pub(crate) unsafe fn read_mat_f64(ptr: *const f64, m: usize, n: usize, lda: usize) -> Mat<f64> {
    unsafe {
        let mut mat = Mat::zeros(m, n);
        for j in 0..n {
            for i in 0..m {
                mat[(i, j)] = *ptr.add(i + j * lda);
            }
        }
        mat
    }
}

/// Write real matrix data to column-major pointer (MatRef).
pub(crate) unsafe fn write_matref_f64(
    mat: MatRef<'_, f64>,
    ptr: *mut f64,
    m: usize,
    n: usize,
    lda: usize,
) {
    unsafe {
        for j in 0..n {
            for i in 0..m {
                *ptr.add(i + j * lda) = mat[(i, j)];
            }
        }
    }
}

/// Write real matrix (owned) to column-major pointer.
pub(crate) unsafe fn write_owned_f64(
    mat: &Mat<f64>,
    ptr: *mut f64,
    m: usize,
    n: usize,
    lda: usize,
) {
    unsafe {
        for j in 0..n {
            for i in 0..m {
                *ptr.add(i + j * lda) = mat[(i, j)];
            }
        }
    }
}

/// Write complex matrix data to column-major Rcomplex pointer (MatRef).
pub(crate) unsafe fn write_matref_c64(
    mat: MatRef<'_, c64>,
    ptr: *mut Rcomplex,
    m: usize,
    n: usize,
    lda: usize,
) {
    unsafe {
        for j in 0..n {
            for i in 0..m {
                let c = mat[(i, j)];
                *ptr.add(i + j * lda) = Rcomplex { r: c.re, i: c.im };
            }
        }
    }
}

/// Write complex matrix (owned) to column-major Rcomplex pointer.
pub(crate) unsafe fn write_owned_c64(
    mat: &Mat<c64>,
    ptr: *mut Rcomplex,
    m: usize,
    n: usize,
    lda: usize,
) {
    unsafe {
        for j in 0..n {
            for i in 0..m {
                let c = mat[(i, j)];
                *ptr.add(i + j * lda) = Rcomplex { r: c.re, i: c.im };
            }
        }
    }
}

/// Read a column-major complex matrix into a faer Mat<c64>.
pub(crate) unsafe fn read_mat_c64(
    ptr: *const Rcomplex,
    m: usize,
    n: usize,
    lda: usize,
) -> Mat<c64> {
    unsafe {
        let mut mat = Mat::zeros(m, n);
        for j in 0..n {
            for i in 0..m {
                let rc = *ptr.add(i + j * lda);
                mat[(i, j)] = c64::new(rc.r, rc.i);
            }
        }
        mat
    }
}

/// Extract the backward permutation from a PermRef as a Vec<usize>.
pub(crate) fn get_bwd_perm(p: faer::perm::PermRef<'_, usize>) -> Vec<usize> {
    let (_fwd, bwd) = p.arrays();
    bwd.iter().copied().collect()
}

/// Convert a faer permutation (backward map: permuted → original) to LAPACK's 1-based ipiv.
pub(crate) fn perm_bwd_to_ipiv(bwd: &[usize], n: usize) -> Vec<i32> {
    let mut current: Vec<usize> = (0..n).collect();
    let mut inv: Vec<usize> = (0..n).collect();
    let mut ipiv = vec![0i32; n];
    for k in 0..n {
        let target = bwd[k];
        let pos = inv[target];
        if pos != k {
            let other = current[k];
            current[k] = target;
            current[pos] = other;
            inv[target] = k;
            inv[other] = pos;
        }
        ipiv[k] = (pos + 1) as i32;
    }
    ipiv
}

// ============================================================
// Real LAPACK routines (d-prefixed)
// ============================================================
