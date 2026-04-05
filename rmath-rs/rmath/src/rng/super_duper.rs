//! Super-Duper RNG (Reeds et al 1984). Port of R's src/main/RNG.c.
//! C does in-place XOR: I1 ^= ...; I1 ^= I1 << 17; (second uses updated I1).

pub struct SuperDuper {
    i1: u32,
    i2: u32,
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

impl SuperDuper {
    pub fn new() -> Self {
        Self { i1: 1, i2: 1 }
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

        if self.i1 == 0 {
            self.i1 = 1;
        }
        self.i2 |= 1;
    }

    pub fn get_rand(&mut self) -> f64 {
        self.i1 ^= (self.i1 >> 15) & 0x1FFFF;
        self.i1 ^= self.i1 << 17;
        self.i2 = self.i2.wrapping_mul(69069);
        fixup((self.i1 ^ self.i2) as f64 * I2_32M1)
    }
}
