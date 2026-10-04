use core::ffi::{c_char, c_double, c_int};
use std::ptr;
use std::slice;

use crate::main::errors::Rf_error;

/*
 * Port of R's src/library/stats/src/mAR.c
 * Multivariate autoregression via Burg's algorithm (multi_burg)
 * and Whittle's algorithm (multi_yw).
 *
 * Original copyright:
 *   Copyright (C) 1999 Martyn Plummer
 *   Copyright (C) 1999-2016 The R Core Team
 *
 * QR solve implemented via LINPACK dqrdc2/dqrsl.
 */

const MAX_DIM_LENGTH: usize = 4;

pub(crate) mod multi_burg;

/*
 * Array structure: up to 4D arrays with row-major layout.
 *
 * In C, the struct held double-star / double-star-star / etc. pointers into
 * a contiguous backing vector allocated via R_alloc. In Rust we store the
 * flat Vec<f64> and compute index offsets for matrix access.
 */
struct Array {
    /// Flat backing storage for all elements
    vec: Vec<f64>,
    /// Dimensions, up to MAX_DIM_LENGTH
    dim: [i32; MAX_DIM_LENGTH],
    /// Number of active dimensions (1..4)
    ndim: i32,
}

impl Array {
    /// Access element (i, j) of a 2D array (matrix)
    fn mat_get(&self, i: usize, j: usize) -> f64 {
        let ncol = self.dim[1] as usize;
        self.vec[i * ncol + j]
    }

    /// Set element (i, j) of a 2D array (matrix)
    fn mat_set(&mut self, i: usize, j: usize, val: f64) {
        let ncol = self.dim[1] as usize;
        self.vec[i * ncol + j] = val;
    }

    /// NROW
    fn nrow(&self) -> usize {
        self.dim[0] as usize
    }

    /// NCOL
    fn ncol(&self) -> usize {
        self.dim[1] as usize
    }

    /// Total number of elements (product of all dimensions)
    fn vector_length(&self) -> usize {
        let mut len: usize = 1;
        for i in 0..self.ndim as usize {
            len *= self.dim[i] as usize;
        }
        len
    }
}

fn array_assert(cond: bool) {
    if !cond {
        unsafe { Rf_error(b"assert failed in src/library/stats/mar.rs\0".as_ptr() as *const _) };
    }
}

fn test_array_conform(a1: &Array, a2: &Array) -> bool {
    if a1.ndim != a2.ndim {
        return false;
    }
    for i in 0..a1.ndim as usize {
        if a1.dim[i] != a2.dim[i] {
            return false;
        }
    }
    true
}

/// Create an Array from an existing data slice with given dimensions.
fn make_array(data: &[f64], dim: &[i32], ndim: i32) -> Array {
    array_assert((ndim as usize) <= MAX_DIM_LENGTH);

    let mut len: usize = 1;
    for i in 0..ndim as usize {
        len *= dim[i] as usize;
    }

    let mut a = Array {
        vec: data[..len].to_vec(),
        dim: [0; MAX_DIM_LENGTH],
        ndim,
    };
    for i in 0..ndim as usize {
        a.dim[i] = dim[i];
    }
    a
}

/// Create a zero-initialized array with given dimensions.
fn make_zero_array(dim: &[i32], ndim: i32) -> Array {
    let mut len: usize = 1;
    for i in 0..ndim as usize {
        len *= dim[i] as usize;
    }

    let mut a = Array {
        vec: vec![0.0; len],
        dim: [0; MAX_DIM_LENGTH],
        ndim,
    };
    for i in 0..ndim as usize {
        a.dim[i] = dim[i];
    }
    a
}

/// Create a matrix (2D array) from a flat slice.
fn make_matrix(data: &[f64], nrow: usize, ncol: usize) -> Array {
    make_array(data, &[nrow as i32, ncol as i32], 2)
}

/// Create a zero-initialized matrix.
fn make_zero_matrix(nrow: usize, ncol: usize) -> Array {
    make_zero_array(&[nrow as i32, ncol as i32], 2)
}

/// Create an identity matrix of size n.
fn make_identity_matrix(n: usize) -> Array {
    let mut a = make_zero_matrix(n, n);
    for i in 0..n {
        a.mat_set(i, i, 1.0);
    }
    a
}

