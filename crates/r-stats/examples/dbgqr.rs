fn main() {
    // trivial: x = [[1,1],[1,2],[1,3]] fit y = x exactly -> coef 0,1
    let n = 3usize; let p = 2usize;
    let mut x = vec![1.0, 1.0, 1.0, 1.0, 2.0, 3.0]; // col-major: col0=ones, col1=1,2,3
    let mut qraux = vec![0.0; p];
    let mut jpvt = vec![0i64; p];
    r_stats::linpack::dqrdc(&mut x, n, p, &mut qraux, &mut jpvt, true);
    let b = vec![1.0, 2.0, 3.0];
    let coef = r_stats::linpack::qr_solve(&x, &qraux, &jpvt, &b, n, p, p);
    println!("coef = {:?} (expect [0,1])", coef);
    println!("jpvt = {:?}", jpvt);
    println!("R diag = {},{}", x[0], x[1 + 3]);
}
