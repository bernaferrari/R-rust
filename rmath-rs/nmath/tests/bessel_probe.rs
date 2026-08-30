//! Scratch trunk-parity probe: reads /tmp/bessel_probe/trunk.csv, computes
//! the same grid with the Rust port, writes /tmp/bessel_probe/rust.csv.
//! Removed before final delivery.

use std::io::Write;

fn fmt(v: f64) -> String {
    if v.is_nan() {
        "NaN".to_string()
    } else if v.is_infinite() {
        if v > 0.0 { "Inf".to_string() } else { "-Inf".to_string() }
    } else {
        format!("{v:.17e}")
    }
}

#[test]
fn probe() {
    let data = std::fs::read_to_string("/tmp/bessel_probe/trunk.csv").unwrap();
    let mut out = std::io::BufWriter::new(std::fs::File::create("/tmp/bessel_probe/rust.csv").unwrap());
    writeln!(out, "fn,expo,x,nu,val_trunk,val_rust").unwrap();
    for (n, line) in data.lines().enumerate() {
        if n == 0 {
            continue;
        }
        // columns: fn,expo,x,nu,val  (R write.csv, quote=FALSE)
        let f: Vec<&str> = line.split(',').collect();
        let func = f[0];
        let expo: f64 = f[1].parse().unwrap();
        let x: f64 = f[2].parse().unwrap();
        let nu: f64 = f[3].parse().unwrap();
        let val_trunk = f[4];
        let got = match func {
            "I" => rmath_nmath::special::bessel_i::bessel_i(x, nu, expo),
            "K" => rmath_nmath::special::bessel_k::bessel_k(x, nu, expo),
            "Y" => rmath_nmath::special::bessel_y::bessel_y(x, nu),
            _ => panic!("bad fn {func}"),
        };
        writeln!(out, "{func},{expo},{x},{nu},{val_trunk},{}", fmt(got)).unwrap();
    }
}
