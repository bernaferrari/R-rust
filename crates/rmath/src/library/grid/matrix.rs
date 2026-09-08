/*
 *  R : A Computer Language for Statistical Data Analysis
 *  Copyright (C) 2001-3 Paul Murrell
 *                2003 The R Core Team
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program; if not, a copy is available at
 *  https://www.R-project.org/Licenses/
 */

/* Code for matrices, matrix multiplication, etc for performing
 *  2D affine transformations:  translations, scaling, and rotations.
 */

use std::f64::consts::PI;

use super::types::{LLocation, LTransform};

fn location_x(l: &LLocation) -> f64 {
    l[0]
}

fn location_y(l: &LLocation) -> f64 {
    l[1]
}

fn copy_transform(t1: &LTransform, t2: &mut LTransform) {
    for i in 0..3 {
        for j in 0..3 {
            t2[i][j] = t1[i][j];
        }
    }
}

fn inv_transform(t: &LTransform, invt: &mut LTransform) {
    let det = t[0][0] * (t[2][2] * t[1][1] - t[2][1] * t[1][2])
        - t[1][0] * (t[2][2] * t[0][1] - t[2][1] * t[0][2])
        + t[2][0] * (t[1][2] * t[0][1] - t[1][1] * t[0][2]);
    if det == 0.0 {
        return;
    }
    let inv_det = 1.0 / det;
    invt[0][0] = inv_det * (t[2][2] * t[1][1] - t[2][1] * t[1][2]);
    invt[0][1] = -inv_det * (t[2][2] * t[0][1] - t[2][1] * t[0][2]);
    invt[0][2] = inv_det * (t[1][2] * t[0][1] - t[1][1] * t[0][2]);
    invt[1][0] = -inv_det * (t[2][2] * t[1][0] - t[2][0] * t[1][2]);
    invt[1][1] = inv_det * (t[2][2] * t[0][0] - t[2][0] * t[0][2]);
    invt[1][2] = -inv_det * (t[1][2] * t[0][0] - t[1][0] * t[0][2]);
    invt[2][0] = inv_det * (t[2][1] * t[1][0] - t[2][0] * t[1][1]);
    invt[2][1] = -inv_det * (t[2][1] * t[0][0] - t[2][0] * t[0][1]);
    invt[2][2] = inv_det * (t[1][1] * t[0][0] - t[1][0] * t[0][1]);
}

fn identity_matrix(m: &mut LTransform) {
    for i in 0..3 {
        for j in 0..3 {
            m[i][j] = if i == j { 1.0 } else { 0.0 };
        }
    }
}

fn translation_matrix(tx: f64, ty: f64, m: &mut LTransform) {
    identity_matrix(m);
    m[2][0] = tx;
    m[2][1] = ty;
}

fn scaling_matrix(sx: f64, sy: f64, m: &mut LTransform) {
    identity_matrix(m);
    m[0][0] = sx;
    m[1][1] = sy;
}

fn rotation_matrix(theta: f64, m: &mut LTransform) {
    let thetarad = theta / 180.0 * PI;
    let costheta = thetarad.cos();
    let sintheta = thetarad.sin();
    identity_matrix(m);
    m[0][0] = costheta;
    m[0][1] = sintheta;
    m[1][0] = -sintheta;
    m[1][1] = costheta;
}

fn multiply_matrix(m1: &LTransform, m2: &LTransform, m: &mut LTransform) {
    m[0][0] = m1[0][0] * m2[0][0] + m1[0][1] * m2[1][0] + m1[0][2] * m2[2][0];
    m[0][1] = m1[0][0] * m2[0][1] + m1[0][1] * m2[1][1] + m1[0][2] * m2[2][1];
    m[0][2] = m1[0][0] * m2[0][2] + m1[0][1] * m2[1][2] + m1[0][2] * m2[2][2];
    m[1][0] = m1[1][0] * m2[0][0] + m1[1][1] * m2[1][0] + m1[1][2] * m2[2][0];
    m[1][1] = m1[1][0] * m2[0][1] + m1[1][1] * m2[1][1] + m1[1][2] * m2[2][1];
    m[1][2] = m1[1][0] * m2[0][2] + m1[1][1] * m2[1][2] + m1[1][2] * m2[2][2];
    m[2][0] = m1[2][0] * m2[0][0] + m1[2][1] * m2[1][0] + m1[2][2] * m2[2][0];
    m[2][1] = m1[2][0] * m2[0][1] + m1[2][1] * m2[1][1] + m1[2][2] * m2[2][1];
    m[2][2] = m1[2][0] * m2[0][2] + m1[2][1] * m2[1][2] + m1[2][2] * m2[2][2];
}

fn location_xy(x: f64, y: f64, v: &mut LLocation) {
    v[0] = x;
    v[1] = y;
    v[2] = 1.0;
}

fn trans_location(vin: &LLocation, m: &LTransform, vout: &mut LLocation) {
    vout[0] = vin[0] * m[0][0] + vin[1] * m[1][0] + vin[2] * m[2][0];
    vout[1] = vin[0] * m[0][1] + vin[1] * m[1][1] + vin[2] * m[2][1];
    vout[2] = vin[0] * m[0][2] + vin[1] * m[1][2] + vin[2] * m[2][2];
}

pub unsafe fn locationX(l: *const LLocation) -> f64 {
    unsafe { location_x(&*l) }
}

pub unsafe fn locationY(l: *const LLocation) -> f64 {
    unsafe { location_y(&*l) }
}

pub unsafe fn copyTransform(t1: *const LTransform, t2: *mut LTransform) {
    unsafe { copy_transform(&*t1, &mut *t2) }
}

pub unsafe fn invTransform(t: *const LTransform, invt: *mut LTransform) {
    unsafe { inv_transform(&*t, &mut *invt) }
}

pub unsafe fn identity(m: *mut LTransform) {
    unsafe { identity_matrix(&mut *m) }
}

pub unsafe fn translation(tx: f64, ty: f64, m: *mut LTransform) {
    unsafe { translation_matrix(tx, ty, &mut *m) }
}

pub unsafe fn scaling(sx: f64, sy: f64, m: *mut LTransform) {
    unsafe { scaling_matrix(sx, sy, &mut *m) }
}

pub unsafe fn rotation(theta: f64, m: *mut LTransform) {
    unsafe { rotation_matrix(theta, &mut *m) }
}

pub unsafe fn multiply(m1: *const LTransform, m2: *const LTransform, m: *mut LTransform) {
    unsafe { multiply_matrix(&*m1, &*m2, &mut *m) }
}

pub unsafe fn location(x: f64, y: f64, v: *mut LLocation) {
    unsafe { location_xy(x, y, &mut *v) }
}

pub unsafe fn trans(vin: *const LLocation, m: *const LTransform, vout: *mut LLocation) {
    unsafe { trans_location(&*vin, &*m, &mut *vout) }
}
