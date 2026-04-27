/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Ported to Rust from fft.c
 *
 *  Fast Fourier Transform
 *
 *  These routines are based on code by Richard Singleton in the
 *  book "Programs for Digital Signal Processing" put out by IEEE.
 *
 *  Original C translation by Ross Ihaka, University of Auckland, Feb 1997.
 */
use libc::{c_double, c_int};

use crate::sexp::instance::with_required_current_instance;

#[derive(Clone, Copy, Default)]
pub(crate) struct FftState {
    old_n: c_int,
    nfac: [c_int; 20],
    m_fac: c_int,
    kt: c_int,
    maxf: c_int,
    maxp: c_int,
}

fn with_fft_state<R>(f: impl FnOnce(&mut FftState) -> R) -> R {
    with_required_current_instance(|instance| f(&mut instance.fft_state))
}

/// fft_factor - factorization check and determination of memory
/// requirements for the fft.
///
/// On return, `pmaxf` will give the maximum factor size
/// and `pmaxp` will give the amount of integer scratch storage required.
///
/// If `*pmaxf == 0`, there was an error:
///   If `*pmaxp == 0`  There was an illegal zero parameter.
///   If `*pmaxp == 1`  There were more than 20 factors to ntot.
pub unsafe fn fft_factor(n: c_int, pmaxf: *mut c_int, pmaxp: *mut c_int) {
    unsafe {
        with_fft_state(|state| {
            let mut j: c_int;
            let mut jj: c_int;
            let mut k: c_int;
            let mut sqrtk: c_int;
            let mut kchanged: c_int;

            if n <= 0 {
                state.old_n = 0;
                *pmaxf = 0;
                *pmaxp = 0;
                return;
            }

            *state = FftState::default();
            state.old_n = n;

            k = n;
            if k == 1 {
                return;
            }

            while k % 16 == 0 {
                state.nfac[state.m_fac as usize] = 4;
                state.m_fac += 1;
                k /= 16;
            }

            kchanged = 0;
            sqrtk = libm::sqrt(k as f64) as c_int;
            j = 3;
            while j <= sqrtk {
                jj = j * j;
                while k % jj == 0 {
                    state.nfac[state.m_fac as usize] = j;
                    state.m_fac += 1;
                    k /= jj;
                    kchanged = 1;
                }
                if kchanged != 0 {
                    kchanged = 0;
                    sqrtk = libm::sqrt(k as f64) as c_int;
                }
                j += 2;
            }

            if k <= 4 {
                state.kt = state.m_fac;
                state.nfac[state.m_fac as usize] = k;
                if k != 1 {
                    state.m_fac += 1;
                }
            } else {
                if k % 4 == 0 {
                    state.nfac[state.m_fac as usize] = 2;
                    state.m_fac += 1;
                    k /= 4;
                }

                state.kt = state.m_fac;
                state.maxp = std::cmp::max(state.kt + state.kt + 2, k - 1);
                j = 2;
                loop {
                    if k % j == 0 {
                        state.nfac[state.m_fac as usize] = j;
                        state.m_fac += 1;
                        k /= j;
                    }
                    if j > c_int::MAX - 2 {
                        break;
                    }
                    j = ((j + 1) / 2) * 2 + 1;
                    if j > k {
                        break;
                    }
                }
            }

            if state.m_fac <= state.kt + 1 {
                state.maxp = state.m_fac + state.kt + 1;
            }
            if state.m_fac + state.kt > 20 {
                state.old_n = 0;
                *pmaxf = 0;
                *pmaxp = 0;
                return;
            }

            if state.kt != 0 {
                j = state.kt;
                while j != 0 {
                    j -= 1;
                    state.nfac[state.m_fac as usize] = state.nfac[j as usize];
                    state.m_fac += 1;
                }
            }
            state.maxf = state.nfac[(state.m_fac - state.kt - 1) as usize];
            if state.kt > 0 {
                state.maxf = std::cmp::max(state.nfac[(state.kt - 1) as usize], state.maxf);
            }
            if state.kt > 1 {
                state.maxf = std::cmp::max(state.nfac[(state.kt - 2) as usize], state.maxf);
            }
            if state.kt > 2 {
                state.maxf = std::cmp::max(state.nfac[(state.kt - 3) as usize], state.maxf);
            }

            *pmaxf = state.maxf;
            *pmaxp = state.maxp;
        });
    }
}

