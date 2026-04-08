#![allow(unused_variables, clippy::manual_memcpy)]
#![allow(unused_assignments)]
// Ported from R's appl/integrate.c
//
// C translations of Fortran routines from QUADPACK:
//   QUADPACK is part of SLATEC 'and therefore in the public domain'.
//
// Rdqagi: Integration over infinite intervals
// Rdqags: Integration over finite intervals

use crate::utils::*;
use libm::*;

// =====================================================================
// Callback type: vectorizing integrand function
// f(x[1..n], ex) overwrites x[] with f(x[1..n])
// =====================================================================

/// Type alias for the integrand callback function.
/// The function receives a mutable slice of x values and an opaque pointer.
/// It should overwrite x[i] with f(x[i]).
pub type IntegrFn =
    unsafe extern "C" fn(x: *mut f64, n: std::os::raw::c_int, ex: *mut std::ffi::c_void);

// =====================================================================
// Constants
// =====================================================================

const DBL_EPSILON: f64 = 2.220446049250313e-16;
const DBL_MIN: f64 = 2.2250738585072014e-308;
const DBL_MAX: f64 = 1.7976931348623157e+308;

// =====================================================================
// Internal QUADPACK functions
// =====================================================================

/// Gauss-Kronrod 15-point rule for infinite intervals
/// Maps (0,1) subinterval, computes integral of transformed integrand.
fn rdqk15i(
    f: IntegrFn,
    ex: *mut std::ffi::c_void,
    boun: f64,
    inf: i32,
    a: f64,
    b: f64,
) -> (f64, f64, f64, f64) {
    // (result, abserr, resabs, resasc)

    // Gauss weights (7-point)
    const WG: [f64; 8] = [
        0.,
        0.129484966168869693270611432679082,
        0.,
        0.27970539148927666790146777142378,
        0.,
        0.381830050505118944950369775488975,
        0.,
        0.417959183673469387755102040816327,
    ];

    // Kronrod abscissae (15-point)
    const XGK: [f64; 8] = [
        0.991455371120812639206854697526329,
        0.949107912342758524526189684047851,
        0.864864423359769072789712788640926,
        0.741531185599394439863864773280788,
        0.58608723546769113029414483825873,
        0.405845151377397166906606412076961,
        0.207784955007898467600689403773245,
        0.,
    ];

    // Kronrod weights (15-point)
    const WGK: [f64; 8] = [
        0.02293532201052922496373200805897,
        0.063092092629978553290700663189204,
        0.104790010322250183839876322541518,
        0.140653259715525918745189590510238,
        0.16900472663926790282658342659855,
        0.190350578064785409913256402421014,
        0.204432940075298892414161999234649,
        0.209482141084727828012999174891714,
    ];

    let epmach = DBL_EPSILON;
    let uflow = DBL_MIN;
    let dinf = if 1 < inf { 1.0 } else { inf as f64 };

    let centr = (a + b) * 0.5;
    let hlgth = (b - a) * 0.5;
    let mut tabsc1 = boun + dinf * (1.0 - centr) / centr;

    let mut vec = [0.0f64; 15];
    let mut vec2 = [0.0f64; 15];

    vec[0] = tabsc1;
    if inf == 2 {
        vec2[0] = -tabsc1;
    }

    for j in 1..=7 {
        let absc = hlgth * XGK[j - 1];
        let absc1 = centr - absc;
        let absc2 = centr + absc;
        tabsc1 = boun + dinf * (1.0 - absc1) / absc1;
        let tabsc2 = boun + dinf * (1.0 - absc2) / absc2;
        vec[(j << 1) - 1] = tabsc1;
        vec[j * 2] = tabsc2;
        if inf == 2 {
            vec2[(j << 1) - 1] = -tabsc1;
            vec2[j * 2] = -tabsc2;
        }
    }

    unsafe {
        f(vec.as_mut_ptr(), 15, ex);
    }
    if inf == 2 {
        unsafe {
            f(vec2.as_mut_ptr(), 15, ex);
        }
    }

    let mut fval1 = vec[0];
    if inf == 2 {
        fval1 += vec2[0];
    }
    let fc = fval1 / centr / centr;

    let mut resg = WG[7] * fc;
    let mut resk = WGK[7] * fc;
    let mut resabs = fabs(resk);

    let mut fv1 = [0.0f64; 7];
    let mut fv2 = [0.0f64; 7];

    for j in 1..=7 {
        let absc = hlgth * XGK[j - 1];
        let absc1 = centr - absc;
        let absc2 = centr + absc;
        tabsc1 = boun + dinf * (1.0 - absc1) / absc1;
        let _tabsc2 = boun + dinf * (1.0 - absc2) / absc2;
        fval1 = vec[(j << 1) - 1];
        let mut fval2 = vec[j * 2];
        if inf == 2 {
            fval1 += vec2[(j << 1) - 1];
        }
        if inf == 2 {
            fval2 += vec2[j * 2];
        }
        fval1 = fval1 / absc1 / absc1;
        fval2 = fval2 / absc2 / absc2;
        fv1[j - 1] = fval1;
        fv2[j - 1] = fval2;
        let fsum = fval1 + fval2;
        resg += WG[j - 1] * fsum;
        resk += WGK[j - 1] * fsum;
        resabs += WGK[j - 1] * (fabs(fval1) + fabs(fval2));
    }

    let reskh = resk * 0.5;
    let mut resasc = WGK[7] * fabs(fc - reskh);
    for j in 1..=7 {
        resasc += WGK[j - 1] * (fabs(fv1[j - 1] - reskh) + fabs(fv2[j - 1] - reskh));
    }

    let result = resk * hlgth;
    resasc *= hlgth;
    resabs *= hlgth;
    let mut abserr = fabs((resk - resg) * hlgth);

    if resasc != 0.0 && abserr != 0.0 {
        abserr = resasc * fmin2(1.0, pow(abserr * 200.0 / resasc, 1.5));
    }
    if resabs > uflow / (epmach * 50.0) {
        abserr = fmax2(epmach * 50.0 * resabs, abserr);
    }

    (result, abserr, resabs, resasc)
}

