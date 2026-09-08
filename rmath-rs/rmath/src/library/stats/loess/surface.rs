//! KD-cell interpolation and the two-dimensional blending correction.
use super::Model;

struct Cell {
    lo: Vec<f64>,
    hi: Vec<f64>,
    observations: Vec<usize>,
    corners: Vec<usize>,
}

pub(super) fn hermite(t: f64, width: f64, a: f64, b: f64, da: f64, db: f64) -> f64 {
    (1. - t).powi(2) * (1. + 2. * t) * a
        + t * t * (3. - 2. * t) * b
        + width * (t * (1. - t).powi(2) * da + t * t * (t - 1.) * db)
}

pub(super) fn interpolate(
    model: &Model,
    queries: &[Vec<f64>],
    weights: &[f64],
) -> Result<Vec<Vec<f64>>, String> {
    let n = model.x.len();
    let d = model.divisor.len();
    let vc = 1 << d;
    let mut lo = vec![f64::INFINITY; d];
    let mut hi = vec![f64::NEG_INFINITY; d];
    for x in &model.x {
        for j in 0..d {
            lo[j] = lo[j].min(x[j]);
            hi[j] = hi[j].max(x[j]);
        }
    }
    for j in 0..d {
        let mu = 0.005 * (hi[j] - lo[j]).max(1e-10 * lo[j].abs().max(hi[j].abs()) + 1e-30);
        lo[j] -= mu;
        hi[j] += mu;
    }
    let mut vertices: Vec<Vec<f64>> = vec![];
    fn corners(lo: &[f64], hi: &[f64], vertices: &mut Vec<Vec<f64>>) -> Vec<usize> {
        (0..1 << lo.len())
            .map(|bits| {
                let point: Vec<_> = (0..lo.len())
                    .map(|j| if bits & (1 << j) == 0 { lo[j] } else { hi[j] })
                    .collect();
                if let Some(index) = vertices.iter().position(|v| *v == point) {
                    index
                } else {
                    vertices.push(point);
                    vertices.len() - 1
                }
            })
            .collect()
    }
    let rootcorners = corners(&lo, &hi, &mut vertices);
    let mut cells = vec![Cell {
        lo,
        hi,
        observations: (0..n).collect(),
        corners: rootcorners,
    }];
    let mut leaves = vec![];
    let mut next = 0;
    let threshold = (n as f64 * model.config.span * model.config.cell).floor() as usize;
    while next < cells.len() {
        let cell = &cells[next];
        if cell.observations.len() <= threshold.max(1)
            || vertices.len() + vc / 2 > n.max(200)
            || cells.len() + 2 > n.max(200)
        {
            leaves.push(next);
            next += 1;
            continue;
        }
        let axis = (0..d)
            .filter(|j| !model.config.parametric[*j])
            .max_by(|a, b| {
                let range = |j: usize| {
                    let mut lo = f64::INFINITY;
                    let mut hi = f64::NEG_INFINITY;
                    for i in &cell.observations {
                        lo = lo.min(model.x[*i][j]);
                        hi = hi.max(model.x[*i][j]);
                    }
                    hi - lo
                };
                range(*a).total_cmp(&range(*b)).then(b.cmp(a))
            })
            .unwrap();
        let mut indices = cell.observations.clone();
        indices.sort_by(|a, b| model.x[*a][axis].total_cmp(&model.x[*b][axis]));
        let middle = (indices.len() - 1) / 2;
        let split = (0..indices.len() - 1)
            .filter(|i| {
                model.x[indices[*i]][axis] < model.x[indices[*i + 1]][axis]
                    && model.x[indices[*i]][axis] > cell.lo[axis]
                    && model.x[indices[*i]][axis] < cell.hi[axis]
            })
            .min_by_key(|i| (i.abs_diff(middle), usize::from(*i < middle)));
        if let Some(split) = split {
            let cut = model.x[indices[split]][axis];
            let (leftlo, mut lefthi) = (cell.lo.clone(), cell.hi.clone());
            lefthi[axis] = cut;
            let (mut rightlo, righthi) = (cell.lo.clone(), cell.hi.clone());
            rightlo[axis] = cut;
            let lc = corners(&leftlo, &lefthi, &mut vertices);
            let rc = corners(&rightlo, &righthi, &mut vertices);
            cells.push(Cell {
                lo: leftlo,
                hi: lefthi,
                observations: indices[..=split].to_vec(),
                corners: lc,
            });
            cells.push(Cell {
                lo: rightlo,
                hi: righthi,
                observations: indices[split + 1..].to_vec(),
                corners: rc,
            });
        } else {
            leaves.push(next);
        }
        next += 1;
    }
    let coefficients: Vec<_> = vertices
        .iter()
        .map(|q| model.local(q, weights))
        .collect::<Result<_, _>>()?;
    let mut output = Vec::with_capacity(queries.len());
    for q in queries {
        let cell = leaves
            .iter()
            .map(|i| &cells[*i])
            .find(|c| (0..d).all(|j| q[j] >= c.lo[j] && q[j] <= c.hi[j]))
            .ok_or("extrapolation is not allowed with interpolated LOESS")?;
        let mut row = vec![0.; n];
        for obs in 0..n {
            let mut g: Vec<Vec<_>> = cell
                .corners
                .iter()
                .map(|i| coefficients[*i].iter().map(|r| r[obs]).collect())
                .collect();
            let mut size = vc;
            for axis in (0..d).rev() {
                size /= 2;
                let width = cell.hi[axis] - cell.lo[axis];
                let t = (q[axis] - cell.lo[axis]) / width;
                for i in 0..size {
                    g[i][0] = hermite(
                        t,
                        width,
                        g[i][0],
                        g[i + size][0],
                        g[i][axis + 1],
                        g[i + size][axis + 1],
                    );
                    for j in 1..=axis {
                        g[i][j] = (1. - t).powi(2) * (1. + 2. * t) * g[i][j]
                            + t * t * (3. - 2. * t) * g[i + size][j];
                    }
                }
            }
            let tensor = g[0][0];
            row[obs] = if d == 2 {
                // Coons blending: evaluate each edge using the finest adjacent
                // subdivision, so T-junctions share the same boundary curve.
                let edge = |axis: usize, side: usize| {
                    let normal = 1 - axis;
                    let fixed = if side == 0 {
                        cell.lo[normal]
                    } else {
                        cell.hi[normal]
                    };
                    let mut vs: Vec<_> = (0..vertices.len())
                        .filter(|i| {
                            vertices[*i][normal] == fixed
                                && vertices[*i][axis] >= cell.lo[axis]
                                && vertices[*i][axis] <= cell.hi[axis]
                        })
                        .collect();
                    vs.sort_by(|a, b| vertices[*a][axis].total_cmp(&vertices[*b][axis]));
                    let pair = vs
                        .windows(2)
                        .find(|p| {
                            q[axis] >= vertices[p[0]][axis] && q[axis] <= vertices[p[1]][axis]
                        })
                        .unwrap();
                    let (a, b) = (pair[0], pair[1]);
                    let width = vertices[b][axis] - vertices[a][axis];
                    let t = (q[axis] - vertices[a][axis]) / width;
                    let value = hermite(
                        t,
                        width,
                        coefficients[a][0][obs],
                        coefficients[b][0][obs],
                        coefficients[a][axis + 1][obs],
                        coefficients[b][axis + 1][obs],
                    );
                    let derivative =
                        (1. - t).powi(2) * (1. + 2. * t) * coefficients[a][normal + 1][obs]
                            + t * t * (3. - 2. * t) * coefficients[b][normal + 1][obs];
                    (value, derivative)
                };
                let (s, ds) = edge(0, 0);
                let (north, dn) = edge(0, 1);
                let (w, dw) = edge(1, 0);
                let (e, de) = edge(1, 1);
                let tx = (q[0] - cell.lo[0]) / (cell.hi[0] - cell.lo[0]);
                let ty = (q[1] - cell.lo[1]) / (cell.hi[1] - cell.lo[1]);
                hermite(ty, cell.hi[1] - cell.lo[1], s, north, ds, dn)
                    + hermite(tx, cell.hi[0] - cell.lo[0], w, e, dw, de)
                    - tensor
            } else {
                tensor
            };
        }
        output.push(row);
    }
    Ok(output)
}
