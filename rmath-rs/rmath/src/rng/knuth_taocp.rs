//! Knuth-TAOCP RNG (1997 version). Port of R's .TAOCP1997init and C's KT_next/ran_array.

const KK: usize = 100;
const LL: usize = 37;
const MM: i64 = 1 << 30;
const KKK: usize = 199;
const KKL: usize = 63;
const QUALITY: usize = 1009;
const KT_SCALE: f64 = 9.31322574615479e-10;

pub struct KnuthTaocp {
    x: [i64; KK],
    pos: usize,
}

fn mod_diff(x: i64, y: i64) -> i64 {
    (x - y) & (MM - 1)
}

impl KnuthTaocp {
    pub fn new() -> Self {
        Self {
            x: [0; KK],
            pos: KK,
        }
    }

    fn taocp1997init(seed: i64) -> [i64; KK] {
        let mut ss = seed - (seed % 2) + 2;
        let mut x = [0i64; KKK];

        for j in 0..KK {
            x[j] = ss;
            ss = ss + ss;
            if ss >= MM {
                ss = ss - MM + 2;
            }
        }
        x[1] += 1;

        let mut ss = seed;
        let mut t = 69i64;

        while t > 0 {
            for j in (2..=KK).rev() {
                x[2 * j - 2] = x[j - 1];
            }

            let mut rj = KKK as i64;
            while rj >= (KKL + 1) as i64 {
                x[(KKK as i64 + 2 - rj - 1) as usize] =
                    x[(rj - 1) as usize] - x[(rj - 1) as usize] % 2;
                rj -= 2;
            }

            for j in (KK..KKK).rev() {
                if x[j] % 2 == 1 {
                    x[j - KKL] = (x[j - KKL] - x[j]).rem_euclid(MM);
                    x[j - KK] = (x[j - KK] - x[j]).rem_euclid(MM);
                }
            }

            if ss % 2 == 1 {
                for j in (0..KK).rev() {
                    x[j + 1] = x[j];
                }
                x[0] = x[KK];
                if x[KK] % 2 == 1 {
                    x[LL] = (x[LL] - x[KK]).rem_euclid(MM);
                }
            }

            if ss != 0 {
                ss /= 2;
            } else {
                t -= 1;
            }
        }

        let mut result = [0i64; KK];
        result[0..63].copy_from_slice(&x[37..100]);
        result[63..100].copy_from_slice(&x[0..37]);
        result
    }

    pub fn set_seed(&mut self, seed: i64) {
        let mut seed = seed as i32;
        for _ in 0..50 {
            seed = seed.wrapping_mul(69069).wrapping_add(1);
        }
        let s = ((seed as i64 % 1_073_741_821) + 1_073_741_821) % 1_073_741_821;
        self.x.copy_from_slice(&Self::taocp1997init(s));
        self.pos = KK;
    }

    #[allow(clippy::manual_memcpy)]
    fn ran_array(&mut self, aa: &mut [i64], n: usize) {
        for j in 0..KK {
            aa[j] = self.x[j];
        }
        for j in KK..n {
            aa[j] = mod_diff(aa[j - KK], aa[j - LL]);
        }
        let mut j = n;
        for i in 0..LL {
            self.x[i] = mod_diff(aa[j - KK], aa[j - LL]);
            j += 1;
        }
        for i in LL..KK {
            self.x[i] = mod_diff(aa[j - KK], self.x[i - LL]);
            j += 1;
        }
    }

    fn ran_arr_cycle(&mut self) {
        let mut aa = [0i64; QUALITY];
        self.ran_array(&mut aa, QUALITY);
    }

    pub fn get_rand(&mut self) -> f64 {
        if self.pos >= KK {
            self.ran_arr_cycle();
            self.pos = 0;
        }
        let val = self.x[self.pos];
        self.pos += 1;
        val as f64 * KT_SCALE
    }
}