/// Epsilon algorithm for convergence acceleration (extrapolation)
fn rdqelg(
    n: &mut i32,
    epstab: &mut [f64; 52],
    result: &mut f64,
    abserr: &mut f64,
    res3la: &mut [f64; 3],
    nres: &mut i32,
) {
    let epmach = DBL_EPSILON;
    let oflow = DBL_MAX;

    *nres += 1;
    *abserr = oflow;
    *result = epstab[*n as usize];

    if *n < 3 {
        *abserr = fmax2(*abserr, epmach * 5.0 * fabs(*result));
        return;
    }

    let limexp: i32 = 50;
    epstab[(*n + 2) as usize] = epstab[*n as usize];
    let newelm = (*n - 1) / 2;
    epstab[*n as usize] = oflow;
    let num = *n;
    let mut k1 = *n;

    for i in 1..=newelm {
        let k2 = k1 - 1;
        let k3 = k1 - 2;
        let mut res = epstab[(k1 + 2) as usize];
        let e0 = epstab[k3 as usize];
        let e1 = epstab[k2 as usize];
        let e2 = res;
        let e1abs = fabs(e1);
        let delta2 = e2 - e1;
        let err2 = fabs(delta2);
        let tol2 = fmax2(fabs(e2), e1abs) * epmach;
        let delta3 = e1 - e0;
        let err3 = fabs(delta3);
        let tol3 = fmax2(e1abs, fabs(e0)) * epmach;

        if err2 <= tol2 && err3 <= tol3 {
            *result = res;
            *abserr = err2 + err3;
            *abserr = fmax2(*abserr, epmach * 5.0 * fabs(*result));
            return;
        }

        let e3 = epstab[k1 as usize];
        epstab[k1 as usize] = e1;
        let delta1 = e1 - e3;
        let err1 = fabs(delta1);
        let tol1 = fmax2(e1abs, fabs(e3)) * epmach;

        if err1 > tol1 && err2 > tol2 && err3 > tol3 {
            let ss = 1.0 / delta1 + 1.0 / delta2 - 1.0 / delta3;
            let epsinf = fabs(ss * e1);

            if epsinf > 1e-4 {
                // Compute new element
                res = e1 + 1.0 / ss;
                epstab[k1 as usize] = res;
                k1 -= 2;
                let err_a = err2 + fabs(res - e2) + err3;
                if err_a <= *abserr {
                    *abserr = err_a;
                    *result = res;
                }
                continue;
            }
        }

        *n = i + i - 1;
        break;
    }

    // Shift the table
    if *n == limexp {
        *n = (limexp / 2) * 2 - 1;
    }

    let ib = if num / 2 * 2 == num { 2 } else { 1 };
    let ie = newelm + 1;
    let mut ib = ib as i32;
    for _i in 1..=ie {
        let ib2 = ib + 2;
        epstab[ib as usize] = epstab[ib2 as usize];
        ib = ib2;
    }

    if num != *n {
        let mut indx = num - *n + 1;
        for i in 1..=*n {
            epstab[i as usize] = epstab[indx as usize];
            indx += 1;
        }
    }

    if *nres >= 4 {
        *abserr = fabs(*result - res3la[2]) + fabs(*result - res3la[1]) + fabs(*result - res3la[0]);
        res3la[0] = res3la[1];
        res3la[1] = res3la[2];
        res3la[2] = *result;
    } else {
        res3la[(*nres - 1) as usize] = *result;
        *abserr = oflow;
    }

    *abserr = fmax2(*abserr, epmach * 5.0 * fabs(*result));
}

