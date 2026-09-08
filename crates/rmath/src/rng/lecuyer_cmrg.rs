//! L'Ecuyer-CMRG RNG. Port of R's src/main/RNG.c.

pub struct LecuyerCmrg {
    s: [i64; 6],
}

const M1: i64 = 4294967087;
const M2: i64 = 4294944443;
const NORMC: f64 = 2.328306549295727688e-10;
const A12: i64 = 1403580;
const A13N: i64 = 810728;
const A21: i64 = 527612;
const A23N: i64 = 1370589;

impl LecuyerCmrg {
    #[must_use]
    pub fn new() -> Self {
        Self { s: [0; 6] }
    }

    pub fn set_seed(&mut self, seed: i64) {
        let mut seed = seed as u32;
        for _ in 0..50 {
            seed = seed.wrapping_mul(69069).wrapping_add(1);
        }
        for j in 0..6 {
            seed = seed.wrapping_mul(69069).wrapping_add(1);
            while seed >= M2 as u32 {
                seed = seed.wrapping_mul(69069).wrapping_add(1);
            }
            self.s[j] = seed as i64;
        }
    }

    #[must_use]
    pub fn get_rand(&mut self) -> f64 {
        let [i0, i1, i2, i3, i4, i5] = self.s;

        let p1 = A12 * i1 - A13N * i0;
        let k = p1 / M1;
        let mut p1 = p1 - k * M1;
        if p1 < 0 {
            p1 += M1;
        }

        let p2 = A21 * i5 - A23N * i3;
        let k = p2 / M2;
        let mut p2 = p2 - k * M2;
        if p2 < 0 {
            p2 += M2;
        }

        self.s = [i1, i2, p1, i4, i5, p2];

        let result = if p1 > p2 { p1 - p2 } else { p1 - p2 + M1 };
        result as f64 * NORMC
    }
}