/// Return a subarray of `a` at the given first-dimension index.
/// The data is copied (not shared).
fn subarray(a: &Array, index: usize) -> Array {
    array_assert(index < a.dim[0] as usize);

    let new_ndim = a.ndim - 1;
    let mut new_dim = [0i32; MAX_DIM_LENGTH];
    for i in 0..new_ndim as usize {
        new_dim[i] = a.dim[i + 1];
    }

    // Calculate the offset into the flat vector for this subarray.
    // This mirrors the C fall-through switch logic.
    let mut offset = index;
    let ndim = a.ndim as usize;
    if ndim >= 4 {
        offset *= a.dim[ndim - 4 + 1] as usize;
    }
    if ndim >= 3 {
        offset *= a.dim[ndim - 3 + 1] as usize;
    }
    if ndim >= 2 {
        offset *= a.dim[ndim - 2 + 1] as usize;
    }

    // Calculate the length of the subarray
    let mut sub_len: usize = 1;
    for i in 0..new_ndim as usize {
        sub_len *= new_dim[i] as usize;
    }

    let sub_vec = a.vec[offset..offset + sub_len].to_vec();

    let mut b = Array {
        vec: sub_vec,
        dim: new_dim,
        ndim: new_ndim,
    };
    b
}

/// Copy all elements from orig to ans (must be conformant).
fn copy_array(orig: &Array, ans: &mut Array) {
    array_assert(test_array_conform(orig, ans));
    let len = orig.vector_length();
    for i in 0..len {
        ans.vec[i] = orig.vec[i];
    }
}

/// Set all elements of arr to zero.
fn set_array_to_zero(arr: &mut Array) {
    for v in arr.vec.iter_mut() {
        *v = 0.0;
    }
}

/// Element-wise array operations: '+', '-', '*', '/'
/// NOTE: arr1, arr2, and ans must all be distinct (no aliasing).
fn array_op(arr1: &Array, arr2: &Array, op: char, ans: &mut Array) {
    array_assert(test_array_conform(arr1, arr2));
    array_assert(test_array_conform(arr2, ans));
    let len = ans.vector_length();
    match op {
        '+' => {
            for i in 0..len {
                ans.vec[i] = arr1.vec[i] + arr2.vec[i];
            }
        }
        '-' => {
            for i in 0..len {
                ans.vec[i] = arr1.vec[i] - arr2.vec[i];
            }
        }
        '*' => {
            for i in 0..len {
                ans.vec[i] = arr1.vec[i] * arr2.vec[i];
            }
        }
        '/' => {
            for i in 0..len {
                ans.vec[i] = arr1.vec[i] / arr2.vec[i];
            }
        }
        _ => {
            unsafe { Rf_error(b"Unknown op in array_op\0".as_ptr() as *const _) };
        }
    }
}

/// In-place element-wise operation: ans[i] op= arr[i].
/// Used when C code aliased arr1/ans to the same array.
fn array_op_in_place(ans: &mut Array, arr: &Array, op: char) {
    array_assert(test_array_conform(arr, ans));
    let len = ans.vector_length();
    match op {
        '+' => {
            for i in 0..len {
                ans.vec[i] += arr.vec[i];
            }
        }
        '-' => {
            for i in 0..len {
                ans.vec[i] -= arr.vec[i];
            }
        }
        '*' => {
            for i in 0..len {
                ans.vec[i] *= arr.vec[i];
            }
        }
        '/' => {
            for i in 0..len {
                ans.vec[i] /= arr.vec[i];
            }
        }
        _ => {
            unsafe { Rf_error(b"Unknown op in array_op_in_place\0".as_ptr() as *const _) };
        }
    }
}

/// Element-wise scalar operations: '+', '-', '*', '/'
fn scalar_op(arr: &Array, s: f64, op: char, ans: &mut Array) {
    array_assert(test_array_conform(arr, ans));
    let len = ans.vector_length();
    match op {
        '+' => {
            for i in 0..len {
                ans.vec[i] = arr.vec[i] + s;
            }
        }
        '-' => {
            for i in 0..len {
                ans.vec[i] = arr.vec[i] - s;
            }
        }
        '*' => {
            for i in 0..len {
                ans.vec[i] = arr.vec[i] * s;
            }
        }
        '/' => {
            for i in 0..len {
                ans.vec[i] = arr.vec[i] / s;
            }
        }
        _ => {
            unsafe { Rf_error(b"Unknown op in scalar_op\0".as_ptr() as *const _) };
        }
    }
}

