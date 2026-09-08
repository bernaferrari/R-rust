//! Wichmann-Hill RNG. Port of R's src/main/RNG.c.

pub struct WichmannHill {
    i1: u32,
    i2: u32,
    i3: u32,
}

const I2_32M1: f64 = 2.328306437080797e-10;

fn fixup(x: f64) -> f64 {
    if x <= 0.0 {
        return 0.5 * I2_32M1;
    }
    if (1.0 - x) <= 0.0 {
        return 1.0 - 0.5 * I2_32M1;
    }
    x
}

impl WichmannHill {
    #[must_use]
    pub fn new() -> Self {
        Self {
            i1: 1,
            i2: 1,
            i3: 1,
        }
    }

    pub fn set_seed(&mut self, seed: i64) {
        let mut seed = seed as i32;
        for _ in 0..50 {
            seed = seed.wrapping_mul(69069).wrapping_add(1);
        }
        seed = seed.wrapping_mul(69069).wrapping_add(1);
        self.i1 = seed as u32;
        seed = seed.wrapping_mul(69069).wrapping_add(1);
        self.i2 = seed as u32;
        seed = seed.wrapping_mul(69069).wrapping_add(1);
        self.i3 = seed as u32;

        self.i1 %= 30269;
        self.i2 %= 30307;
        self.i3 %= 30323;
        if self.i1 == 0 {
            self.i1 = 1;
        }
        if self.i2 == 0 {
            self.i2 = 1;
        }
        if self.i3 == 0 {
            self.i3 = 1;
        }
    }

    #[must_use]
    pub fn get_rand(&mut self) -> f64 {
        self.i1 = self.i1.wrapping_mul(171) % 30269;
        self.i2 = self.i2.wrapping_mul(172) % 30307;
        self.i3 = self.i3.wrapping_mul(170) % 30323;
        let value = self.i1 as f64 / 30269.0 + self.i2 as f64 / 30307.0 + self.i3 as f64 / 30323.0;
        fixup(value - value.floor())
    }
}