/// Sorting routine to maintain descending ordering in error estimates
/// maxerr is 0-indexed (unlike C which is 1-indexed), iord values are 1-indexed
fn rdqpsrt(
    limit: i32,
    last: i32,
    maxerr: &mut usize,
    ermax: &mut f64,
    elist: &mut [f64],
    iord: &mut [i32],
    nrmax: &mut i32,
) {
    if last <= 2 {
        iord[0] = 1;
        iord[1] = 2;
        *maxerr = (iord[(*nrmax - 1) as usize] - 1) as usize;
        *ermax = elist[*maxerr];
        return;
    }

    let errmax = elist[*maxerr];
    if *nrmax > 1 {
        let ido = *nrmax - 1;
        for _i in 1..=ido {
            let isucc = iord[(*nrmax - 1) as usize];
            if errmax <= elist[(isucc - 1) as usize] {
                break;
            }
            iord[(*nrmax - 1) as usize] = isucc;
            *nrmax -= 1;
        }
    }

    let jupbnd = if last > limit / 2 + 2 {
        limit + 3 - last
    } else {
        last
    };

    let errmin = elist[(last - 1) as usize];

    let jbnd = jupbnd - 1;
    for i in (*nrmax + 1)..=jbnd {
        let isucc = iord[(i - 1) as usize];
        if errmax >= elist[(isucc - 1) as usize] {
            iord[(i - 2) as usize] = (*maxerr + 1) as i32;
            let mut k = jbnd;
            for _j in i..=jbnd {
                let isucc = iord[(k - 1) as usize];
                if errmin < elist[(isucc - 1) as usize] {
                    iord[k as usize] = last;
                    *maxerr = (iord[(*nrmax - 1) as usize] - 1) as usize;
                    *ermax = elist[*maxerr];
                    return;
                }
                iord[k as usize] = isucc;
                k -= 1;
            }
            iord[(i - 1) as usize] = last;
            *maxerr = (iord[(*nrmax - 1) as usize] - 1) as usize;
            *ermax = elist[*maxerr];
            return;
        }
        iord[(i - 2) as usize] = isucc;
    }

    iord[(jbnd - 1) as usize] = (*maxerr + 1) as i32;
    iord[(jupbnd - 1) as usize] = last;

    *maxerr = (iord[(*nrmax - 1) as usize] - 1) as usize;
    *ermax = elist[*maxerr];
}

/// Gauss-Kronrod 21-point rule for finite intervals
fn rdqk21(f: IntegrFn, ex: *mut std::ffi::c_void, a: f64, b: f64) -> (f64, f64, f64, f64) {
    // (result, abserr, resabs, resasc)

    // Gauss weights (10-point)
    const WG: [f64; 5] = [
        0.066671344308688137593568809893332,
        0.149451349150580593145776339657697,
        0.219086362515982043995534934228163,
        0.269266719309996355091226921569469,
        0.295524224714752870173892994651338,
    ];

    // Kronrod abscissae (21-point)
    const XGK: [f64; 11] = [
        0.995657163025808080735527280689003,
        0.973906528517171720077964012084452,
        0.930157491355708226001207180059508,
        0.865063366688984510732096688423493,
        0.780817726586416897063717578345042,
        0.679409568299024406234327365114874,
        0.562757134668604683339000099272694,
        0.433395394129247190799265943165784,
        0.294392862701460198131126603103866,
        0.14887433898163121088482600112972,
        0.0,
    ];

    // Kronrod weights (21-point)
    const WGK: [f64; 11] = [
        0.011694638867371874278064396062192,
        0.03255816230796472747881897245939,
        0.05475589657435199603138130024458,
        0.07503967481091995276704314091619,
        0.093125454583697605535065465083366,
        0.109387158802297641899210590325805,
        0.123491976262065851077958109831074,
        0.134709217311473325928054001771707,
        0.142775938577060080797094273138717,
        0.147739104901338491374841515972068,
        0.149445554002916905664936468389821,
    ];

    let epmach = DBL_EPSILON;
    let uflow = DBL_MIN;

    let centr = (a + b) * 0.5;
    let hlgth = (b - a) * 0.5;
    let dhlgth = fabs(hlgth);

    let mut resg = 0.0;
    let mut vec = [0.0f64; 21];
    vec[0] = centr;

    for j in 1..=5 {
        let jtw = j << 1;
        let absc = hlgth * XGK[jtw - 1];
        vec[(j << 1) - 1] = centr - absc;
        vec[j * 2] = centr + absc;
    }
    for j in 1..=5 {
        let jtwm1 = (j << 1) - 1;
        let absc = hlgth * XGK[jtwm1 - 1];
        vec[(j << 1) + 9] = centr - absc;
        vec[(j << 1) + 10] = centr + absc;
    }

    unsafe {
        f(vec.as_mut_ptr(), 21, ex);
    }

    let fc = vec[0];
    let mut resk = WGK[10] * fc;
    let mut resabs = fabs(resk);

    let mut fv1 = [0.0f64; 10];
    let mut fv2 = [0.0f64; 10];

    for j in 1..=5 {
        let jtw = j << 1;
        let _absc = hlgth * XGK[jtw - 1];
        let fval1 = vec[(j << 1) - 1];
        let fval2 = vec[j * 2];
        fv1[jtw - 1] = fval1;
        fv2[jtw - 1] = fval2;
        let fsum = fval1 + fval2;
        resg += WG[j - 1] * fsum;
        resk += WGK[jtw - 1] * fsum;
        resabs += WGK[jtw - 1] * (fabs(fval1) + fabs(fval2));
    }
    for j in 1..=5 {
        let jtwm1 = (j << 1) - 1;
        let _absc = hlgth * XGK[jtwm1 - 1];
        let fval1 = vec[(j << 1) + 9];
        let fval2 = vec[(j << 1) + 10];
        fv1[jtwm1 - 1] = fval1;
        fv2[jtwm1 - 1] = fval2;
        let fsum = fval1 + fval2;
        resk += WGK[jtwm1 - 1] * fsum;
        resabs += WGK[jtwm1 - 1] * (fabs(fval1) + fabs(fval2));
    }

    let reskh = resk * 0.5;
    let mut resasc = WGK[10] * fabs(fc - reskh);
    for j in 1..=10 {
        resasc += WGK[j - 1] * (fabs(fv1[j - 1] - reskh) + fabs(fv2[j - 1] - reskh));
    }

    let result = resk * hlgth;
    resasc *= dhlgth;
    resabs *= dhlgth;
    let mut abserr = fabs((resk - resg) * hlgth);

    if resasc != 0.0 && abserr != 0.0 {
        abserr = resasc * fmin2(1.0, pow(abserr * 200.0 / resasc, 1.5));
    }
    if resabs > uflow / (epmach * 50.0) {
        abserr = fmax2(epmach * 50.0 * resabs, abserr);
    }

    (result, abserr, resabs, resasc)
}

