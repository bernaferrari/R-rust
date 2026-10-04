//! Owned matrix operations and GNU-style Householder QR without projections.
#![forbid(unsafe_code)]
use super::{Error, Result, zeros};

pub(super) struct Matrix {
    pub rows: usize,
    pub cols: usize,
    pub data: Vec<f64>,
}
impl Matrix {
    pub fn zero(rows: usize, cols: usize) -> Result<Self> {
        Ok(Self {
            rows,
            cols,
            data: zeros(rows.checked_mul(cols).ok_or(Error::Overflow)?)?,
        })
    }
    pub fn from_slice(rows: usize, cols: usize, data: &[f64]) -> Result<Self> {
        let mut result = Self::zero(rows, cols)?;
        if result.data.len() != data.len() {
            return Err(Error::Storage);
        }
        result.data.copy_from_slice(data);
        Ok(result)
    }
    pub fn copy(&self) -> Result<Self> {
        Self::from_slice(self.rows, self.cols, &self.data)
    }
    pub fn get(&self, row: usize, col: usize) -> f64 {
        self.data[row * self.cols + col]
    }
    pub fn set(&mut self, row: usize, col: usize, value: f64) {
        self.data[row * self.cols + col] = value;
    }
    pub fn identity(n: usize) -> Result<Self> {
        let mut result = Self::zero(n, n)?;
        for i in 0..n {
            result.set(i, i, 1.);
        }
        Ok(result)
    }
    pub fn transpose(&self) -> Result<Self> {
        let mut result = Self::zero(self.cols, self.rows)?;
        for i in 0..self.rows {
            for j in 0..self.cols {
                result.set(j, i, self.get(i, j));
            }
        }
        Ok(result)
    }
    pub fn product(
        &self,
        other: &Self,
        transpose_left: bool,
        transpose_right: bool,
    ) -> Result<Self> {
        let (rows, inner) = if transpose_left {
            (self.cols, self.rows)
        } else {
            (self.rows, self.cols)
        };
        let (other_inner, cols) = if transpose_right {
            (other.cols, other.rows)
        } else {
            (other.rows, other.cols)
        };
        if inner != other_inner {
            return Err(Error::Storage);
        }
        let mut result = Self::zero(rows, cols)?;
        for i in 0..rows {
            for j in 0..cols {
                let mut sum = 0.;
                for k in 0..inner {
                    let left = if transpose_left {
                        self.get(k, i)
                    } else {
                        self.get(i, k)
                    };
                    let right = if transpose_right {
                        other.get(j, k)
                    } else {
                        other.get(k, j)
                    };
                    sum += left * right;
                }
                result.set(i, j, sum);
            }
        }
        Ok(result)
    }
    pub fn add(&mut self, other: &Self, subtract: bool) -> Result<()> {
        if self.rows != other.rows || self.cols != other.cols {
            return Err(Error::Storage);
        }
        for (left, right) in self.data.iter_mut().zip(&other.data) {
            if subtract {
                *left -= right;
            } else {
                *left += right;
            }
        }
        Ok(())
    }
    pub fn divide(&mut self, denominator: f64) {
        for value in &mut self.data {
            *value /= denominator;
        }
    }
    pub fn solve(&self, rhs: &Self) -> Result<Self> {
        if self.rows != self.cols || rhs.rows != self.rows {
            return Err(Error::Storage);
        }
        // GNU qr_solve transposes the row-major matrix for LINPACK.
        let columns = self.transpose()?;
        let factor = Qr::factor(self.rows, &columns.data, Error::SingularQr)?;
        let mut result = Self::zero(self.cols, rhs.cols)?;
        for column in 0..rhs.cols {
            let mut input = zeros(self.rows)?;
            for (row, value) in input.iter_mut().enumerate() {
                *value = rhs.get(row, column);
            }
            let coefficients = factor.solve(&input)?;
            for (row, value) in coefficients.iter().enumerate() {
                result.set(row, column, *value);
            }
        }
        Ok(result)
    }
    pub fn log_determinant(&self) -> Result<f64> {
        if self.rows != self.cols {
            return Err(Error::Storage);
        }
        // GNU ldet gives the row-major backing to LINPACK directly. This
        // factors its transpose; preserving it also preserves summation order.
        let factor = Qr::factor(self.rows, &self.data, Error::SingularDet)?;
        let mut sum = 0.;
        for i in 0..self.rows {
            sum += factor.data[i + i * self.rows].abs().ln();
        }
        Ok(sum)
    }
}

