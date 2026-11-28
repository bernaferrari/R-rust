fn main() {
    let n = 30;
    let xs: Vec<f64> = (0..n).map(|i| i as f64 / 29.0).collect();
    let ys: Vec<f64> = xs.iter().map(|&x| (5.0 * x).sin() + (31.0 * x).cos() / 8.0).collect();
    let ws: Vec<f64> = xs.iter().map(|&x| 1.0 + x).collect();
    for cell in [0.2f64, 0.15] {
        let st = r_stats::loessf::loess_fit_exact(&xs, &ys, &ws, 0.75, 2, cell);
        let ss: f64 = ws.iter().zip(ys.iter().zip(st.fitted.iter())).map(|(w, (y, f))| w * (y - f) * (y - f)).sum();
        let s = (ss / st.delta1).sqrt();
        println!("cell={}: fitted[1]={:.7} trl={:.7} s={:.7} (golden -0.0044871, 4.8325647, 0.1365471)", cell, st.fitted[0], st.trl, s);
    }
}