// =====================================================================
// Internal adaptive integration for infinite intervals (QAGIE)
// =====================================================================

fn rdqagie(
    f: IntegrFn,
    ex: *mut std::ffi::c_void,
    bound: f64,
    inf: i32,
    epsabs: f64,
    epsrel: f64,
    limit: i32,
    result: &mut f64,
    abserr: &mut f64,
    neval: &mut i32,
    ier: &mut i32,
    alist: &mut [f64],
    blist: &mut [f64],
    rlist: &mut [f64],
    elist: &mut [f64],
    iord: &mut [i32],
    last: &mut i32,
) {
    let epmach = DBL_EPSILON;
    let uflow = DBL_MIN;
    let oflow = DBL_MAX;

    *ier = 0;
    *neval = 0;
    *last = 0;
    *result = 0.0;
    *abserr = 0.0;
    alist[0] = 0.0;
    blist[0] = 1.0;
    rlist[0] = 0.0;
    elist[0] = 0.0;
    iord[0] = 0;

    if epsabs <= 0.0 && epsrel < fmax2(epmach * 50.0, 5e-29) {
        *ier = 6;
        return;
    }

    let boun = if inf == 2 { 0.0 } else { bound };

    let (res, err, defabs, resabs) = rdqk15i(f, ex, boun, inf, 0.0, 1.0);
    *result = res;
    *abserr = err;

    *last = 1;
    rlist[0] = *result;
    elist[0] = *abserr;
    iord[0] = 1;

    let dres = fabs(*result);
    let mut errbnd = fmax2(epsabs, epsrel * dres);

    if *abserr <= epmach * 100.0 * defabs && *abserr > errbnd {
        *ier = 2;
    }
    if limit == 1 {
        *ier = 1;
    }
    if *ier != 0 || (*abserr <= errbnd && *abserr != resabs) || *abserr == 0.0 {
        // L130
        *neval = *last * 30 - 15;
        if inf == 2 {
            *neval <<= 1;
        }
        if *ier > 2 {
            *ier -= 1;
        }
        return;
    }

    // Initialization
    let mut rlist2 = [0.0f64; 52];
    rlist2[0] = *result;
    let mut errmax = *abserr;
    let mut maxerr: usize = 0; // 0-indexed
    let mut area = *result;
    let mut errsum = *abserr;
    *abserr = oflow;
    let mut nrmax: i32 = 1;
    let mut nres: i32 = 0;
    let mut numrl2: i32 = 2;
    let mut ktmin: i32 = 0;
    let mut extrap = false;
    let mut noext = false;
    let mut ierro: i32 = 0;
    let mut iroff1: i32 = 0;
    let mut iroff2: i32 = 0;
    let mut iroff3: i32 = 0;
    let ksgn: i32;
    let defabs_val = defabs;

    ksgn = if dres >= (1.0 - epmach * 50.0) * defabs_val {
        1
    } else {
        -1
    };

    let mut correc = 0.0;
    let mut erlarg = 0.0;
    let mut ertest = 0.0;
    let mut small = 0.0;

    // Main loop
    *last = 1;
    while *last < limit {
        *last += 1;

        let a1 = alist[maxerr];
        let b1 = (alist[maxerr] + blist[maxerr]) * 0.5;
        let a2 = b1;
        let b2 = blist[maxerr];
        let erlast = errmax;

        let (area1, error1, _, defab1) = rdqk15i(f, ex, boun, inf, a1, b1);
        let (area2, error2, _, defab2) = rdqk15i(f, ex, boun, inf, a2, b2);

        let area12 = area1 + area2;
        let erro12 = error1 + error2;
        errsum = errsum + erro12 - errmax;
        area = area + area12 - rlist[maxerr];

        if !(defab1 == error1 || defab2 == error2) {
            if fabs(rlist[maxerr] - area12) <= fabs(area12) * 1e-5 && erro12 >= errmax * 0.99 {
                if extrap {
                    iroff2 += 1;
                } else {
                    iroff1 += 1;
                }
            }
            if *last > 10 && erro12 > errmax {
                iroff3 += 1;
            }
        }

        rlist[maxerr] = area1;
        rlist[(*last - 1) as usize] = area2;
        errbnd = fmax2(epsabs, epsrel * fabs(area));

        if iroff1 + iroff2 >= 10 || iroff3 >= 20 {
            *ier = 2;
        }
        if iroff2 >= 5 {
            ierro = 3;
        }
        if *last == limit {
            *ier = 1;
        }
        if fmax2(fabs(a1), fabs(b2)) <= (epmach * 100.0 + 1.0) * (fabs(a2) + uflow * 1e3) {
            *ier = 4;
        }

        if error2 <= error1 {
            alist[(*last - 1) as usize] = a2;
            blist[maxerr] = b1;
            blist[(*last - 1) as usize] = b2;
            elist[maxerr] = error1;
            elist[(*last - 1) as usize] = error2;
        } else {
            alist[maxerr] = a2;
            alist[(*last - 1) as usize] = a1;
            blist[(*last - 1) as usize] = b1;
            rlist[maxerr] = area2;
            rlist[(*last - 1) as usize] = area1;
            elist[maxerr] = error2;
            elist[(*last - 1) as usize] = error1;
        }

        rdqpsrt(
            limit,
            *last,
            &mut maxerr,
            &mut errmax,
            elist,
            iord,
            &mut nrmax,
        );

        if errsum <= errbnd {
            break;
        }
        if *ier != 0 {
            break;
        }
        if *last == 2 {
            small = 0.375;
            erlarg = errsum;
            ertest = errbnd;
            rlist2[1] = area;
            continue;
        }
        if noext {
            continue;
        }

        erlarg -= erlast;
        if fabs(b1 - a1) > small {
            erlarg += erro12;
        }
        if !extrap {
            if fabs(blist[maxerr] - alist[maxerr]) > small {
                continue;
            }
            extrap = true;
            nrmax = 2;
        }

        if ierro != 3 && erlarg > ertest {
            let id = nrmax;
            let jupbnd = if *last > limit / 2 + 2 {
                limit + 3 - *last
            } else {
                *last
            };
            let mut found_large = false;
            for _k in id..=jupbnd {
                maxerr = (iord[(nrmax - 1) as usize] - 1) as usize;
                errmax = elist[maxerr];
                if fabs(blist[maxerr] - alist[maxerr]) > small {
                    found_large = true;
                    break;
                }
                nrmax += 1;
            }
            if found_large {
                continue;
            }
        }

        // Perform extrapolation
        numrl2 += 1;
        rlist2[(numrl2 - 1) as usize] = area;
        let mut res3la = [0.0f64; 3];
        rdqelg(
            &mut numrl2,
            &mut rlist2,
            result,
            abserr,
            &mut res3la,
            &mut nres,
        );
        ktmin += 1;
        if ktmin > 5 && *abserr < errsum * 0.001 {
            *ier = 5;
        }
        if *abserr >= erlarg {
            // L70
            if numrl2 == 1 {
                noext = true;
            }
            if *ier == 5 {
                break;
            }
            maxerr = (iord[0] - 1) as usize;
            errmax = elist[maxerr];
            nrmax = 1;
            extrap = false;
            small *= 0.5;
            erlarg = errsum;
            continue;
        }
        ktmin = 0;
        correc = erlarg;
        ertest = fmax2(epsabs, epsrel * fabs(*result));
        if *abserr <= ertest {
            break;
        }

        // L70
        if numrl2 == 1 {
            noext = true;
        }
        if *ier == 5 {
            break;
        }
        maxerr = (iord[0] - 1) as usize;
        errmax = elist[maxerr];
        nrmax = 1;
        extrap = false;
        small *= 0.5;
        erlarg = errsum;
    }

    // Set final result and error estimate
    if *abserr == oflow {
        // L115: compute global integral sum
        *result = 0.0;
        for k in 0..*last as usize {
            *result += rlist[k];
        }
        *abserr = errsum;
        // L130
        *neval = *last * 30 - 15;
        if inf == 2 {
            *neval <<= 1;
        }
        if *ier > 2 {
            *ier -= 1;
        }
        return;
    }

    if *ier + ierro == 0 {
        // L110: test on divergence
        if !(ksgn == -1 && fmax2(fabs(*result), fabs(area)) <= defabs_val * 0.01)
            && (0.01 > *result / area || *result / area > 100.0 || errsum > fabs(area))
        {
            *ier = 6;
        }
        // L130
        *neval = *last * 30 - 15;
        if inf == 2 {
            *neval <<= 1;
        }
        if *ier > 2 {
            *ier -= 1;
        }
        return;
    }

    if ierro == 3 {
        *abserr += correc;
    }
    if *ier == 0 {
        *ier = 3;
    }
    if *result == 0.0 || area == 0.0 {
        if *abserr > errsum {
            // L115
            *result = 0.0;
            for k in 0..*last as usize {
                *result += rlist[k];
            }
            *abserr = errsum;
            // L130
            *neval = *last * 30 - 15;
            if inf == 2 {
                *neval <<= 1;
            }
            if *ier > 2 {
                *ier -= 1;
            }
            return;
        }
        if area == 0.0 {
            // L130
            *neval = *last * 30 - 15;
            if inf == 2 {
                *neval <<= 1;
            }
            if *ier > 2 {
                *ier -= 1;
            }
            return;
        }
    } else {
        if *abserr / fabs(*result) > errsum / fabs(area) {
            // L115
            *result = 0.0;
            for k in 0..*last as usize {
                *result += rlist[k];
            }
            *abserr = errsum;
            // L130
            *neval = *last * 30 - 15;
            if inf == 2 {
                *neval <<= 1;
            }
            if *ier > 2 {
                *ier -= 1;
            }
            return;
        }
    }

    // L110: test on divergence
    if !(ksgn == -1 && fmax2(fabs(*result), fabs(area)) <= defabs_val * 0.01)
        && (0.01 > *result / area || *result / area > 100.0 || errsum > fabs(area))
    {
        *ier = 6;
    }

    // L130
    *neval = *last * 30 - 15;
    if inf == 2 {
        *neval <<= 1;
    }
    if *ier > 2 {
        *ier -= 1;
    }
}