/// In-place scalar operation: arr[i] op= s.
fn scalar_op_in_place(arr: &mut Array, s: f64, op: char) {
    let len = arr.vector_length();
    match op {
        '+' => {
            for v in arr.vec.iter_mut() {
                *v += s;
            }
        }
        '-' => {
            for v in arr.vec.iter_mut() {
                *v -= s;
            }
        }
        '*' => {
            for v in arr.vec.iter_mut() {
                *v *= s;
            }
        }
        '/' => {
            for v in arr.vec.iter_mut() {
                *v /= s;
            }
        }
        _ => {
            unsafe { Rf_error(b"Unknown op in scalar_op_in_place\0".as_ptr() as *const _) };
        }
    }
}

/// Transpose matrix `mat` and store in `ans`.
fn transpose_matrix(mat: &Array, ans: &mut Array) {
    array_assert(mat.ndim == 2 && ans.ndim == 2);
    array_assert(mat.dim[1] == ans.dim[0]);
    array_assert(mat.dim[0] == ans.dim[1]);

    let mut tmp = make_zero_matrix(ans.nrow(), ans.ncol());
    for i in 0..mat.nrow() {
        for j in 0..mat.ncol() {
            tmp.mat_set(j, i, mat.mat_get(i, j));
        }
    }
    copy_array(&tmp, ans);
}

/// In-place transpose of a square matrix.
fn transpose_in_place(mat: &mut Array) {
    array_assert(mat.ndim == 2);
    array_assert(mat.dim[0] == mat.dim[1]); // must be square
    let n = mat.dim[0] as usize;
    let ncol = n;
    for i in 0..n {
        for j in (i + 1)..n {
            let a_val = mat.vec[i * ncol + j];
            let b_val = mat.vec[j * ncol + i];
            mat.vec[i * ncol + j] = b_val;
            mat.vec[j * ncol + i] = a_val;
        }
    }
}

/// General matrix product C = A * B (or transposed variants).
/// trans1/trans2 indicate whether mat1/mat2 should be transposed.
fn matrix_prod(mat1: &Array, mat2: &Array, trans1: bool, trans2: bool, ans: &mut Array) {
    array_assert(mat1.ndim == 2 && mat2.ndim == 2 && ans.ndim == 2);

    let k1: usize;
    if trans1 {
        array_assert(mat1.dim[1] == ans.dim[0]);
        k1 = mat1.dim[0] as usize;
    } else {
        array_assert(mat1.dim[0] == ans.dim[0]);
        k1 = mat1.dim[1] as usize;
    }
    let k2: usize;
    if trans2 {
        array_assert(mat2.dim[0] == ans.dim[1]);
        k2 = mat2.dim[1] as usize;
    } else {
        array_assert(mat2.dim[1] == ans.dim[1]);
        k2 = mat2.dim[0] as usize;
    }
    array_assert(k1 == k2);
    let k = k1;

    let mut tmp = make_zero_matrix(ans.nrow(), ans.ncol());

    for i in 0..ans.nrow() {
        for j in 0..ans.ncol() {
            let mut sum = 0.0;
            for kk in 0..k {
                let m1 = if trans1 {
                    mat1.mat_get(kk, i)
                } else {
                    mat1.mat_get(i, kk)
                };
                let m2 = if trans2 {
                    mat2.mat_get(j, kk)
                } else {
                    mat2.mat_get(kk, j)
                };
                sum += m1 * m2;
            }
            tmp.mat_set(i, j, sum);
        }
    }
    copy_array(&tmp, ans);
}

