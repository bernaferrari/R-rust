fn main() {
    // step through the factorization manually
    let n = 3usize; let p = 2usize;
    let mut x = vec![1.0, 1.0, 1.0, 1.0, 2.0, 3.0];
    let mut qraux = vec![0.0; p];
    let mut jpvt = vec![0i64; p];
    r_stats::linpack::dqrdc(&mut x, n, p, &mut qraux, &mut jpvt, true);
    println!("x after dqrdc = {:?}", x);
    println!("qraux = {:?}", qraux);
    // reference LINPACK for this input (pivot c1 first):
    // col c1=(1,2,3): nrmxl=3.7416573867739413, u=(1.267261, 0.534522, 0.801784)
    // R[0,0]=-3.7416574; applied to c0: R[0,1] = -2.4037 hmm compute:
    // t = -(u·ones)/u0 = -(1.267261+0.534522+0.801784)/1.267261 = -2.054493
    // c0' = ones + t*u = (1-2.603568, 1-1.098340, 1-1.647510) = (-1.603568, -0.098340, -0.647510)
    // second householder on subcolumn (-0.098340, -0.647510): nrmxl=0.654920
    // sign of x[1,1]=-0.098340 negative → nrmxl=-0.654920
    println!("expect x[3]=-1.603568 x[4],x[5] householder of (-0.098340,-0.647510)");
    // qty trace
    let b = vec![1.0, 2.0, 3.0];
    let qy = r_stats::linpack::qty(&x, &qraux, &b, n, p);
    println!("qty = {:?} — hand check j0 contribution (-3.7417,0,0) before j1", qy);
}