// =====================================================================
// Internal adaptive integration for finite intervals (QAGSE)
// =====================================================================

fn rdqagse(
    f: IntegrFn,
    ex: *mut std::ffi::c_void,
    a: f64,
    b: f64,
    epsabs: f64,
    epsrel: f64,
    limit: i32,
    result: &mut f64,
    abserr: &mut f64,
    neval: &mut i32,
    ier: &mut i32,
    alist: &mut [f64],
    blist: &mut [f64],
    rlist: &mut [f64],
    elist: &mut [f64],
    iord: &mut [i32],
    last: &mut i32,
) {
    let epmach = DBL_EPSILON;
    let uflow = DBL_MIN;
    let oflow = DBL_MAX;

    *ier = 0;
    *neval = 0;
    *last = 0;
    *result = 0.0;
    *abserr = 0.0;
    alist[0] = a;
    blist[0] = b;
    rlist[0] = 0.0;
    elist[0] = 0.0;

    if epsabs <= 0.0 && epsrel < fmax2(epmach * 50.0, 5e-29) {
        *ier = 6;
        return;
    }

    let (res, err, defabs, _resasc) = rdqk21(f, ex, a, b);
    *result = res;
    *abserr = err;

    let dres = fabs(*result);
    let mut errbnd = fmax2(epsabs, epsrel * dres);
    *last = 1;
    rlist[0] = *result;
    elist[0] = *abserr;
    iord[0] = 1;

    if *abserr <= epmach * 100.0 * defabs && *abserr > errbnd {
        *ier = 2;
    }
    if limit == 1 {
        *ier = 1;
    }
    if *ier != 0 || (*abserr <= errbnd && *abserr != defabs) || *abserr == 0.0 {
        *neval = *last * 42 - 21;
        return;
    }

    // Initialization
    let mut rlist2 = [0.0f64; 52];
    rlist2[0] = *result;
    let mut errmax = *abserr;
    let mut maxerr: usize = 0; // 0-indexed
    let mut area = *result;
    let mut errsum = *abserr;
    *abserr = oflow;
    let mut nrmax: i32 = 1;
    let mut nres: i32 = 0;
    let mut numrl2: i32 = 2;
    let mut ktmin: i32 = 0;
    let mut extrap = false;
    let mut noext = false;
    let mut ierro: i32 = 0;
    let mut iroff1: i32 = 0;
    let mut iroff2: i32 = 0;
    let mut iroff3: i32 = 0;
    let ksgn: i32;
    let defabs_val = defabs;

    ksgn = if dres >= (1.0 - epmach * 50.0) * defabs_val {
        1
    } else {
        -1
    };

    let mut correc = 0.0;
    let mut erlarg = 0.0;
    let mut ertest = 0.0;
    let mut small = 0.0;

    // Main loop
    *last = 1;
    while *last < limit {
        *last += 1;

        let a1 = alist[maxerr];
        let b1 = (alist[maxerr] + blist[maxerr]) * 0.5;
        let a2 = b1;
        let b2 = blist[maxerr];
        let erlast = errmax;

        let (area1, error1, _, defab1) = rdqk21(f, ex, a1, b1);
        let (area2, error2, _, defab2) = rdqk21(f, ex, a2, b2);

        let area12 = area1 + area2;
        let erro12 = error1 + error2;
        errsum = errsum + erro12 - errmax;
        area = area + area12 - rlist[maxerr];

        if !(defab1 == error1 || defab2 == error2) {
            if fabs(rlist[maxerr] - area12) <= fabs(area12) * 1e-5 && erro12 >= errmax * 0.99 {
                if extrap {
                    iroff2 += 1;
                } else {
                    iroff1 += 1;
                }
            }
            if *last > 10 && erro12 > errmax {
                iroff3 += 1;
            }
        }

        rlist[maxerr] = area1;
        rlist[(*last - 1) as usize] = area2;
        errbnd = fmax2(epsabs, epsrel * fabs(area));

        if iroff1 + iroff2 >= 10 || iroff3 >= 20 {
            *ier = 2;
        }
        if iroff2 >= 5 {
            ierro = 3;
        }
        if *last == limit {
            *ier = 1;
        }
        if fmax2(fabs(a1), fabs(b2)) <= (epmach * 100.0 + 1.0) * (fabs(a2) + uflow * 1e3) {
            *ier = 4;
        }

        if error2 > error1 {
            alist[maxerr] = a2;
            alist[(*last - 1) as usize] = a1;
            blist[(*last - 1) as usize] = b1;
            rlist[maxerr] = area2;
            rlist[(*last - 1) as usize] = area1;
            elist[maxerr] = error2;
            elist[(*last - 1) as usize] = error1;
        } else {
            alist[(*last - 1) as usize] = a2;
            blist[maxerr] = b1;
            blist[(*last - 1) as usize] = b2;
            elist[maxerr] = error1;
            elist[(*last - 1) as usize] = error2;
        }

        rdqpsrt(
            limit,
            *last,
            &mut maxerr,
            &mut errmax,
            elist,
            iord,
            &mut nrmax,
        );

        if errsum <= errbnd {
            break;
        }
        if *ier != 0 {
            break;
        }
        if *last == 2 {
            small = fabs(b - a) * 0.375;
            erlarg = errsum;
            ertest = errbnd;
            rlist2[1] = area;
            continue;
        }
        if noext {
            continue;
        }

        erlarg -= erlast;
        if fabs(b1 - a1) > small {
            erlarg += erro12;
        }
        if !extrap {
            if fabs(blist[maxerr] - alist[maxerr]) > small {
                continue;
            }
            extrap = true;
            nrmax = 2;
        }

        if ierro != 3 && erlarg > ertest {
            let id = nrmax;
            let jupbnd = if *last > limit / 2 + 2 {
                limit + 3 - *last
            } else {
                *last
            };
            let mut found_large = false;
            for _k in id..=jupbnd {
                maxerr = (iord[(nrmax - 1) as usize] - 1) as usize;
                errmax = elist[maxerr];
                if fabs(blist[maxerr] - alist[maxerr]) > small {
                    found_large = true;
                    break;
                }
                nrmax += 1;
            }
            if found_large {
                continue;
            }
        }

        // Perform extrapolation
        numrl2 += 1;
        rlist2[(numrl2 - 1) as usize] = area;
        let mut res3la = [0.0f64; 3];
        rdqelg(
            &mut numrl2,
            &mut rlist2,
            result,
            abserr,
            &mut res3la,
            &mut nres,
        );
        ktmin += 1;
        if ktmin > 5 && *abserr < errsum * 0.001 {
            *ier = 5;
        }
        if *abserr < erlarg {
            ktmin = 0;
            correc = erlarg;
            ertest = fmax2(epsabs, epsrel * fabs(*result));
            if *abserr <= ertest {
                break;
            }
        }

        // L70: prepare bisection of the smallest interval
        if numrl2 == 1 {
            noext = true;
        }
        if *ier == 5 {
            break;
        }
        maxerr = (iord[0] - 1) as usize;
        errmax = elist[maxerr];
        nrmax = 1;
        extrap = false;
        small *= 0.5;
        erlarg = errsum;
    }

    // Set final result and error estimate
    if *abserr == oflow {
        // L115: compute global integral sum
        *result = 0.0;
        for k in 0..*last as usize {
            *result += rlist[k];
        }
        *abserr = errsum;
        *neval = *last * 42 - 21;
        return;
    }

    if *ier + ierro != 0 {
        if ierro == 3 {
            *abserr += correc;
        }
        if *ier == 0 {
            *ier = 3;
        }
        if *result == 0.0 || area == 0.0 {
            if *abserr > errsum {
                // L115
                *result = 0.0;
                for k in 0..*last as usize {
                    *result += rlist[k];
                }
                *abserr = errsum;
                *neval = *last * 42 - 21;
                return;
            }
            if area == 0.0 {
                *neval = *last * 42 - 21;
                return;
            }
        } else {
            if *abserr / fabs(*result) > errsum / fabs(area) {
                // L115
                *result = 0.0;
                for k in 0..*last as usize {
                    *result += rlist[k];
                }
                *abserr = errsum;
                *neval = *last * 42 - 21;
                return;
            }
        }
    }

    // L110: test on divergence
    if !(ksgn == -1 && fmax2(fabs(*result), fabs(area)) <= defabs_val * 0.01)
        && (0.01 > *result / area || *result / area > 100.0 || errsum > fabs(area))
    {
        *ier = 5;
    }

    *neval = *last * 42 - 21;
}