/// fft_work - perform the FFT transform.
///
/// Returns 1 (TRUE) if the transform was completed successfully,
/// 0 (FALSE) if invalid values of the parameters were supplied.
pub unsafe fn fft_work(
    a: *mut c_double,
    b: *mut c_double,
    nseg: c_int,
    n: c_int,
    nspn: c_int,
    isn: c_int,
    work: *mut c_double,
    iwork: *mut c_int,
) -> c_int {
    unsafe {
        let state = with_fft_state(|state| *state);

        if state.old_n == 0 {
            return 0;
        }

        if n != state.old_n || nseg <= 0 || nspn <= 0 || isn == 0 {
            return 0;
        }

        let mf = state.maxf as usize;
        let nspan = n * nspn;
        let ntot = nspan * nseg;

        fftmx(
            a,
            b,
            ntot,
            n,
            nspan,
            isn,
            state.m_fac,
            state.kt,
            state.nfac,
            work,
            work.add(mf),
            work.add(2 * mf),
            work.add(3 * mf),
            iwork,
        );

        1 // TRUE
    }
}

/// Internal FFT routine - the Singleton mixed-radix FFT algorithm.
///
/// Ported from Fortran with 1-based indexing converted to 0-based.
/// The original C code did `a--; b--; at--; ck--; bt--; sk--; np--;` to
/// shift pointers to 1-based. Here we keep 0-based and adjust all index
/// calculations: where C had `a[k]`, we use `a[(k-1) as usize]`.
unsafe fn fftmx(
    a: *mut c_double,
    b: *mut c_double,
    ntot: c_int,
    n: c_int,
    nspan: c_int,
    isn: c_int,
    m: c_int,
    kt: c_int,
    mut nfac: [c_int; 20],
    at: *mut c_double,
    ck: *mut c_double,
    bt: *mut c_double,
    sk: *mut c_double,
    np: *mut c_int,
) {
    unsafe {
        let mut aa: c_double;
        let mut aj: c_double;
        let mut ajm: c_double;
        let mut ajp: c_double;
        let mut ak: c_double;
        let mut akm: c_double;
        let mut akp: c_double;
        let mut bb: c_double;
        let mut bj: c_double;
        let mut bjm: c_double;
        let mut bjp: c_double;
        let mut bk: c_double;
        let mut bkm: c_double;
        let mut bkp: c_double;
        let mut c1: c_double;
        let mut c2: c_double = 0.0;
        let mut c3: c_double = 0.0;
        let c72: c_double;
        let mut cd: c_double;
        let mut dr: c_double;
        let mut rad: c_double;
        let mut s1: c_double;
        let mut s120: c_double;
        let mut s2: c_double = 0.0;
        let mut s3: c_double = 0.0;
        let mut s72: c_double;
        let mut sd: c_double;
        let mut i: c_int;
        let mut inc: c_int;
        let mut j: c_int;
        let mut jc: c_int;
        let mut jf: c_int;
        let mut jj: c_int;
        let mut k: c_int;
        let mut k1: c_int;
        let mut k2: c_int;
        let mut k3: c_int = 0;
        let mut k4: c_int;
        let mut kk: c_int;
        let mut klim: c_int;
        let mut ks: c_int;
        let mut kspan: c_int;
        let mut kspnn: c_int = 0;
        let mut lim: c_int;
        let mut maxf: c_int;
        let mut mm: c_int;
        let mut nn: c_int;
        let mut nt: c_int;

        // Helper macro: convert 1-based index to 0-based pointer offset
        // C code did a-- etc. to make a[k] (1-based) == original a[k-1] (0-based).
        // We use A(k) to mean a[(k-1)].
        macro_rules! A {
            ($idx:expr) => {
                *a.add(($idx - 1) as usize)
            };
        }
        macro_rules! B {
            ($idx:expr) => {
                *b.add(($idx - 1) as usize)
            };
        }
        macro_rules! AT {
            ($idx:expr) => {
                *at.add(($idx - 1) as usize)
            };
        }
        macro_rules! BT {
            ($idx:expr) => {
                *bt.add(($idx - 1) as usize)
            };
        }
        macro_rules! CK {
            ($idx:expr) => {
                *ck.add(($idx - 1) as usize)
            };
        }
        macro_rules! SK {
            ($idx:expr) => {
                *sk.add(($idx - 1) as usize)
            };
        }
        macro_rules! NP {
            ($idx:expr) => {
                *np.add(($idx - 1) as usize)
            };
        }
        macro_rules! NF {
            ($idx:expr) => {
                nfac[$idx as usize]
            };
        }
        macro_rules! NF_set {
            ($idx:expr, $val:expr) => {
                nfac[$idx as usize] = $val
            };
        }

        inc = isn.abs();
        nt = inc * ntot;
        ks = inc * nspan;
        rad = std::f64::consts::FRAC_PI_4; /* pi/4 = 45 degrees */
        s72 = rad / 0.625; /* 72 = 45 / .625 degrees */
        c72 = libm::cos(s72);
        s72 = libm::sin(s72);
        s120 = 0.5 * libm::sqrt(3.0); /* sin(120) = sqrt(3)/2 */
        if isn <= 0 {
            s72 = -s72;
            s120 = -s120;
            rad = -rad;
        }
        // Note: SCALING code omitted (same as C without SCALING defined)

        kspan = ks;
        nn = nt - inc;
        jc = ks / n;

        // sin, cos values are re-initialized each lim steps
        lim = 32;
        klim = lim * jc;
        i = 0;
        jf = 0;
        maxf = NF!(m - kt - 1);
        if kt > 0 {
            maxf = std::cmp::max(NF!(kt - 1), maxf);
        }

        // compute fourier transform
        // L_start:
        loop {
            'factor_loop: loop {
                dr = (8.0 * jc as c_double) / kspan as c_double;
                cd = libm::sin(0.5 * dr * rad);
                cd = 2.0 * cd * cd;
                sd = libm::sin(dr * rad);
                kk = 1;
                i += 1;

                if NF!(i - 1) != 2 {
                    // goto L110
                    if NF!(i - 1) != 4 {
                        // goto L_f_odd
                        k = NF!(i - 1);
                        kspnn = kspan;
                        kspan /= k;
                        if k == 3 {
                            // goto L100: transform for factor of 3
                            loop {
                                k1 = kk + kspan;
                                k2 = k1 + kspan;
                                ak = A!(kk);
                                bk = B!(kk);
                                aj = A!(k1) + A!(k2);
                                bj = B!(k1) + B!(k2);
                                A!(kk) = ak + aj;
                                B!(kk) = bk + bj;
                                ak = -0.5 * aj + ak;
                                bk = -0.5 * bj + bk;
                                aj = (A!(k1) - A!(k2)) * s120;
                                bj = (B!(k1) - B!(k2)) * s120;
                                A!(k1) = ak - bj;
                                B!(k1) = bk + aj;
                                A!(k2) = ak + bj;
                                B!(k2) = bk - aj;
                                kk = k2 + kspan;
                                if kk < nn {
                                    continue;
                                }
                                kk = kk - nn;
                                if kk <= kspan {
                                    continue;
                                }
                                break; // goto L290
                            }
                        } else if k == 5 {
                            // goto L_f5: transform for factor of 5
                            loop {
                                c2 = c72 * c72 - s72 * s72;
                                s2 = 2.0 * c72 * s72;
                                // L220:
                                k1 = kk + kspan;
                                k2 = k1 + kspan;
                                k3 = k2 + kspan;
                                k4 = k3 + kspan;
                                akp = A!(k1) + A!(k4);
                                akm = A!(k1) - A!(k4);
                                bkp = B!(k1) + B!(k4);
                                bkm = B!(k1) - B!(k4);
                                ajp = A!(k2) + A!(k3);
                                ajm = A!(k2) - A!(k3);
                                bjp = B!(k2) + B!(k3);
                                bjm = B!(k2) - B!(k3);
                                aa = A!(kk);
                                bb = B!(kk);
                                A!(kk) = aa + akp + ajp;
                                B!(kk) = bb + bkp + bjp;
                                ak = akp * c72 + ajp * c2 + aa;
                                bk = bkp * c72 + bjp * c2 + bb;
                                aj = akm * s72 + ajm * s2;
                                bj = bkm * s72 + bjm * s2;
                                A!(k1) = ak - bj;
                                A!(k4) = ak + bj;
                                B!(k1) = bk + aj;
                                B!(k4) = bk - aj;
                                ak = akp * c2 + ajp * c72 + aa;
                                bk = bkp * c2 + bjp * c72 + bb;
                                aj = akm * s2 - ajm * s72;
                                bj = bkm * s2 - bjm * s72;
                                A!(k2) = ak - bj;
                                A!(k3) = ak + bj;
                                B!(k2) = bk + aj;
                                B!(k3) = bk - aj;
                                kk = k4 + kspan;
                                if kk < nn {
                                    continue;
                                }
                                kk = kk - nn;
                                if kk <= kspan {
                                    continue;
                                }
                                break; // goto L290
                            }
                        } else if k == jf {
                            // goto L250: odd factor transform (reuse cached twiddle)
                            loop {
                                // L250:
                                k1 = kk;
                                k2 = kk + kspnn;
                                aa = A!(kk);
                                bb = B!(kk);
                                ak = aa;
                                bk = bb;
                                j = 1;
                                k1 = k1 + kspan;
                                // L260:
                                loop {
                                    k2 = k2 - kspan;
                                    j += 1;
                                    AT!(j) = A!(k1) + A!(k2);
                                    ak = AT!(j) + ak;
                                    BT!(j) = B!(k1) + B!(k2);
                                    bk = BT!(j) + bk;
                                    j += 1;
                                    AT!(j) = A!(k1) - A!(k2);
                                    BT!(j) = B!(k1) - B!(k2);
                                    k1 = k1 + kspan;
                                    if k1 < k2 {
                                        continue;
                                    }
                                    break;
                                }
                                A!(kk) = ak;
                                B!(kk) = bk;
                                k1 = kk;
                                k2 = kk + kspnn;
                                j = 1;
                                // L270:
                                loop {
                                    k1 += kspan;
                                    k2 -= kspan;
                                    jj = j;
                                    ak = aa;
                                    bk = bb;
                                    aj = 0.0;
                                    bj = 0.0;
                                    k = 1;
                                    while k < jf {
                                        ak += AT!(k) * CK!(jj);
                                        bk += BT!(k) * CK!(jj);
                                        k += 1;
                                        aj += AT!(k) * SK!(jj);
                                        bj += BT!(k) * SK!(jj);
                                        jj += j;
                                        if jj > jf {
                                            jj -= jf;
                                        }
                                    }
                                    k = jf - j;
                                    A!(k1) = ak - bj;
                                    B!(k1) = bk + aj;
                                    A!(k2) = ak + bj;
                                    B!(k2) = bk - aj;
                                    j += 1;
                                    if j < k {
                                        continue;
                                    }
                                    break;
                                }
                                kk = kk + kspnn;
                                if kk <= nn {
                                    continue;
                                }
                                kk = kk - nn;
                                if kk <= kspan {
                                    continue;
                                }
                                break; // goto L290
                            }
                        } else {
                            // odd factor: compute and cache twiddle factors
                            jf = k;
                            s1 = rad / (k as f64 / 8.0);
                            c1 = libm::cos(s1);
                            s1 = libm::sin(s1);
                            CK!(jf) = 1.0;
                            SK!(jf) = 0.0;

                            let mut jj_k = k;
                            for jj_j in 1..k {
                                CK!(jj_j) = CK!(jj_k) * c1 + SK!(jj_k) * s1;
                                SK!(jj_j) = CK!(jj_k) * s1 - SK!(jj_k) * c1;
                                jj_k -= 1;
                                CK!(jj_k) = CK!(jj_j);
                                SK!(jj_k) = -SK!(jj_j);
                            }
                            // now goto L250
                            loop {
                                // L250:
                                k1 = kk;
                                k2 = kk + kspnn;
                                aa = A!(kk);
                                bb = B!(kk);
                                ak = aa;
                                bk = bb;
                                j = 1;
                                k1 = k1 + kspan;
                                // L260:
                                loop {
                                    k2 = k2 - kspan;
                                    j += 1;
                                    AT!(j) = A!(k1) + A!(k2);
                                    ak = AT!(j) + ak;
                                    BT!(j) = B!(k1) + B!(k2);
                                    bk = BT!(j) + bk;
                                    j += 1;
                                    AT!(j) = A!(k1) - A!(k2);
                                    BT!(j) = B!(k1) - B!(k2);
                                    k1 = k1 + kspan;
                                    if k1 < k2 {
                                        continue;
                                    }
                                    break;
                                }
                                A!(kk) = ak;
                                B!(kk) = bk;
                                k1 = kk;
                                k2 = kk + kspnn;
                                j = 1;
                                // L270:
                                loop {
                                    k1 += kspan;
                                    k2 -= kspan;
                                    jj = j;
                                    ak = aa;
                                    bk = bb;
                                    aj = 0.0;
                                    bj = 0.0;
                                    k = 1;
                                    while k < jf {
                                        ak += AT!(k) * CK!(jj);
                                        bk += BT!(k) * CK!(jj);
                                        k += 1;
                                        aj += AT!(k) * SK!(jj);
                                        bj += BT!(k) * SK!(jj);
                                        jj += j;
                                        if jj > jf {
                                            jj -= jf;
                                        }
                                    }
                                    k = jf - j;
                                    A!(k1) = ak - bj;
                                    B!(k1) = bk + aj;
                                    A!(k2) = ak + bj;
                                    B!(k2) = bk - aj;
                                    j += 1;
                                    if j < k {
                                        continue;
                                    }
                                    break;
                                }
                                kk = kk + kspnn;
                                if kk <= nn {
                                    continue;
                                }
                                kk = kk - nn;
                                if kk <= kspan {
                                    continue;
                                }
                                break; // goto L290
                            }
                        }
                    } else {
                        // nfac[i-1] == 4: transform for factor of 4
                        kspnn = kspan;
                        kspan /= 4;

                        // L120:
                        loop {
                            c1 = 1.0;
                            s1 = 0.0;
                            mm = std::cmp::min(kspan, klim);
                            // L150:
                            loop {
                                // L140:
                                if s1 != 0.0 {
                                    c2 = c1 * c1 - s1 * s1;
                                    s2 = c1 * s1 * 2.0;
                                    c3 = c2 * c1 - s2 * s1;
                                    s3 = c2 * s1 + s2 * c1;
                                } else {
                                    c2 = 1.0;
                                    s2 = 0.0;
                                    c3 = 1.0;
                                    s3 = 0.0;
                                }

                                // L150 body:
                                k1 = kk + kspan;
                                k2 = k1 + kspan;
                                k3 = k2 + kspan;
                                akp = A!(kk) + A!(k2);
                                akm = A!(kk) - A!(k2);
                                ajp = A!(k1) + A!(k3);
                                ajm = A!(k1) - A!(k3);
                                A!(kk) = akp + ajp;
                                ajp = akp - ajp;
                                bkp = B!(kk) + B!(k2);
                                bkm = B!(kk) - B!(k2);
                                bjp = B!(k1) + B!(k3);
                                bjm = B!(k1) - B!(k3);
                                B!(kk) = bkp + bjp;
                                bjp = bkp - bjp;

                                if isn < 0 {
                                    akp = akm + bjm;
                                    akm = akm - bjm;
                                    bkp = bkm - ajm;
                                    bkm = bkm + ajm;
                                } else {
                                    akp = akm - bjm;
                                    akm = akm + bjm;
                                    bkp = bkm + ajm;
                                    bkm = bkm - ajm;
                                }

                                if s1 == 0.0 {
                                    // L190:
                                    A!(k1) = akp;
                                    B!(k1) = bkp;
                                    A!(k2) = ajp;
                                    B!(k2) = bjp;
                                    A!(k3) = akm;
                                    B!(k3) = bkm;
                                } else {
                                    // L160:
                                    A!(k1) = akp * c1 - bkp * s1;
                                    B!(k1) = akp * s1 + bkp * c1;
                                    A!(k2) = ajp * c2 - bjp * s2;
                                    B!(k2) = ajp * s2 + bjp * c2;
                                    A!(k3) = akm * c3 - bkm * s3;
                                    B!(k3) = akm * s3 + bkm * c3;
                                }

                                kk = k3 + kspan;
                                if kk <= nt {
                                    continue;
                                }
                                // L170:
                                kk = kk - nt + jc;
                                if kk <= mm {
                                    // L130:
                                    c2 = c1 - (cd * c1 + sd * s1);
                                    s1 = (sd * c1 - cd * s1) + s1;
                                    // Rounded arithmetic: c1 = c2
                                    c1 = c2;
                                    continue;
                                }
                                if kk < kspan {
                                    // L200:
                                    s1 = ((kk - 1) / jc) as c_double * dr * rad;
                                    c1 = libm::cos(s1);
                                    s1 = libm::sin(s1);
                                    mm = std::cmp::min(kspan, mm + klim);
                                    continue;
                                }
                                break; // exit L150 loop
                            }
                            // after L150 / L170
                            kk = kk - kspan + inc;
                            if kk <= jc {
                                continue; // goto L120
                            }
                            if kspan == jc {
                                break 'factor_loop; // goto L_fin
                            }
                            break; // goto L_start (outer loop)
                        }
                    }
                } else {
                    // nfac[i-1] == 2: transform for factor of 2

                    kspan /= 2;
                    k1 = kspan + 2;
                    loop {
                        loop {
                            k2 = kk + kspan;
                            ak = A!(k2);
                            bk = B!(k2);
                            A!(k2) = A!(kk) - ak;
                            B!(k2) = B!(kk) - bk;
                            A!(kk) += ak;
                            B!(kk) += bk;
                            kk = k2 + kspan;
                            if kk <= nn {
                                continue;
                            }
                            break;
                        }
                        kk -= nn;
                        if kk > jc {
                            break;
                        }
                    }

                    if kk > kspan {
                        break 'factor_loop; // goto L_fin
                    }

                    // L60:
                    loop {
                        c1 = 1.0 - cd;
                        s1 = sd;
                        mm = std::cmp::min(k1 / 2, klim);

                        // L80:
                        loop {
                            loop {
                                k2 = kk + kspan;
                                ak = A!(kk) - A!(k2);
                                bk = B!(kk) - B!(k2);
                                A!(kk) += A!(k2);
                                B!(kk) += B!(k2);
                                A!(k2) = c1 * ak - s1 * bk;
                                B!(k2) = s1 * ak + c1 * bk;
                                kk = k2 + kspan;
                                if kk < nt {
                                    continue;
                                }
                                break;
                            }
                            k2 = kk - nt;
                            c1 = -c1;
                            kk = k1 - k2;
                            if kk > k2 {
                                continue;
                            }
                            kk += jc;
                            if kk <= mm {
                                // L70:
                                ak = c1 - (cd * c1 + sd * s1);
                                s1 = (sd * c1 - cd * s1) + s1;
                                // Rounded arithmetic: c1 = ak
                                c1 = ak;
                                continue;
                            }
                            if kk >= k2 {
                                k1 = k1 + inc + inc;
                                kk = (k1 - kspan) / 2 + jc;
                                if kk <= jc + jc {
                                    continue; // goto L60
                                }
                                break; // goto L_start
                            }
                            // re-init sin/cos
                            s1 = ((kk - 1) / jc) as c_double * dr * rad;
                            c1 = libm::cos(s1);
                            s1 = libm::sin(s1);
                            mm = std::cmp::min(k1 / 2, mm + klim);
                            continue; // goto L80
                        }
                        // exited L80 via goto L_start
                        break; // continue outer factor_loop
                    }
                }

                // L290: multiply by rotation factor (except for factors of 2 and 4)
                if i == m {
                    break 'factor_loop; // goto L_fin
                }
                kk = jc + 1;
                // L300:
                loop {
                    c2 = 1.0 - cd;
                    s1 = sd;
                    mm = std::cmp::min(kspan, klim);

                    loop {
                        // L320:
                        c1 = c2;
                        s2 = s1;
                        kk += kspan;
                        // L330:
                        loop {
                            loop {
                                ak = A!(kk);
                                A!(kk) = c2 * ak - s2 * B!(kk);
                                B!(kk) = s2 * ak + c2 * B!(kk);
                                kk += kspnn;
                                if kk <= nt {
                                    continue;
                                }
                                break;
                            }
                            ak = s1 * s2;
                            s2 = s1 * c2 + c1 * s2;
                            c2 = c1 * c2 - ak;
                            kk += -nt + kspan;
                            if kk <= kspnn {
                                continue;
                            }
                            break;
                        }
                        kk += -kspnn + jc;
                        if kk <= mm {
                            // L310:
                            c2 = c1 - (cd * c1 + sd * s1);
                            s1 = s1 + (sd * c1 - cd * s1);
                            // Truncated arithmetic compensation omitted (using rounded)
                            continue; // goto L320
                        }
                        if kk >= kspan {
                            kk = kk - kspan + jc + inc;
                            if kk <= jc + jc {
                                continue; // goto L300
                            }
                            break; // goto L_start
                        }
                        // re-init sin/cos
                        s1 = ((kk - 1) / jc) as c_double * dr * rad;
                        c2 = libm::cos(s1);
                        s1 = libm::sin(s1);
                        mm = std::cmp::min(kspan, mm + klim);
                        continue; // back to L320
                    }
                    // goto L_start
                    break; // continue outer factor_loop
                }
            } // end 'factor_loop

            break; // only one iteration needed after break from inner
        }

        // L_fin: permute the results to normal order
        NP!(1) = ks;
        if kt == 0 {
            // goto L440
        } else {
            k = kt + kt + 1;
            if m < k {
                k -= 1;
            }
            NP!(k + 1) = jc;
            let mut jj_np = 1;
            let mut kk_np = k;
            while jj_np < kk_np {
                NP!(jj_np + 1) = NP!(jj_np) / NF!(jj_np - 1);
                NP!(kk_np) = NP!(kk_np + 1) * NF!(jj_np - 1);
                jj_np += 1;
                kk_np -= 1;
            }
            k3 = NP!(k + 1);
            kspan = NP!(2);
            kk = jc + 1;
            k2 = kspan + 1;
            j = 1;

            if n == ntot {
                // permutation for single-variate transform
                loop {
                    // L370:
                    loop {
                        ak = A!(kk);
                        A!(kk) = A!(k2);
                        A!(k2) = ak;
                        bk = B!(kk);
                        B!(kk) = B!(k2);
                        B!(k2) = bk;
                        kk += inc;
                        k2 += kspan;
                        if k2 < ks {
                            continue;
                        }
                        break;
                    }
                    // L380:
                    loop {
                        k2 -= NP!(j);
                        j += 1;
                        k2 += NP!(j + 1);
                        if k2 > NP!(j) {
                            continue;
                        }
                        break;
                    }
                    j = 1;
                    loop {
                        if kk < k2 {
                            break; // goto L370
                        }
                        kk += inc;
                        k2 += kspan;
                        if k2 < ks {
                            continue;
                        }
                        break;
                    }
                    if kk < k2 {
                        continue; // goto L370 (the break above exits the inner loop)
                    }
                    if kk < ks {
                        continue; // goto L380
                    }
                    break;
                }
                jc = k3;
            } else {
                // permutation for multivariate transform
                loop {
                    // L400:
                    loop {
                        k = kk + jc;
                        loop {
                            ak = A!(kk);
                            A!(kk) = A!(k2);
                            A!(k2) = ak;
                            bk = B!(kk);
                            B!(kk) = B!(k2);
                            B!(k2) = bk;
                            kk += inc;
                            k2 += inc;
                            if kk < k {
                                continue;
                            }
                            break;
                        }
                        kk += ks - jc;
                        k2 += ks - jc;
                        if kk < nt {
                            continue;
                        }
                        break;
                    }
                    k2 += -nt + kspan;
                    kk += -nt + jc;
                    if k2 < ks {
                        continue; // goto L400
                    }

                    loop {
                        loop {
                            k2 -= NP!(j);
                            j += 1;
                            k2 += NP!(j + 1);
                            if k2 > NP!(j) {
                                continue;
                            }
                            break;
                        }
                        j = 1;
                        loop {
                            if kk < k2 {
                                break; // goto L400
                            }
                            kk += jc;
                            k2 += kspan;
                            if k2 < ks {
                                continue;
                            }
                            break;
                        }
                        if kk < k2 {
                            continue; // goto L400
                        }
                        if kk < ks {
                            continue;
                        }
                        break;
                    }
                    break;
                }
                jc = k3;
            }
        }

        // L440:
        if 2 * kt + 1 >= m {
            return;
        }
        kspnn = NP!(kt + 1);

        // permutation for square-free factors of n
        // Here, nfac[] is overwritten... now CUMULATIVE ("cumprod") factors
        nn = m - kt;
        NF_set!(nn, 1);
        let mut jj_loop = nn;
        while jj_loop > kt {
            nfac[jj_loop as usize - 1] *= nfac[jj_loop as usize];
            jj_loop -= 1;
        }
        // Work on local copies so the per-instance factorization plan remains reusable.
        let mut kt_local = kt;
        kt_local += 1;
        nn = NF!(kt_local - 1) - 1;
        jj = 0;
        j = 0;

        // L480 / L470 / L460
        loop {
            // L480:
            k2 = NF!(kt_local - 1);
            k = kt_local + 1;
            kk = NF!(k - 1);
            j += 1;
            if j <= nn {
                // L470:
                loop {
                    jj += kk;
                    if jj >= k2 {
                        // L460:
                        jj -= k2;
                        k2 = kk;
                        k += 1;
                        kk = NF!(k - 1);
                        if jj < k2 {
                            break; // exit L470
                        }
                        continue;
                    }
                    break;
                }
                NP!(j) = jj;
                continue; // goto L480
            }
            break;
        }

        // determine the permutation cycles of length greater than 1
        j = 0;
        let mut k3_cycle = 0;
        loop {
            // L500:
            loop {
                j += 1;
                kk = NP!(j);
                if kk >= 0 {
                    break;
                }
            }
            if kk != j {
                // cycle:
                loop {
                    let tmp = kk;
                    kk = NP!(kk);
                    NP!(tmp) = -kk;
                    if kk == j {
                        break;
                    }
                }
                k3_cycle = kk;
                continue; // goto L500
            }
            NP!(j) = -j;
            if j != nn {
                continue; // goto L500
            }
            break;
        }
        let maxf_perm = maxf * inc;

        // L570 / L_ord: reorder a and b, following the permutation cycles
        j = k3_cycle + 1;
        nt = nt - kspnn;
        i = nt - inc + 1;
        while nt >= 0 {
            // L_ord:
            loop {
                while NP!(j) < 0 {
                    j -= 1;
                }
                let mut jj_ord = jc;

                // L520:
                loop {
                    kspan = std::cmp::min(jj_ord, maxf_perm);
                    jj_ord -= kspan;
                    k = NP!(j);
                    kk = jc * k + i + jj_ord;

                    // save at[k1], bt[k1] for k1 = kk+1..kk+kspan
                    k1 = kk + kspan;
                    let mut k2_save = 1;
                    while k1 != kk {
                        AT!(k2_save) = A!(k1);
                        BT!(k2_save) = B!(k1);
                        k1 -= inc;
                        k2_save += 1;
                    }

                    loop {
                        k1 = kk + kspan;
                        k2 = k1 - jc * (k + NP!(k));
                        k = -NP!(k);
                        loop {
                            A!(k1) = A!(k2);
                            B!(k1) = B!(k2);
                            k1 -= inc;
                            k2 -= inc;
                            if k1 != kk {
                                continue;
                            }
                            break;
                        }
                        kk = k2;
                        if k != j {
                            continue;
                        }
                        break;
                    }

                    // restore from at/bt
                    k1 = kk + kspan;
                    k2_save = 1;
                    while k1 > kk {
                        A!(k1) = AT!(k2_save);
                        B!(k1) = BT!(k2_save);
                        k1 -= inc;
                        k2_save += 1;
                    }

                    if jj_ord != 0 {
                        continue; // goto L520
                    }
                    break;
                }
                if j != 1 {
                    continue; // goto L_ord
                }
                break;
            }

            // L570:
            j = k3_cycle + 1;
            nt = nt - kspnn;
            i = nt - inc + 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::instance::{RInstance, replace_current_instance};

    #[test]
    fn fft_factorization_state_is_session_local() {
        let mut first = RInstance::new();
        let mut second = RInstance::new();

        unsafe {
            let previous = replace_current_instance(Some(&mut first as *mut RInstance));
            let mut maxf = 0;
            let mut maxp = 0;
            fft_factor(12, &mut maxf, &mut maxp);
            assert_eq!(first.fft_state.old_n, 12);
            assert!(first.fft_state.m_fac > 0);
            replace_current_instance(previous);

            let previous = replace_current_instance(Some(&mut second as *mut RInstance));
            assert_eq!(second.fft_state.old_n, 0);
            fft_factor(5, &mut maxf, &mut maxp);
            assert_eq!(second.fft_state.old_n, 5);
            replace_current_instance(previous);
        }

        assert_eq!(first.fft_state.old_n, 12);
        assert_eq!(second.fft_state.old_n, 5);
    }
}
