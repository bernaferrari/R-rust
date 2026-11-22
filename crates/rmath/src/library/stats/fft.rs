//! Safe numerical kernel for the R-level Fourier transforms.

#![forbid(unsafe_code)]

use num::complex::Complex64;

/// Pure-Rust mixed-radix discrete Fourier transform.
///
/// Composite lengths use Cooley--Tukey decomposition; prime leaves use the
/// direct transform.  This keeps arbitrary-length GNU R semantics without a
/// BLAS/Fortran dependency (notably on WASM and Android), while avoiding the
/// quadratic path for the overwhelmingly common composite sizes.
pub(super) fn transform(values: &[Complex64], inverse: bool) -> Vec<Complex64> {
    transform_checkpoint();
    let n = values.len();
    if n <= 1 {
        return values.to_vec();
    }

    let mut factor = n;
    let mut candidate = 2usize;
    while candidate <= n / candidate {
        if n.is_multiple_of(candidate) {
            factor = candidate;
            break;
        }
        candidate += 1;
    }
    let sign = if inverse { 1.0 } else { -1.0 };

    if factor == n {
        return (0..n)
            .map(|k| {
                let mut sum = Complex64::new(0.0, 0.0);
                for (index, &value) in values.iter().enumerate() {
                    if index.is_multiple_of(1024) {
                        transform_checkpoint();
                    }
                    let twiddle = Complex64::from_polar(
                        1.0,
                        sign * std::f64::consts::TAU * ((k * index) % n) as f64 / n as f64,
                    );
                    sum += value * twiddle;
                }
                sum
            })
            .collect();
    }

    let inner_len = n / factor;
    let inner: Vec<Vec<Complex64>> = (0..factor)
        .map(|residue| {
            let lane: Vec<_> = (0..inner_len)
                .map(|index| values[index * factor + residue])
                .collect();
            transform(&lane, inverse)
        })
        .collect();

    let mut output = vec![Complex64::new(0.0, 0.0); n];
    for k in 0..n {
        if k.is_multiple_of(1024) {
            transform_checkpoint();
        }
        let inner_k = k % inner_len;
        let step = Complex64::from_polar(1.0, sign * std::f64::consts::TAU * k as f64 / n as f64);
        let mut twiddle = Complex64::new(1.0, 0.0);
        for lane in &inner {
            output[k] += lane[inner_k] * twiddle;
            twiddle *= step;
        }
    }
    output
}

fn transform_checkpoint() {
    crate::sexp::instance::check_cancellation();
    if crate::sexp::instance::with_current_instance(|_| ()).is_some() {
        let _guard = crate::eval::limits::check_eval_depth().unwrap_or_else(|message| {
            std::panic::panic_any(crate::sexp::context::RError { message });
        });
    }
}