// =====================================================================
// Public API
// =====================================================================

/// Integration over infinite intervals (QAGI from QUADPACK).
///
/// Computes an approximation to the integral of f over:
/// - (bound, +infinity) when inf = 1
/// - (-infinity, bound) when inf = -1
/// - (-infinity, +infinity) when inf = 2
///
/// # Arguments
/// * `f` - vectorizing integrand function
/// * `ex` - opaque pointer passed to f
/// * `bound` - finite bound of integration range
/// * `inf` - type of infinite interval (1, -1, or 2)
/// * `epsabs` - absolute accuracy requested
/// * `epsrel` - relative accuracy requested
/// * `result` - approximation to the integral (output)
/// * `abserr` - estimate of absolute error (output)
/// * `neval` - number of integrand evaluations (output)
/// * `ier` - error code (output)
/// * `limit` - maximum number of subintervals
/// * `lenw` - dimension of work (must be >= limit*4)
/// * `last` - number of subintervals produced (output)
/// * `iwork` - integer work array of dimension limit
/// * `work` - double work array of dimension lenw
pub extern "C" fn Rdqagi(
    f: IntegrFn,
    ex: *mut std::ffi::c_void,
    bound: *mut f64,
    inf: *mut std::os::raw::c_int,
    epsabs: *mut f64,
    epsrel: *mut f64,
    result: *mut f64,
    abserr: *mut f64,
    neval: *mut std::os::raw::c_int,
    ier: *mut std::os::raw::c_int,
    limit: *mut std::os::raw::c_int,
    lenw: *mut std::os::raw::c_int,
    last: *mut std::os::raw::c_int,
    iwork: *mut std::os::raw::c_int,
    work: *mut f64,
) {
    let bound_val = unsafe { *bound };
    let inf_val = unsafe { *inf };
    let epsabs_val = unsafe { *epsabs };
    let epsrel_val = unsafe { *epsrel };
    let limit_val = unsafe { *limit };
    let lenw_val = unsafe { *lenw };

    unsafe {
        *ier = 6;
        *neval = 0;
        *last = 0;
        *result = 0.0;
        *abserr = 0.0;
    }

    if limit_val < 1 || lenw_val < limit_val * 4 {
        return;
    }

    let l1 = limit_val as usize;
    let l2 = l1 + l1;
    let l3 = l2 + l1;

    let work_slice = unsafe { std::slice::from_raw_parts_mut(work, lenw_val as usize) };
    let iwork_slice = unsafe { std::slice::from_raw_parts_mut(iwork, limit_val as usize) };

    // Copy into separate vectors to avoid borrow checker issues with overlapping slices
    let mut alist = vec![0.0f64; l1];
    let mut blist = vec![0.0f64; l1];
    let mut rlist = vec![0.0f64; l1];
    let mut elist = vec![0.0f64; l1];
    let mut iord = vec![0i32; l1];

    for i in 0..l1 {
        alist[i] = work_slice[i];
        blist[i] = work_slice[l1 + i];
        rlist[i] = work_slice[l2 + i];
        elist[i] = work_slice[l3 + i];
        iord[i] = iwork_slice[i];
    }

    let mut result_val = 0.0;
    let mut abserr_val = 0.0;
    let mut neval_val = 0;
    let mut ier_val = 0;
    let mut last_val = 0;

    rdqagie(
        f,
        ex,
        bound_val,
        inf_val,
        epsabs_val,
        epsrel_val,
        limit_val,
        &mut result_val,
        &mut abserr_val,
        &mut neval_val,
        &mut ier_val,
        &mut alist,
        &mut blist,
        &mut rlist,
        &mut elist,
        &mut iord,
        &mut last_val,
    );

    // Copy results back to work/iwork arrays
    for i in 0..l1 {
        work_slice[i] = alist[i];
        work_slice[l1 + i] = blist[i];
        work_slice[l2 + i] = rlist[i];
        work_slice[l3 + i] = elist[i];
        iwork_slice[i] = iord[i];
    }

    unsafe {
        *result = result_val;
        *abserr = abserr_val;
        *neval = neval_val;
        *ier = ier_val;
        *last = last_val;
    }
}

