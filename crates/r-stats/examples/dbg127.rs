// isolate ehg127: fit at one vertex, compare against naive normal equations
fn main() {
    let n = 30usize;
    let xs: Vec<f64> = (0..n).map(|i| i as f64 / 29.0).collect();
    let ys: Vec<f64> = xs.iter().map(|&x| (5.0 * x).sin() + (31.0 * x).cos() / 8.0).collect();
    let ws: Vec<f64> = xs.iter().map(|&x| 1.0 + x).collect();
    let nf = 22usize;
    // vertex at x=0 (boundary-ish q): distances sorted, nearest 22
    let q = [0.0f64];
    let mut ycm = vec![0.0f64; n + 1];
    for i in 0..n { ycm[i + 1] = ys[i]; }
    let mut rwcm = vec![0.0f64; n + 1];
    for i in 0..n { rwcm[i + 1] = ws[i]; }
    let mut psi: Vec<usize> = (0..=n).collect();
    let cdeg = vec![2i64];
    let fit = r_stats::loessf::ehg127(&q, n, 1, nf, 0.75, &xs, &mut psi, &ycm, &rwcm, 3, 1, 2, &cdeg);
    println!("ehg127 value at q=0: {:.7}", fit.value);
    // naive reference: WLS normal equations over the same 22 nearest
    let mut idx: Vec<usize> = (0..n).collect();
    idx.sort_by(|&a, &b| (xs[a] - 0.0).abs().partial_cmp(&(xs[b] - 0.0).abs()).unwrap());
    let rho = (xs[idx[nf - 1]] - 0.0).powi(2);
    let mut ata = [[0.0f64; 3]; 3];
    let mut aty = [0.0f64; 3];
    for i in 0..nf {
        let k = idx[i];
        let u = ((xs[k]).abs() / rho.sqrt());
        let t = (1.0 - u.powi(3)).powi(3);
        let w2 = ws[k] * t; // squared weight
        let dx = xs[k];
        let row = [1.0, dx, dx * dx];
        for a in 0..3 {
            aty[a] += w2 * ys[k] * row[a];
            for b in 0..3 {
                ata[a][b] += w2 * row[a] * row[b];
            }
        }
    }
    // solve 3x3
    let det = ata[0][0] * (ata[1][1] * ata[2][2] - ata[1][2] * ata[2][1])
        - ata[0][1] * (ata[1][0] * ata[2][2] - ata[1][2] * ata[2][0])
        + ata[0][2] * (ata[1][0] * ata[2][1] - ata[1][1] * ata[2][0]);
    let inv00 = (ata[1][1] * ata[2][2] - ata[1][2] * ata[2][1]) / det;
    let inv01 = (ata[0][2] * ata[2][1] - ata[0][1] * ata[2][2]) / det;
    let inv02 = (ata[0][1] * ata[1][2] - ata[0][2] * ata[1][1]) / det;
    let c0 = inv00 * aty[0] + inv01 * aty[1] + inv02 * aty[2];
    println!("naive WLS value at q=0: {:.7}", c0);
}
