//! Checked, self-contained GNU LOESS interpolation workspaces.
//! No runtime, model cache, native address, or original training data is needed
//! to restore a surface. The parent numerical module forbids unsafe code.
use super::{Execution, Model, surface::{self, Cell, Tree}};

pub(crate) struct Shape {
    pub d: usize,
    pub nc: usize,
    pub nv: usize,
}

pub(crate) fn shape(parameter: &[i32]) -> Result<Shape, String> {
    let [d, n, vc, nc, nv, liv, lv, ..] = parameter else {
        return Err("LOESS surface requires seven parameters".into());
    };
    if !(1..=4).contains(d) || *n <= 0 || *vc != 1 << *d
        || *nc <= 0 || *nc % 2 != 1 || *nv < *vc
        || *nc > (*n).max(200) || *nv > (*n).max(200)
    {
        return Err("invalid LOESS surface dimensions".into());
    }
    let (d, nc, nv) = (*d as usize, *nc as usize, *nv as usize);
    // These are the original grow layout's minimum capacities, not allocation
    // requests. A malicious capacity field cannot induce a huge allocation.
    let minimum_iv = 49u64 + ((1u64 << d) + 3) * nc as u64;
    let minimum_v = 49u64 + (2 * d as u64 + 1) * nv as u64 + nc as u64;
    if i64::from(*liv) < minimum_iv as i64 || i64::from(*lv) < minimum_v as i64 {
        return Err("LOESS surface workspace capacity is too small".into());
    }
    Ok(Shape { d, nc, nv })
}

pub(crate) struct PrunedSurface {
    pub parameter: [i32; 7],
    pub a: Vec<i32>,
    pub xi: Vec<f64>,
    pub vert: Vec<f64>,
    pub vval: Vec<f64>,
}

impl PrunedSurface {
    pub(super) fn fit(model: &Model, execution: &Execution<'_>) -> Result<Self, String> {
        execution.workspace(model.x.len(), model.divisor.len(), 0, true)?;
        let tree = surface::build_tree(model, execution)?;
        let d = model.divisor.len();
        let max_kd = model.x.len().max(200);
        let parameter = [d, model.x.len(), 1 << d, tree.cells.len(), tree.vertices.len(),
            49 + ((1 << d) + 3) * max_kd, 49 + (2*d + 2) * max_kd]
            .map(|n| i32::try_from(n).map_err(|_| "LOESS surface dimension overflow"))
            .into_iter().collect::<Result<Vec<_>, _>>()?;
        let mut result = Self {
            parameter: parameter.try_into().expect("seven surface parameters"),
            a: tree.cells.iter().map(|c| c.split.map_or(0, |(axis,_)| axis as i32 + 1)).collect(),
            xi: tree.cells.iter().map(|c| c.split.map_or(0., |(_,cut)| cut)).collect(),
            vert: [tree.cells[0].lo.clone(), tree.cells[0].hi.clone()].concat(),
            vval: Vec::with_capacity(tree.vertices.len() * (d + 1)),
        };
        for vertex in &tree.vertices {
            let coefficients = model.local(vertex, &model.weights, execution)?;
            result.vval.extend(coefficients.iter().map(|row| {
                row.iter().zip(&model.y).map(|(w,y)| w*y).sum::<f64>()
            }));
        }
        Ok(result)
    }

    pub(crate) fn predict(
        parameter: &[i32], a: &[i32], xi: &[f64], vert: &[f64], vval: &[f64],
        queries: &[Vec<f64>], execution: &Execution<'_>,
    ) -> Result<Vec<f64>, String> {
        let shape = shape(parameter)?;
        let Shape { d, nc, nv } = shape;
        if a.len() < nc || xi.len() < nc || vert.len() < 2*d || vval.len() < (d+1)*nv {
            return Err("truncated LOESS surface workspace".into());
        }
        // Bound restored geometry, nested coefficient vectors, queries and
        // results before allocating. Prediction never allocates by n/liv/lv.
        let bytes = nc.checked_mul(256 + 32*d)
            .and_then(|b| nv.checked_mul(128 + 64*d).and_then(|v| b.checked_add(v)))
            .and_then(|b| queries.len().checked_mul(64 + 16*d).and_then(|q| b.checked_add(q)));
        if bytes.is_none_or(|bytes| bytes > execution.workspace_limit) {
            return Err("LOESS surface workspace limit exceeded".into());
        }
        execution.checkpoint()?;
        let lo = vert[..d].to_vec();
        let hi = vert[d..2*d].to_vec();
        if (0..d).any(|j| !lo[j].is_finite() || !hi[j].is_finite() || lo[j] >= hi[j]) {
            return Err("invalid LOESS surface bounds".into());
        }
        if queries.iter().any(|q| q.len() != d || q.iter().any(|v| !v.is_finite())) {
            return Err("invalid LOESS surface prediction coordinates".into());
        }
        let vc = 1 << d;
        let mut tree = Tree {
            vertices: (0..vc).map(|bits| (0..d)
                .map(|j| if bits & (1 << j) == 0 { lo[j] } else { hi[j] }).collect()).collect(),
            cells: vec![Cell { lo, hi, observations: Vec::new(), corners: (0..vc).collect(), split: None }],
            leaves: Vec::new(),
        };
        // ehg169 restores the breadth-first children and ehg125 vertex order.
        // Every split is strictly interior and must consume declared children.
        for index in 0..nc {
            execution.checkpoint()?;
            if index >= tree.cells.len() || !(0..=d as i32).contains(&a[index]) {
                return Err("invalid LOESS surface split tree".into());
            }
            if a[index] == 0 {
                tree.leaves.push(index);
                continue;
            }
            let axis = a[index] as usize - 1;
            let cut = xi[index];
            let cell = &tree.cells[index];
            if !cut.is_finite() || cut <= cell.lo[axis] || cut >= cell.hi[axis]
                || tree.cells.len() + 2 > nc
            {
                return Err("invalid LOESS surface split location".into());
            }
            let (left, right) = surface::split_corners(&cell.corners, axis, cut, &mut tree.vertices);
            if tree.vertices.len() > nv {
                return Err("LOESS surface vertex count mismatch".into());
            }
            let leftlo = cell.lo.clone();
            let mut lefthi = cell.hi.clone(); lefthi[axis] = cut;
            let mut rightlo = cell.lo.clone(); rightlo[axis] = cut;
            let righthi = cell.hi.clone();
            tree.cells[index].split = Some((axis,cut));
            tree.cells.push(Cell { lo: leftlo, hi: lefthi, observations: Vec::new(), corners: left, split: None });
            tree.cells.push(Cell { lo: rightlo, hi: righthi, observations: Vec::new(), corners: right, split: None });
        }
        if tree.cells.len() != nc || tree.vertices.len() != nv {
            return Err("LOESS surface tree/vertex count mismatch".into());
        }
        let coefficients: Vec<_> = vval[..nv*(d+1)].chunks_exact(d+1)
            .map(|jet| jet.iter().map(|value| vec![*value]).collect()).collect();
        tree.evaluate(queries, &coefficients, execution)
            .map(|rows| rows.into_iter().map(|row| row[0]).collect())
    }
}

#[cfg(test)]
#[path = "pruned_tests.rs"]
mod tests;