/// Integration over finite intervals (QAGS from QUADPACK).
///
/// Computes an approximation to the integral of f over (a, b).
///
/// # Arguments
/// * `f` - vectorizing integrand function
/// * `ex` - opaque pointer passed to f
/// * `a` - lower limit of integration
/// * `b` - upper limit of integration
/// * `epsabs` - absolute accuracy requested
/// * `epsrel` - relative accuracy requested
/// * `result` - approximation to the integral (output)
/// * `abserr` - estimate of absolute error (output)
/// * `neval` - number of integrand evaluations (output)
/// * `ier` - error code (output)
/// * `limit` - maximum number of subintervals
/// * `lenw` - dimension of work (must be >= limit*4)
/// * `last` - number of subintervals produced (output)
/// * `iwork` - integer work array of dimension limit
/// * `work` - double work array of dimension lenw
pub extern "C" fn Rdqags(
    f: IntegrFn,
    ex: *mut std::ffi::c_void,
    a: *mut f64,
    b: *mut f64,
    epsabs: *mut f64,
    epsrel: *mut f64,
    result: *mut f64,
    abserr: *mut f64,
    neval: *mut std::os::raw::c_int,
    ier: *mut std::os::raw::c_int,
    limit: *mut std::os::raw::c_int,
    lenw: *mut std::os::raw::c_int,
    last: *mut std::os::raw::c_int,
    iwork: *mut std::os::raw::c_int,
    work: *mut f64,
) {
    let a_val = unsafe { *a };
    let b_val = unsafe { *b };
    let epsabs_val = unsafe { *epsabs };
    let epsrel_val = unsafe { *epsrel };
    let limit_val = unsafe { *limit };
    let lenw_val = unsafe { *lenw };

    unsafe {
        *ier = 6;
        *neval = 0;
        *last = 0;
        *result = 0.0;
        *abserr = 0.0;
    }

    if limit_val < 1 || lenw_val < limit_val * 4 {
        return;
    }

    let l1 = limit_val as usize;
    let l2 = l1 + l1;
    let l3 = l2 + l1;

    let work_slice = unsafe { std::slice::from_raw_parts_mut(work, lenw_val as usize) };
    let iwork_slice = unsafe { std::slice::from_raw_parts_mut(iwork, limit_val as usize) };

    // Copy into separate vectors
    let mut alist = vec![0.0f64; l1];
    let mut blist = vec![0.0f64; l1];
    let mut rlist = vec![0.0f64; l1];
    let mut elist = vec![0.0f64; l1];
    let mut iord = vec![0i32; l1];

    for i in 0..l1 {
        alist[i] = work_slice[i];
        blist[i] = work_slice[l1 + i];
        rlist[i] = work_slice[l2 + i];
        elist[i] = work_slice[l3 + i];
        iord[i] = iwork_slice[i];
    }

    let mut result_val = 0.0;
    let mut abserr_val = 0.0;
    let mut neval_val = 0;
    let mut ier_val = 0;
    let mut last_val = 0;

    rdqagse(
        f,
        ex,
        a_val,
        b_val,
        epsabs_val,
        epsrel_val,
        limit_val,
        &mut result_val,
        &mut abserr_val,
        &mut neval_val,
        &mut ier_val,
        &mut alist,
        &mut blist,
        &mut rlist,
        &mut elist,
        &mut iord,
        &mut last_val,
    );

    for i in 0..l1 {
        work_slice[i] = alist[i];
        work_slice[l1 + i] = blist[i];
        work_slice[l2 + i] = rlist[i];
        work_slice[l3 + i] = elist[i];
        iwork_slice[i] = iord[i];
    }

    unsafe {
        *result = result_val;
        *abserr = abserr_val;
        *neval = neval_val;
        *ier = ier_val;
        *last = last_val;
    }
}