/// QR solve via LINPACK dqrdc2/dqrsl.
/// Solves min ||y - X*coef|| and returns coefficients.
fn qr_solve(x: &Array, y: &Array, coef: &mut Array, ier: &mut i32) {
    array_assert(x.nrow() == y.nrow());
    array_assert(coef.ncol() == y.ncol());
    array_assert(x.ncol() == coef.nrow());

    let p = x.ncol() as c_int;
    let n = x.nrow() as c_int;

    // Allocate work arrays
    let mut qraux = vec![0.0f64; p as usize];
    let mut pivot = vec![0; p as usize];
    let mut work = vec![0.0f64; 2 * p as usize];
    for i in 0..p as usize {
        pivot[i] = (i + 1) as i32;
    }

    // Transpose x to column-major for LINPACK
    let mut xt = make_zero_matrix(p as usize, n as usize);
    transpose_matrix(x, &mut xt);

    // QR decomposition
    let mut rank: c_int = 0;
    unsafe {
        crate::appl::linpack_qr::dqrdc2(
            xt.vec.as_mut_ptr(),
            n,
            n,
            p,
            1e-7,
            &mut rank,
            qraux.as_mut_ptr(),
            pivot.as_mut_ptr(),
            work.as_mut_ptr(),
        );
    }

    if rank != p {
        *ier = 1;
        return;
    }

    // Transpose y to column-major
    let mut yt = make_zero_matrix(y.ncol(), y.nrow());
    transpose_matrix(y, &mut yt);

    let k = rank as usize;
    let nrhs = y.ncol();
    let mut coeft = vec![0.0f64; k * nrhs];
    let mut qty = vec![0.0f64; n as usize];
    let mut b = vec![0.0f64; k];
    for col in 0..nrhs {
        let mut info: c_int = 0;
        unsafe {
            crate::appl::linpack_qr::dqrsl(
                xt.vec.as_mut_ptr(),
                n,
                n,
                rank,
                qraux.as_ptr(),
                yt.vec.as_ptr().add(col * n as usize),
                std::ptr::null_mut(),
                qty.as_mut_ptr(),
                b.as_mut_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                100,
                &mut info,
            );
        }
        coeft[col * k..(col + 1) * k].copy_from_slice(&b);
    }

    // Copy coeft back (column-major) and transpose to get coef (row-major)
    let mut coeft_arr = make_zero_matrix(coef.ncol(), coef.nrow());
    for i in 0..coeft_arr.vector_length() {
        coeft_arr.vec[i] = coeft[i];
    }
    transpose_matrix(&coeft_arr, coef);
}

/// Log determinant of a square matrix.
/// Uses QR decomposition: log|det| = sum(log|diag(R)|)
fn ldet(x: &Array) -> f64 {
    array_assert(x.ndim == 2);
    array_assert(x.nrow() == x.ncol());

    let p = x.ncol() as c_int;
    let n = x.nrow() as c_int;

    let mut qraux = vec![0.0f64; p as usize];
    let mut pivot = vec![0; p as usize];
    let mut work = vec![0.0f64; 2 * p as usize];
    for i in 0..p as usize {
        pivot[i] = (i + 1) as i32;
    }

    // Copy x since dqrdc2 overwrites it
    let mut xtmp = make_zero_matrix(n as usize, p as usize);
    copy_array(x, &mut xtmp);

    let mut rank: c_int = 0;
    unsafe {
        crate::appl::linpack_qr::dqrdc2(
            xtmp.vec.as_mut_ptr(),
            n,
            n,
            p,
            1e-7,
            &mut rank,
            qraux.as_mut_ptr(),
            pivot.as_mut_ptr(),
            work.as_mut_ptr(),
        );
    }

    if rank != p {
        unsafe { Rf_error(b"Singular matrix in ldet\0".as_ptr() as *const c_char) };
        unreachable!();
    }

    // log|det| = sum(log|diag(R)|)
    // xtmp is column-major (ldx=n), diagonal at j + j*n
    let mut ll = 0.0f64;
    for j in 0..p as usize {
        unsafe {
            ll += (*xtmp.vec.as_ptr().add(j + j * n as usize)).abs().ln();
        }
    }
    ll
}

/// Copy elements from src into dst starting at offset.
fn copy_to_slice_at(dst: &mut [f64], src: &[f64], offset: usize) {
    for i in 0..src.len() {
        if offset + i < dst.len() {
            dst[offset + i] = src[i];
        }
    }
}

/* ==========================================================================
 * Whittle's algorithm for autoregression estimation
 * ========================================================================== */