struct Qr {
    size: usize,
    data: Vec<f64>,
    auxiliary: Vec<f64>,
}
impl Qr {
    fn factor(size: usize, data: &[f64], singular: Error) -> Result<Self> {
        if size == 0 || data.len() != size.checked_mul(size).ok_or(Error::Overflow)? {
            return Err(Error::Storage);
        }
        let mut result = Self {
            size,
            data: zeros(data.len())?,
            auxiliary: zeros(size)?,
        };
        result.data.copy_from_slice(data);
        let mut original_norms = zeros(size)?;
        for (column, original) in original_norms.iter_mut().enumerate() {
            let norm = norm(&result.data[column * size..(column + 1) * size]);
            result.auxiliary[column] = norm;
            *original = if norm == 0. { 1. } else { norm };
        }
        let mut rank_bound = size;
        for column in 0..size {
            // GNU dqrdc2 cycles negligible columns; a full-rank square solve
            // never needs pivot reconstruction because any moved column fails.
            while column < rank_bound
                && !(result.auxiliary[column] >= original_norms[column] * 1e-7)
            {
                for row in 0..size {
                    let value = result.data[row + column * size];
                    for j in column..size - 1 {
                        result.data[row + j * size] = result.data[row + (j + 1) * size];
                    }
                    result.data[row + (size - 1) * size] = value;
                }
                let aux = result.auxiliary[column];
                let original = original_norms[column];
                for j in column..size - 1 {
                    result.auxiliary[j] = result.auxiliary[j + 1];
                    original_norms[j] = original_norms[j + 1];
                }
                result.auxiliary[size - 1] = aux;
                original_norms[size - 1] = original;
                rank_bound -= 1;
            }
            if column + 1 != size {
                let diagonal = column + column * size;
                let mut magnitude = norm(&result.data[diagonal..(column + 1) * size]);
                if magnitude != 0. {
                    if result.data[diagonal] != 0. {
                        magnitude = magnitude.copysign(result.data[diagonal]);
                    }
                    for i in column..size {
                        result.data[i + column * size] *= 1. / magnitude;
                    }
                    result.data[diagonal] += 1.;
                    for j in column + 1..size {
                        let mut dot = 0.;
                        for i in column..size {
                            dot += result.data[i + column * size] * result.data[i + j * size];
                        }
                        let scale = -dot / result.data[diagonal];
                        for i in column..size {
                            result.data[i + j * size] += scale * result.data[i + column * size];
                        }
                        if result.auxiliary[j] != 0. {
                            let reduction = (1.
                                - (result.data[column + j * size].abs() / result.auxiliary[j])
                                    .powi(2))
                            .max(0.);
                            if reduction.abs() >= 1e-6 {
                                result.auxiliary[j] *= reduction.sqrt();
                            } else {
                                result.auxiliary[j] =
                                    norm(&result.data[column + 1 + j * size..(j + 1) * size]);
                            }
                        }
                    }
                    result.auxiliary[column] = result.data[diagonal];
                    result.data[diagonal] = -magnitude;
                }
            }
        }
        if rank_bound != size {
            return Err(singular);
        }
        Ok(result)
    }
    fn solve(&self, input: &[f64]) -> Result<Vec<f64>> {
        if input.len() != self.size {
            return Err(Error::Storage);
        }
        let mut result = zeros(self.size)?;
        result.copy_from_slice(input);
        for column in 0..self.size - 1 {
            let diagonal = self.auxiliary[column];
            if diagonal != 0. {
                let mut dot = diagonal * result[column];
                for (i, value) in result.iter().enumerate().skip(column + 1) {
                    dot += self.data[i + column * self.size] * value;
                }
                let scale = -dot / diagonal;
                result[column] += scale * diagonal;
                for (i, value) in result.iter_mut().enumerate().skip(column + 1) {
                    *value += scale * self.data[i + column * self.size];
                }
            }
        }
        for j in (0..self.size).rev() {
            let diagonal = self.data[j + j * self.size];
            if diagonal == 0. {
                return Err(Error::SingularQr);
            }
            result[j] /= diagonal;
            let scale = -result[j];
            for (i, value) in result.iter_mut().enumerate().take(j) {
                *value += scale * self.data[i + j * self.size];
            }
        }
        Ok(result)
    }
}

/// Scaled BLAS norm: avoid overflowing or underflowing intermediate squares.
fn norm(values: &[f64]) -> f64 {
    let mut scale = 0.;
    let mut squares = 1.;
    for value in values {
        if *value != 0. {
            let absolute = value.abs();
            if scale < absolute {
                squares = 1. + squares * (scale / absolute).powi(2);
                scale = absolute;
            } else {
                squares += (absolute / scale).powi(2);
            }
        }
    }
    scale * squares.sqrt()
}
