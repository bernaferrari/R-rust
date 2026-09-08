//! Grid layout allocation using owned physical lengths and raw null weights.
//! The order follows GNU R grid/layout.c: fixed, respected, then remaining.
#![forbid(unsafe_code)]

#[derive(Clone, Copy, Debug)]
pub(super) enum Length {
    Fixed(f64),
    Null(f64),
}

pub(super) struct Axis {
    pub lengths: Vec<f64>,
    weights: Vec<f64>,
    remaining: f64,
}
impl Axis {
    pub fn new(terms: Vec<Length>, extent: f64) -> Self {
        let lengths: Vec<_> = terms
            .iter()
            .map(|v| match v {
                Length::Fixed(x) => *x,
                Length::Null(_) => 0.,
            })
            .collect();
        let remaining = extent - lengths.iter().sum::<f64>();
        let weights = terms
            .iter()
            .map(|v| match v {
                Length::Null(x) => *x,
                Length::Fixed(_) => 0.,
            })
            .collect();
        Self {
            lengths,
            weights,
            remaining,
        }
    }
    fn total_weight(&self) -> f64 {
        self.weights.iter().sum()
    }
    fn respected(&mut self, mask: &[bool], scale: f64) {
        for (i, respected) in mask.iter().enumerate() {
            if *respected && self.weights[i] != 0. {
                self.lengths[i] = self.weights[i] * scale;
                self.remaining -= self.lengths[i];
                self.weights[i] = 0.;
            }
        }
    }
    fn finish(&mut self) {
        let total = self.total_weight();
        if total > 0. {
            let scale = self.remaining.max(0.) / total;
            for (value, weight) in self.lengths.iter_mut().zip(&self.weights) {
                *value += weight * scale;
            }
        }
    }
}
pub(super) fn allocate(
    mut x: Axis,
    mut y: Axis,
    columns: &[bool],
    rows: &[bool],
) -> (Vec<f64>, Vec<f64>) {
    let wx = x.total_weight();
    let wy = y.total_weight();
    let scale = match (wx > 0., wy > 0.) {
        (true, true) => (x.remaining / wx).min(y.remaining / wy),
        (true, false) => x.remaining / wx,
        (false, true) => y.remaining / wy,
        _ => 0.,
    }
    .max(0.);
    x.respected(columns, scale);
    y.respected(rows, scale);
    x.finish();
    y.finish();
    (x.lengths, y.lengths)
}