fn whittle2(
    acf: &Array,
    aold: &Array,
    bold: &Array,
    lag: usize,
    direction: bool, // true = "forward", false = "back"
    a: &mut Array,
    k: &mut Array,
    e: &mut Array,
) {
    let nser = acf.ncol();

    let mut beta = make_zero_matrix(nser, nser);
    let mut tmp = make_zero_matrix(nser, nser);
    let id = make_identity_matrix(nser);

    set_array_to_zero(e);

    // a[0] = identity (first slice of 3D array a)
    // 3D array: dim = [omax+1, nser, nser]; slice 0 starts at offset 0
    for i in 0..nser {
        for j in 0..nser {
            a.vec[i * nser + j] = id.mat_get(i, j);
        }
    }

    let mut ier: i32 = 0;

    for i in 0..lag {
        let acf_sub = subarray(acf, lag - i);
        let aold_sub = subarray(aold, i);
        matrix_prod(&acf_sub, &aold_sub, direction, true, &mut tmp);
        array_op_in_place(&mut beta, &tmp, '+');

        let acf_sub2 = subarray(acf, i);
        let bold_sub = subarray(bold, i);
        matrix_prod(&acf_sub2, &bold_sub, direction, true, &mut tmp);
        array_op_in_place(e, &tmp, '+');
    }

    qr_solve(e, &beta, k, &mut ier);
    transpose_in_place(k);

    for i in 1..=lag {
        let bold_sub = subarray(bold, lag - i);
        matrix_prod(k, &bold_sub, false, false, &mut tmp);
        let aold_sub = subarray(aold, i);
        let off_a = i * nser * nser;
        for idx in 0..nser * nser {
            a.vec[off_a + idx] = aold_sub.vec[idx] - tmp.vec[idx];
        }
    }
}

fn whittle(
    acf: &Array,
    nlag: usize,
    a: &mut [Array],
    b: &mut [Array],
    p_forward: &mut Array,
    v_forward: &mut Array,
    p_back: &mut Array,
    v_back: &mut Array,
) {
    let nser = acf.ncol();

    let mut ka = make_zero_matrix(nser, nser);
    let mut ea = make_zero_matrix(nser, nser);
    let mut kb = make_zero_matrix(nser, nser);
    let mut eb = make_zero_matrix(nser, nser);

    let id = make_identity_matrix(nser);

    // A[0][0] = B[0][0] = I
    {
        let nsq = nser * nser;
        for i in 0..nsq {
            a[0].vec[i] = id.vec[i];
            b[0].vec[i] = id.vec[i];
        }
    }

    // p_forward[0] = p_back[0] = I
    {
        let nsq = nser * nser;
        for i in 0..nsq {
            p_forward.vec[i] = id.vec[i];
            p_back.vec[i] = id.vec[i];
        }
    }

    for lag in 1..=nlag {
        // Split slices to borrow a[lag-1] immutably and a[lag] mutably simultaneously
        let (a_head, a_tail) = a.split_at_mut(lag);
        let (b_head, b_tail) = b.split_at_mut(lag);
        whittle2(
            acf,
            &a_head[lag - 1],
            &b_head[lag - 1],
            lag,
            true,
            &mut a_tail[0],
            &mut ka,
            &mut eb,
        );
        whittle2(
            acf,
            &b_head[lag - 1],
            &a_head[lag - 1],
            lag,
            false,
            &mut b_tail[0],
            &mut kb,
            &mut ea,
        );

        // v_forward[lag-1] = EA
        {
            let off = (lag - 1) * nser * nser;
            for idx in 0..nser * nser {
                v_forward.vec[off + idx] = ea.vec[idx];
            }
        }
        // v_back[lag-1] = EB
        {
            let off = (lag - 1) * nser * nser;
            for idx in 0..nser * nser {
                v_back.vec[off + idx] = eb.vec[idx];
            }
        }
        // p_forward[lag] = KA
        {
            let off = lag * nser * nser;
            for idx in 0..nser * nser {
                p_forward.vec[off + idx] = ka.vec[idx];
            }
        }
        // p_back[lag] = KB
        {
            let off = lag * nser * nser;
            for idx in 0..nser * nser {
                p_back.vec[off + idx] = kb.vec[idx];
            }
        }
    }

    // v_forward[nlag] = EA * (I - KB' * KA)
    let mut tmp = make_zero_matrix(nser, nser);
    let mut tmp2 = make_zero_matrix(nser, nser);
    matrix_prod(&kb, &ka, true, true, &mut tmp);
    array_op(&id, &tmp, '-', &mut tmp2);
    {
        let off = nlag * nser * nser;
        let mut result = make_zero_matrix(nser, nser);
        matrix_prod(&ea, &tmp2, false, false, &mut result);
        for idx in 0..nser * nser {
            v_forward.vec[off + idx] = result.vec[idx];
        }
    }
}

