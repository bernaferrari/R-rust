// quick validation harness — fitted values against the golden
fn main() {
    let n = 30;
    let xs: Vec<f64> = (0..n).map(|i| i as f64 / 29.0).collect();
    let ys: Vec<f64> = xs.iter().map(|&x| (5.0 * x).sin() + (31.0 * x).cos() / 8.0).collect();
    let ws: Vec<f64> = xs.iter().map(|&x| 1.0 + x).collect();
    let st = r_stats::loessf::loess_fit_exact(&xs, &ys, &ws, 0.75, 2, 0.2);
    println!("fitted[1,15,30] = {:.7},{:.7},{:.7}", st.fitted[0], st.fitted[14], st.fitted[29]);
    println!("golden          = -0.0044871,0.6536559,-1.0234164");
    println!("trl/d1/d2 = {:.7},{:.7},{:.7}", st.trl, st.delta1, st.delta2);
    println!("golden trace.hat,one.delta,two.delta,s = 4.8325647,24.788964,24.474259,0.1365471");
}
