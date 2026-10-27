//! Public `ppr(..., sm.method = "spline")` regression against pinned GNU R.

use r_embed::RSession;

fn csv_values(line: &str, prefix: &str) -> Vec<f64> {
    line.strip_prefix(prefix)
        .expect("expected output prefix")
        .split(',')
        .map(|s| s.trim().parse().expect("numeric output"))
        .collect()
}

fn assert_vector(actual: &[f64], expected: &[f64], tolerance: f64) {
    assert_eq!(actual.len(), expected.len());
    for (index, (&actual, &expected)) in actual.iter().zip(expected).enumerate() {
        assert!(
            (actual - expected).abs() <= tolerance,
            "value {index}: got {actual:.17}, expected {expected:.17}"
        );
    }
}

#[test]
fn ppr_spline_matches_gnu_fitted_values() {
    let mut session = RSession::new().expect("create R session");
    let output = session
        .eval(
            r#"
x <- data.frame(x=seq(0, 1, length.out=12))
y <- sin(6*x$x) + 0.2*x$x
xn <- nrow(x)
ord <- order(-x$x)
z <- (y - mean(y)) / sqrt(mean((y-mean(y))^2))
raw <- .Fortran(get("C_rbart", asNamespace("stats")),
  as.double(1), as.double(5), x=seq(0, 1, length.out=xn), y=z[ord],
  w=rep(1, xn), ssw=as.double(0), n=as.integer(xn),
  knot=c(rep(0,4), seq(1/9, 8/9, length.out=8), rep(1,4)), nk=as.integer(12),
  coef=double(12), sz=double(xn), lev=double(xn), crit=double(1),
  iparms=as.integer(c(3,0,500,0)), spar=double(1),
  parms=as.double(c(0,1.5,1e-2,.000244,0)), scratch=double(18*12+1),
  ld4=as.integer(4), ldnk=as.integer(1), ier=integer(1))
cat("rbart=", format(raw$spar, digits=17), ";", format(sum(raw$lev), digits=17), ";",
    paste(format(raw$lev, digits=12), collapse=","), "\n", sep="")
cat("raw-spline=", paste(sprintf("%.8f", raw$sz), collapse=","), "\n", sep="")
m <- ppr(x, y, nterms=1, max.terms=1, sm.method="spline")
cat("alpha=", format(m$alpha, digits=17), "; beta=", format(m$beta, digits=17), "\n", sep="")
cat("edf=", format(m$edf, digits=17), "\n", sep="")
cat("smoothed=", paste(sprintf("%.8f", m$smod[10:21]), collapse=","), "\n", sep="")
cat("projection=", paste(sprintf("%.8f", m$smod[22:33]), collapse=","), "\n", sep="")
cat(paste(sprintf("%.8f", as.vector(fitted(m))), collapse=","), "\n", sep="")
"#,
        )
        .expect("fit PPR with spline smoother");
    let lines: Vec<_> = output.lines().collect();
    assert_eq!(lines.len(), 7, "unexpected PPR spline output: {output}");

    let raw: Vec<_> = lines[0]
        .strip_prefix("rbart=")
        .unwrap()
        .split(';')
        .collect();
    assert_eq!(raw.len(), 3);
    assert!((raw[0].trim().parse::<f64>().unwrap() - 0.39973305453352825).abs() < 1e-9);
    assert!((raw[1].trim().parse::<f64>().unwrap() - 5.0655330164816164).abs() < 1e-8);
    assert_vector(
        &csv_values(&format!("lev={}", raw[2]), "lev="),
        &[
            0.756668332896,
            0.372949295705,
            0.357402993274,
            0.347902363247,
            0.346284995861,
            0.351558527312,
            0.351558527312,
            0.346284995861,
            0.347902363247,
            0.357402993274,
            0.372949295705,
            0.756668332787,
        ],
        1e-10,
    );
    assert_vector(
        &csv_values(lines[1], "raw-spline="),
        &[
            -0.46796801,
            -0.96577267,
            -1.26711459,
            -1.22749664,
            -0.82172839,
            -0.15887294,
            0.56126613,
            1.11759545,
            1.34283105,
            1.17727446,
            0.68635774,
            0.02362839,
        ],
        1e-8,
    );
    let beta = lines[2]
        .strip_prefix("alpha=-1; beta=")
        .unwrap()
        .parse::<f64>()
        .unwrap();
    assert!((beta - 0.64566988298904726).abs() < 1e-8);
    let edf = lines[3]
        .strip_prefix("edf=")
        .unwrap()
        .parse::<f64>()
        .unwrap();
    assert!((edf - 5.0655330166365786).abs() < 1e-8);
    assert_vector(
        &csv_values(lines[4], "smoothed="),
        &[
            0.02564195,
            0.74484767,
            1.27759926,
            1.45726423,
            1.21283453,
            0.60909602,
            -0.17241175,
            -0.89175431,
            -1.33210126,
            -1.37509537,
            -1.04807374,
            -0.50784723,
        ],
        1e-8,
    );
    assert_vector(
        &csv_values(lines[5], "projection="),
        &[
            0.0,
            -0.09090909,
            -0.18181818,
            -0.27272727,
            -0.36363636,
            -0.45454545,
            -0.54545455,
            -0.63636364,
            -0.72727273,
            -0.81818182,
            -0.90909091,
            -1.0,
        ],
        1e-8,
    );
    assert_vector(
        &csv_values(lines[6], ""),
        &[
            0.11084740,
            0.57521687,
            0.91919852,
            1.03520278,
            0.87738189,
            0.48756612,
            -0.01702992,
            -0.48148774,
            -0.76580650,
            -0.79356651,
            -0.58241849,
            -0.23361051,
        ],
        1e-8,
    );
}