/// multi_yw - Whittle's algorithm for multivariate autoregression.
/// Interface to R, also handles model selection using AIC.
pub unsafe fn multi_yw(
    acf: *mut f64,
    pn: *mut c_int,
    pomax: *mut c_int,
    pnser: *mut c_int,
    coef: *mut f64,
    pacf: *mut f64,
    var: *mut f64,
    aic: *mut f64,
    porder: *mut c_int,
    useaic: *mut c_int,
) {
    unsafe {
        let omax = *pomax as usize;
        let n = *pn as usize;
        let nser = *pnser as usize;
        let mut order = *porder as usize;
        let useaic_flag = *useaic != 0;

        let dim = [(omax + 1) as i32, nser as i32, nser as i32];
        let total_len = (omax + 1) * nser * nser;

        let acf_slice = slice::from_raw_parts(acf, total_len);
        let acf_array = make_array(acf_slice, &dim, 3);

        let pacf_slice = slice::from_raw_parts(pacf, total_len);
        let mut p_forward = make_array(pacf_slice, &dim, 3);

        let var_slice = slice::from_raw_parts(var, total_len);
        let mut v_forward = make_array(var_slice, &dim, 3);

        // Backward equations (discarded but needed by algorithm)
        let mut p_back = make_zero_array(&dim, 3);
        let mut v_back = make_zero_array(&dim, 3);

        // Allocate A and B arrays
        let mut a_vec: Vec<Array> = Vec::with_capacity(omax + 2);
        let mut b_vec: Vec<Array> = Vec::with_capacity(omax + 2);
        for _ in 0..=omax {
            a_vec.push(make_zero_array(&dim, 3));
            b_vec.push(make_zero_array(&dim, 3));
        }

        whittle(
            &acf_array,
            omax,
            &mut a_vec,
            &mut b_vec,
            &mut p_forward,
            &mut v_forward,
            &mut p_back,
            &mut v_back,
        );

        // Model order selection
        let aic_slice = slice::from_raw_parts_mut(aic, omax + 1);
        for m in 0..=omax {
            let v_sub = subarray(&v_forward, m);
            let ld = ldet(&v_sub);
            aic_slice[m] = (n as f64) * ld + 2.0 * (m as f64) * (nser as f64) * (nser as f64);
        }

        if useaic_flag {
            order = 0;
            let mut aicmin = aic_slice[0];
            for m in 0..=omax {
                if aic_slice[m] < aicmin {
                    aicmin = aic_slice[m];
                    order = m;
                }
            }
        } else {
            order = omax;
        }
        *porder = order as c_int;

        // Copy coefficients
        let coef_slice = slice::from_raw_parts_mut(coef, a_vec[order].vector_length());
        for i in 0..coef_slice.len() {
            coef_slice[i] = a_vec[order].vec[i];
        }

        // Write back pacf and var
        let pacf_out = slice::from_raw_parts_mut(pacf, p_forward.vector_length());
        for i in 0..pacf_out.len() {
            pacf_out[i] = p_forward.vec[i];
        }
        let var_out = slice::from_raw_parts_mut(var, v_forward.vector_length());
        for i in 0..var_out.len() {
            var_out[i] = v_forward.vec[i];
        }
    }
}

pub unsafe extern "C-unwind" fn c_multi_yw(
    acf: *mut std::ffi::c_void,
    n: *mut std::ffi::c_void,
    omax: *mut std::ffi::c_void,
    nser: *mut std::ffi::c_void,
    coef: *mut std::ffi::c_void,
    pacf: *mut std::ffi::c_void,
    var: *mut std::ffi::c_void,
    aic: *mut std::ffi::c_void,
    order: *mut std::ffi::c_void,
    useaic: *mut std::ffi::c_void,
) {
    unsafe {
        multi_yw(
            acf as *mut f64,
            n as *mut c_int,
            omax as *mut c_int,
            nser as *mut c_int,
            coef as *mut f64,
            pacf as *mut f64,
            var as *mut f64,
            aic as *mut f64,
            order as *mut c_int,
            useaic as *mut c_int,
        );
    }
}
