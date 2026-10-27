//! Device-layout contract for the coupled `par()` region parameters.
//!
//! Mirrors the `GReset` → `mapping()` cascade of
//! r-source/src/library/graphics/src/graphics.c on the portable device's
//! nominal geometry (72 raster units per inch, `cra` 10.8/14.4, `ipr`
//! 1/72), keeping GNU's exact floating-point operation order so `par()`
//! queries byte-match the reference engine on the default 504x504 page.
//!
//! Owned parameters: mai/mar (+ mUnits), oma/omi/omd (+ oUnits),
//! fig/fin (+ fUnits), plt/pin (+ pUnits), pty, the mex-driven line
//! unit, mfrow/mfcol/mfg figure stepping, and the derived
//! din/csi/cin/cra/cxy queries. Everything else stays in the plain
//! override map.

use crate::library::graphics::par::{ParValue, par_error};
/// Portable-device nominal font metrics: the default device reports
/// `cra` in raster pixels at 72 dpi, derived from the 12pt nominal
/// pointsize exactly like `pdf()` (0.9/1.2 em; the products are NOT the
/// decimal literals 10.8/14.4, and the difference is observable in
/// par("mai")-style conversions).
const POINTSIZE: f64 = 12.0;
const CRA: [f64; 2] = [0.9 * POINTSIZE, 1.2 * POINTSIZE];
/// Inches per raster unit (72 dpi device).
const IPR: [f64; 2] = [1.0 / 72.0, 1.0 / 72.0];
/// Nominal page in raster units (7in x 7in at 72 dpi).
const DEFAULT_PAGE: [f64; 2] = [504.0, 504.0];

#[derive(Clone, Copy, PartialEq, Eq)]
enum MarginUnits {
    Lines,
    Inches,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OuterUnits {
    Lines,
    Inches,
    Ndc,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RegionUnits {
    /// NIC (fig) / NFC (plt): fractions of the enclosing region.
    Relative,
    Inches,
}

/// Raster-unit rectangles with y growing downward (portable device
/// convention), converted from GNU's y-up dev coordinates.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Regions {
    /// [left, top, right, bottom] of the whole device.
    pub device: [f64; 4],
    /// [left, top, right, bottom] of the inner (outer-margin-clipped)
    /// region.
    pub inner: [f64; 4],
    /// [left, top, right, bottom] of the current figure region.
    pub figure: [f64; 4],
    /// [left, top, right, bottom] of the current plot region.
    pub plot: [f64; 4],
}

pub(crate) struct LayoutState {
    // Device geometry in raster units.
    width: f64,
    height: f64,
    // mapNDC2Dev yardsticks captured at the last reset.
    x_ndc_per_inch: f64,
    y_ndc_per_inch: f64,
    x_ndc_per_char: f64,
    y_ndc_per_char: f64,
    x_ndc_per_line: f64,
    y_ndc_per_line: f64,
    // Outer margins.
    oma: [f64; 4],
    omi: [f64; 4],
    omd: [f64; 4],
    o_units: OuterUnits,
    // Inner region transform in dev units: [ax, bx, ay, by].
    inner2dev: [f64; 4],
    // Figure region.
    fig: [f64; 4],
    fin: [f64; 2],
    f_units: RegionUnits,
    default_figure: bool,
    layout_grid: bool,
    order: Vec<i32>,
    numrows: i32,
    numcols: i32,
    mfind: bool,
    current_figure: i32,
    last_figure: i32,
    // Figure transform in dev units: [ax, bx, ay, by].
    fig2dev: [f64; 4],
    // Figure margins.
    mar: [f64; 4],
    mai: [f64; 4],
    m_units: MarginUnits,
    default_plot: bool,
    pty: u8,
    // Plot region.
    plt: [f64; 4],
    pin: [f64; 2],
    p_units: RegionUnits,
}

impl Default for LayoutState {
    fn default() -> Self {
        let mut state = Self {
            width: DEFAULT_PAGE[0],
            height: DEFAULT_PAGE[1],
            x_ndc_per_inch: 0.0,
            y_ndc_per_inch: 0.0,
            x_ndc_per_char: 0.0,
            y_ndc_per_char: 0.0,
            x_ndc_per_line: 0.0,
            y_ndc_per_line: 0.0,
            oma: [0.0; 4],
            omi: [0.0; 4],
            omd: [0.0; 4],
            o_units: OuterUnits::Lines,
            inner2dev: [0.0; 4],
            fig: [0.0, 1.0, 0.0, 1.0],
            fin: [7.0; 2],
            f_units: RegionUnits::Relative,
            default_figure: true,
            layout_grid: false,
            order: Vec::new(),
            numrows: 1,
            numcols: 1,
            mfind: false,
            current_figure: 1,
            last_figure: 1,
            fig2dev: [0.0; 4],
            mar: [5.1, 4.1, 4.1, 2.1],
            mai: [0.0; 4],
            m_units: MarginUnits::Lines,
            default_plot: true,
            pty: b'm',
            plt: [0.0; 4],
            pin: [0.0; 2],
            p_units: RegionUnits::Relative,
        };
        state.reset_nominal(1.0, 1.0, 1.0);
        state
    }
}

// ---- unit conversions (GConvertX/YUnits hub, GNU operation order) ----

impl LayoutState {
    fn x_dev_from_lines(&self, v: f64) -> f64 {
        (v * self.x_ndc_per_line) * self.width
    }
    fn y_dev_from_lines(&self, v: f64) -> f64 {
        (v * self.y_ndc_per_line) * self.height
    }
    fn x_dev_from_inches(&self, v: f64) -> f64 {
        (v * self.x_ndc_per_inch) * self.width
    }
    fn y_dev_from_inches(&self, v: f64) -> f64 {
        (v * self.y_ndc_per_inch) * self.height
    }
    fn x_dev_from_ndc(&self, v: f64) -> f64 {
        v * self.width
    }
    fn y_dev_from_ndc(&self, v: f64) -> f64 {
        v * self.height
    }
    fn x_dev_to_inches(&self, dev: f64) -> f64 {
        (dev / self.width) / self.x_ndc_per_inch
    }
    fn y_dev_to_inches(&self, dev: f64) -> f64 {
        (dev / self.height) / self.y_ndc_per_inch
    }
    fn x_dev_to_lines(&self, dev: f64) -> f64 {
        (dev / self.width) / self.x_ndc_per_line
    }
    fn y_dev_to_lines(&self, dev: f64) -> f64 {
        (dev / self.height) / self.y_ndc_per_line
    }
    fn x_dev_to_ndc(&self, dev: f64) -> f64 {
        dev / self.width
    }
    fn y_dev_to_ndc(&self, dev: f64) -> f64 {
        dev / self.height
    }

    fn convert_x(&self, v: f64, from: MarginUnits, to: MarginUnits) -> f64 {
        let dev = match from {
            MarginUnits::Lines => self.x_dev_from_lines(v),
            MarginUnits::Inches => self.x_dev_from_inches(v),
        };
        match to {
            MarginUnits::Lines => self.x_dev_to_lines(dev),
            MarginUnits::Inches => self.x_dev_to_inches(dev),
        }
    }

    fn convert_y(&self, v: f64, from: MarginUnits, to: MarginUnits) -> f64 {
        let dev = match from {
            MarginUnits::Lines => self.y_dev_from_lines(v),
            MarginUnits::Inches => self.y_dev_from_inches(v),
        };
        match to {
            MarginUnits::Lines => self.y_dev_to_lines(dev),
            MarginUnits::Inches => self.y_dev_to_inches(dev),
        }
    }

    /// Axis mapping for margin indices: 0 and 2 are y (bottom/top),
    /// 1 and 3 are x (left/right).
    fn margin_is_x(index: usize) -> bool {
        index % 2 == 1
    }

    fn convert_margin(&self, v: f64, index: usize, from: MarginUnits, to: MarginUnits) -> f64 {
        if Self::margin_is_x(index) {
            self.convert_x(v, from, to)
        } else {
            self.convert_y(v, from, to)
        }
    }

    fn x_inch_to_nfc(&self, v: f64) -> f64 {
        self.x_dev_from_inches(v) / self.fig2dev[1].abs()
    }
    fn y_inch_to_nfc(&self, v: f64) -> f64 {
        self.y_dev_from_inches(v) / self.fig2dev[3].abs()
    }
    fn x_nfc_to_inch(&self, v: f64) -> f64 {
        self.x_dev_to_inches(v * self.fig2dev[1].abs())
    }
    fn y_nfc_to_inch(&self, v: f64) -> f64 {
        self.y_dev_to_inches(v * self.fig2dev[3].abs())
    }
}

// ---- the mapping() cascade ----

impl LayoutState {
    /// mapNDC2Dev: capture the unit yardsticks from the current base
    /// character expansion, line-spacing multiplier, and device font
    /// metrics (the thumbnail viewport admission scales the metrics).
    fn map_ndc2dev(&mut self, cexbase: f64, mex: f64, font_scale: f64) {
        // Geometry is refreshed by the callers (the device cannot be
        // queried during instance construction).
        let cra_height = CRA[1] * font_scale;
        let (width, height) = (self.width, self.height);
        self.x_ndc_per_inch = 1.0 / (width * IPR[0]).abs();
        self.y_ndc_per_inch = 1.0 / (height * IPR[1]).abs();
        // asp = ipr[1]/ipr[0] = 1 on the square-pixel portable device;
        // scale = 1 (no pointsize rescaling at capture time).
        self.x_ndc_per_char = (cexbase * cra_height / width).abs();
        self.y_ndc_per_char = (cexbase * cra_height / height).abs();
        self.x_ndc_per_line = (mex * cexbase * cra_height / width).abs();
        self.y_ndc_per_line = (mex * cexbase * cra_height / height).abs();
    }

    fn update_outer_margins(&mut self) {
        match self.o_units {
            OuterUnits::Lines => {
                for (i, value) in self.oma.iter().enumerate() {
                    self.omi[i] =
                        self.convert_margin(*value, i, MarginUnits::Lines, MarginUnits::Inches);
                }
            }
            OuterUnits::Inches => {
                for (i, value) in self.omi.iter().enumerate() {
                    self.oma[i] =
                        self.convert_margin(*value, i, MarginUnits::Inches, MarginUnits::Lines);
                }
            }
            OuterUnits::Ndc => {
                self.oma[0] = self.y_dev_to_lines(self.y_dev_from_ndc(self.omd[2]));
                self.oma[1] = self.x_dev_to_lines(self.x_dev_from_ndc(self.omd[0]));
                self.oma[2] = self.y_dev_to_lines(self.y_dev_from_ndc(1.0 - self.omd[3]));
                self.oma[3] = self.x_dev_to_lines(self.x_dev_from_ndc(1.0 - self.omd[1]));
                self.omi[0] = self.y_dev_to_inches(self.y_dev_from_ndc(self.omd[2]));
                self.omi[1] = self.x_dev_to_inches(self.x_dev_from_ndc(self.omd[0]));
                self.omi[2] = self.y_dev_to_inches(self.y_dev_from_ndc(1.0 - self.omd[3]));
                self.omi[3] = self.x_dev_to_inches(self.x_dev_from_ndc(1.0 - self.omd[1]));
            }
        }
        if self.o_units != OuterUnits::Ndc {
            self.omd[0] = self.x_dev_to_ndc(self.x_dev_from_inches(self.omi[1]));
            self.omd[1] = 1.0 - self.x_dev_to_ndc(self.x_dev_from_inches(self.omi[3]));
            self.omd[2] = self.y_dev_to_ndc(self.y_dev_from_inches(self.omi[0]));
            self.omd[3] = 1.0 - self.y_dev_to_ndc(self.y_dev_from_inches(self.omi[2]));
        }
    }

    fn map_inner2dev(&mut self) {
        let x0 = self.x_dev_from_lines(self.oma[1]);
        let y0 = self.y_dev_from_lines(self.oma[0]);
        let x1 = self.x_dev_from_ndc(1.0 - self.x_dev_to_ndc(self.x_dev_from_lines(self.oma[3])));
        let y1 = self.y_dev_from_ndc(1.0 - self.y_dev_to_ndc(self.y_dev_from_lines(self.oma[2])));
        self.inner2dev = [x0, x1 - x0, y0, y1 - y0];
    }

    fn map_figure_region(&mut self) {
        // 1-based (row, col) of currentFigure, GNU mapFigureRegion math.
        let (numrows, numcols) = (self.numrows, self.numcols);
        let cf = self.current_figure;
        let (row, col) = if self.layout_grid && !self.order.is_empty() {
            let (i, j) = self.figure_row_col();
            (i + 1, j + 1)
        } else if self.mfind {
            let col = (cf - 1) / numrows + 1;
            let row = cf - (col - 1) * numrows;
            (row, col)
        } else {
            let row = (cf - 1) / numcols + 1;
            let col = cf - (row - 1) * numcols;
            (row, col)
        };
        let x0 = f64::from(col - 1) / f64::from(numcols);
        let x1 = f64::from(col) / f64::from(numcols);
        let y0 = f64::from(numrows - row) / f64::from(numrows);
        let y1 = f64::from(numrows - row + 1) / f64::from(numrows);
        self.fig = [x0, x1, y0, y1];
        self.f_units = RegionUnits::Relative;
    }

    fn update_figure_region(&mut self) {
        match self.f_units {
            RegionUnits::Relative => {
                self.fin[0] =
                    self.x_dev_to_inches((self.fig[1] - self.fig[0]) * self.inner2dev[1].abs());
                self.fin[1] =
                    self.y_dev_to_inches((self.fig[3] - self.fig[2]) * self.inner2dev[3].abs());
            }
            RegionUnits::Inches => {
                let nic_width = self.x_dev_from_inches(self.fin[0]) / self.inner2dev[1].abs();
                let nic_height = self.y_dev_from_inches(self.fin[1]) / self.inner2dev[3].abs();
                self.fig[0] = 0.5 - nic_width / 2.0;
                self.fig[1] = self.fig[0] + nic_width;
                self.fig[2] = 0.5 - nic_height / 2.0;
                self.fig[3] = self.fig[2] + nic_height;
            }
        }
    }

    fn map_fig2dev(&mut self) {
        let x0 = self.inner2dev[0] + self.fig[0] * self.inner2dev[1];
        let x1 = self.inner2dev[0] + self.fig[1] * self.inner2dev[1];
        let y0 = self.inner2dev[2] + self.fig[2] * self.inner2dev[3];
        let y1 = self.inner2dev[2] + self.fig[3] * self.inner2dev[3];
        self.fig2dev = [x0, x1 - x0, y0, y1 - y0];
    }

    fn update_figure_margins(&mut self) {
        match self.m_units {
            MarginUnits::Lines => {
                for (i, value) in self.mar.iter().enumerate() {
                    self.mai[i] =
                        self.convert_margin(*value, i, MarginUnits::Lines, MarginUnits::Inches);
                }
            }
            MarginUnits::Inches => {
                for (i, value) in self.mai.iter().enumerate() {
                    self.mar[i] =
                        self.convert_margin(*value, i, MarginUnits::Inches, MarginUnits::Lines);
                }
            }
        }
    }

    fn map_plot_region(&mut self) {
        let mut x0 = self.x_dev_from_lines(self.mar[1]) / self.fig2dev[1].abs();
        let mut y0 = self.y_dev_from_lines(self.mar[0]) / self.fig2dev[3].abs();
        let mut x1 = 1.0 - self.x_dev_from_lines(self.mar[3]) / self.fig2dev[1].abs();
        let mut y1 = 1.0 - self.y_dev_from_lines(self.mar[2]) / self.fig2dev[3].abs();
        if self.pty == b's' {
            // Maximal plot size in inches; shrink the longer side.
            let inch_width = self.x_nfc_to_inch(x1 - x0);
            let inch_height = self.y_nfc_to_inch(y1 - y0);
            if inch_width > inch_height {
                let width = 0.5 * self.x_inch_to_nfc(inch_height);
                let center = 0.5 * (x1 + x0);
                x0 = center - width;
                x1 = center + width;
            } else {
                let height = 0.5 * self.y_inch_to_nfc(inch_width);
                let center = 0.5 * (y1 + y0);
                y0 = center - height;
                y1 = center + height;
            }
        }
        self.plt = [x0, x1, y0, y1];
        self.p_units = RegionUnits::Relative;
    }

    fn update_plot_region(&mut self) {
        match self.p_units {
            RegionUnits::Relative => {
                self.pin[0] = self.x_nfc_to_inch(self.plt[1] - self.plt[0]);
                self.pin[1] = self.y_nfc_to_inch(self.plt[3] - self.plt[2]);
            }
            RegionUnits::Inches => {
                let nfc_width = self.x_inch_to_nfc(self.pin[0]);
                let nfc_height = self.y_inch_to_nfc(self.pin[1]);
                self.plt[0] = 0.5 - nfc_width / 2.0;
                self.plt[1] = self.plt[0] + nfc_width;
                self.plt[2] = 0.5 - nfc_height / 2.0;
                self.plt[3] = self.plt[2] + nfc_height;
            }
        }
    }

    /// GReset against the nominal page: safe during instance
    /// construction, where no device can be queried yet.
    fn reset_nominal(&mut self, cexbase: f64, mex: f64, font_scale: f64) {
        self.map_ndc2dev(cexbase, mex, font_scale);
        self.apply_mapping();
    }

    /// GReset with the live device geometry.
    fn reset(&mut self, cexbase: f64, mex: f64, font_scale: f64) {
        self.refresh_device_geometry();
        self.map_ndc2dev(cexbase, mex, font_scale);
        self.apply_mapping();
    }

    fn apply_mapping(&mut self) {
        // mapping(dd, 0) falls through all four stages.
        self.update_outer_margins();
        self.map_inner2dev();
        if self.default_figure {
            self.map_figure_region();
        }
        self.update_figure_region();
        self.map_fig2dev();
        self.update_figure_margins();
        if self.default_plot {
            self.map_plot_region();
        }
        self.update_plot_region();
    }

    /// Re-run GReset with the given expansion and line spacing (used
    /// after a `par(mex=)` style change).
    pub(crate) fn refresh(&mut self, cexbase: f64, mex: f64, font_scale: f64) {
        self.refresh_device_geometry();
        self.reset(cexbase, mex, font_scale);
    }

    /// par("page") logic: does the next plot start a new page?
    pub(crate) fn starts_new_page(&self) -> bool {
        self.current_figure >= self.last_figure
    }

    pub(crate) fn refresh_if_device_geometry_changed(
        &mut self,
        cexbase: f64,
        mex: f64,
        font_scale: f64,
    ) {
        let (width, height) = device_size_raster();
        if self.width == width && self.height == height {
            return;
        }
        self.width = width;
        self.height = height;
        self.map_ndc2dev(cexbase, mex, font_scale);
        self.apply_mapping();
    }

    fn refresh_device_geometry(&mut self) {
        let (width, height) = device_size_raster();
        self.width = width;
        self.height = height;
    }

    /// 0-based (row, col) of `current_figure` (currentFigureLocation).
    fn figure_row_col(&self) -> (i32, i32) {
        if self.layout_grid && !self.order.is_empty() {
            let target = self.current_figure;
            let nrow = self.numrows;
            for j in 0..self.numcols {
                let Some(column_offset) = (j as usize).checked_mul(nrow as usize) else {
                    continue;
                };
                for i in 0..nrow {
                    let Some(index) = (i as usize).checked_add(column_offset) else {
                        continue;
                    };
                    if self
                        .order
                        .get(index)
                        .is_some_and(|figure| *figure == target)
                    {
                        return (i, j);
                    }
                }
            }
            (0, 0)
        } else if self.mfind {
            let zero = self.current_figure - 1;
            (zero % self.numrows, zero / self.numrows)
        } else {
            let zero = self.current_figure - 1;
            (zero / self.numcols, zero % self.numcols)
        }
    }
}

// ---- Specify-style setters ----

fn nonneg(value: f64, name: &str) {
    if !value.is_finite() || value < 0.0 {
        par_error(format!(
            "invalid value specified for graphical parameter \"{name}\""
        ));
    }
}

fn positive(value: f64, name: &str) {
    if !value.is_finite() || value <= 0.0 {
        par_error(format!(
            "invalid value specified for graphical parameter \"{name}\""
        ));
    }
}

/// GNU coerces the value to REALSXP first, so integer and logical
/// vectors are read elementwise; then lengthCheck applies.
fn real_values(value: &ParValue, name: &str, n: usize) -> Vec<f64> {
    let raw: Vec<f64> = match value {
        ParValue::Real(values) => values.iter().copied().collect(),
        ParValue::Integer(values) | ParValue::Logical(values) => {
            values.iter().map(|v| f64::from(*v)).collect()
        }
        _ => par_error(format!(
            "invalid value specified for graphical parameter \"{name}\""
        )),
    };
    if n != usize::MAX && raw.len() != n {
        par_error(format!(
            "graphical parameter \"{name}\" has the wrong length"
        ));
    }
    raw
}

impl LayoutState {
    /// GNU's cex policy at an mfrow/mfcol/layout split.
    pub(crate) fn split_cex(nrow: i32, ncol: i32) -> f64 {
        if nrow > 2 || ncol > 2 {
            0.66
        } else if nrow == 2 && ncol == 2 {
            0.83
        } else {
            1.0
        }
    }

    pub(crate) fn set_mai(&mut self, value: &ParValue, cexbase: f64, mex: f64, font_scale: f64) {
        let values = real_values(value, "mai", 4);
        for v in &values {
            nonneg(*v, "mai");
        }
        self.mai = [values[0], values[1], values[2], values[3]];
        self.m_units = MarginUnits::Inches;
        self.default_plot = true;
        self.reset(cexbase, mex, font_scale);
    }

    pub(crate) fn set_mar(&mut self, value: &ParValue, cexbase: f64, mex: f64, font_scale: f64) {
        let values = real_values(value, "mar", 4);
        for v in &values {
            nonneg(*v, "mar");
        }
        self.mar = [values[0], values[1], values[2], values[3]];
        self.m_units = MarginUnits::Lines;
        self.default_plot = true;
        self.reset(cexbase, mex, font_scale);
    }

    /// Validates a mex= specification and returns its value.
    pub(crate) fn check_mex(&mut self, value: &ParValue) -> f64 {
        let values = real_values(value, "mex", 1);
        positive(values[0], "mex");
        values[0]
    }

    pub(crate) fn set_oma(&mut self, value: &ParValue, cexbase: f64, mex: f64, font_scale: f64) {
        let values = real_values(value, "oma", 4);
        for v in &values {
            nonneg(*v, "oma");
        }
        self.oma = [values[0], values[1], values[2], values[3]];
        self.o_units = OuterUnits::Lines;
        // Force eject of multiple figures.
        self.current_figure = self.last_figure;
        self.reset(cexbase, mex, font_scale);
    }

    pub(crate) fn set_omi(&mut self, value: &ParValue, cexbase: f64, mex: f64, font_scale: f64) {
        let values = real_values(value, "omi", 4);
        for v in &values {
            nonneg(*v, "omi");
        }
        self.omi = [values[0], values[1], values[2], values[3]];
        self.o_units = OuterUnits::Inches;
        self.current_figure = self.last_figure;
        self.reset(cexbase, mex, font_scale);
    }

    pub(crate) fn set_omd(&mut self, value: &ParValue, cexbase: f64, mex: f64, font_scale: f64) {
        let values = real_values(value, "omd", 4);
        for v in &values {
            if !v.is_finite() || *v < 0.0 || *v > 1.0 {
                par_error("invalid value specified for graphical parameter \"omd\"");
            }
        }
        self.omd = [values[0], values[1], values[2], values[3]];
        self.o_units = OuterUnits::Ndc;
        self.current_figure = self.last_figure;
        self.reset(cexbase, mex, font_scale);
    }

    pub(crate) fn set_fig(&mut self, value: &ParValue, cexbase: f64, mex: f64, font_scale: f64) {
        let values = real_values(value, "fig", 4);
        if !(0.0 <= values[0] && values[0] < values[1] && values[1] <= 1.0)
            || !(0.0 <= values[2] && values[2] < values[3] && values[3] <= 1.0)
        {
            par_error("invalid value specified for graphical parameter \"fig\"");
        }
        self.default_figure = false;
        self.f_units = RegionUnits::Relative;
        self.numrows = 1;
        self.numcols = 1;
        self.current_figure = 1;
        self.last_figure = 1;
        self.layout_grid = false;
        self.order.clear();
        self.fig = [values[0], values[1], values[2], values[3]];
        self.reset(cexbase, mex, font_scale);
    }

    pub(crate) fn set_fin(&mut self, value: &ParValue, cexbase: f64, mex: f64, font_scale: f64) {
        let values = real_values(value, "fin", 2);
        self.default_figure = false;
        self.f_units = RegionUnits::Inches;
        self.numrows = 1;
        self.numcols = 1;
        self.current_figure = 1;
        self.last_figure = 1;
        self.layout_grid = false;
        self.order.clear();
        self.fin = [values[0], values[1]];
        self.reset(cexbase, mex, font_scale);
    }

    pub(crate) fn set_pin(&mut self, value: &ParValue, cexbase: f64, mex: f64, font_scale: f64) {
        let values = real_values(value, "pin", 2);
        for v in &values {
            nonneg(*v, "pin");
        }
        self.pin = [values[0], values[1]];
        self.p_units = RegionUnits::Inches;
        self.default_plot = false;
        self.reset(cexbase, mex, font_scale);
    }

    pub(crate) fn set_plt(&mut self, value: &ParValue, cexbase: f64, mex: f64, font_scale: f64) {
        let values = real_values(value, "plt", 4);
        for v in &values {
            nonneg(*v, "plt");
        }
        self.plt = [values[0], values[1], values[2], values[3]];
        self.p_units = RegionUnits::Relative;
        self.default_plot = false;
        self.reset(cexbase, mex, font_scale);
    }

    /// par(pty=): GNU stores the value but does not run GReset here;
    /// the plot region is rebuilt at the next plot.new.
    pub(crate) fn set_pty(&mut self, value: &str) {
        let first = value.chars().next().unwrap_or('\0');
        if first == 'm' || first == 's' {
            self.pty = first as u8;
            self.default_plot = true;
        } else {
            par_error("invalid value specified for graphical parameter \"pty\"");
        }
    }

    /// par(mfrow=) / par(mfcol=): validates and installs the grid,
    /// returning (nrow, ncol).
    pub(crate) fn set_mfrow(&mut self, value: &ParValue, column_first: bool) -> (i32, i32) {
        let name = if column_first { "mfcol" } else { "mfrow" };
        let values = real_values(value, name, 2);
        let mut dimensions = [0; 2];
        for (dimension, value) in dimensions.iter_mut().zip(values) {
            // GNU first coerces mfrow/mfcol through an integer vector. This
            // truncates finite fractional values toward zero; it does not
            // require the input doubles to already be integral.
            let truncated = value.trunc();
            if !value.is_finite() || truncated < 1.0 || truncated > f64::from(i32::MAX) {
                par_error(format!(
                    "invalid value specified for graphical parameter \"{name}\""
                ));
            }
            *dimension = truncated as i32;
        }
        let (nrow, ncol) = (dimensions[0], dimensions[1]);
        let figure_count = nrow.checked_mul(ncol).unwrap_or_else(|| {
            par_error(format!(
                "invalid value specified for graphical parameter \"{name}\""
            ));
        });
        self.numrows = nrow;
        self.numcols = ncol;
        self.current_figure = figure_count;
        self.last_figure = figure_count;
        self.default_figure = true;
        self.layout_grid = false;
        self.order.clear();
        self.mfind = column_first;
        // GNU's split policy: the reset runs with the reduced base
        // expansion and mex restored to 1.
        self.reset(Self::split_cex(nrow, ncol), 1.0, 1.0);
        (nrow, ncol)
    }

    /// par(mfg=): returns the warning GNU emits for a wrong nr/nc
    /// declaration, if any.
    pub(crate) fn set_mfg(
        &mut self,
        value: &ParValue,
        cexbase: f64,
        mex: f64,
        font_scale: f64,
    ) -> Vec<String> {
        let raw = real_values(value, "mfg", usize::MAX);
        let np = raw.len();
        if np != 2 && np != 4 {
            par_error("parameter \"mfg\" has the wrong length");
        }
        let row = raw[0] as i32;
        let col = raw[1] as i32;
        if row <= 0 || col <= 0 {
            par_error("invalid value specified for graphical parameter \"mfg\"");
        }
        let (nrow, ncol) = (self.numrows, self.numcols);
        if row > nrow {
            par_error("parameter \"i\" in \"mfg\" is out of range");
        }
        if col > ncol {
            par_error("parameter \"j\" in \"mfg\" is out of range");
        }
        let mut warnings = Vec::new();
        if np == 4 {
            let declared_row = raw[2] as i32;
            let declared_col = raw[3] as i32;
            if declared_row <= 0 || declared_col <= 0 {
                par_error("invalid value specified for graphical parameter \"mfg\"");
            }
            if nrow != declared_row {
                warnings.push("value of 'nr' in \"mfg\" is wrong and will be ignored".to_owned());
            }
            if ncol != declared_col {
                warnings.push("value of 'nc' in \"mfg\" is wrong and will be ignored".to_owned());
            }
        }
        self.last_figure = nrow.checked_mul(ncol).unwrap_or_else(|| {
            par_error("invalid value specified for graphical parameter \"mfg\"");
        });
        self.current_figure = if self.mfind {
            (col - 1)
                .checked_mul(nrow)
                .and_then(|index| index.checked_add(row))
                .unwrap_or_else(|| {
                    par_error("invalid value specified for graphical parameter \"mfg\"");
                })
        } else {
            (row - 1)
                .checked_mul(ncol)
                .and_then(|index| index.checked_add(col))
                .unwrap_or_else(|| {
                    par_error("invalid value specified for graphical parameter \"mfg\"");
                })
        };
        self.reset(cexbase, mex, font_scale);
        warnings
    }

    /// layout(): grid with an explicit order matrix. Region widths and
    /// heights are honored in uniform proportions (portable contract);
    /// mfind keeps its previous value as in GNU's C_layout.
    pub(crate) fn set_layout(
        &mut self,
        nrow: i32,
        ncol: i32,
        order: &[i32],
        num_figures: i32,
        cexbase: f64,
        mex: f64,
        font_scale: f64,
    ) {
        if nrow <= 0 || ncol <= 0 || num_figures <= 0 {
            par_error("invalid value specified for graphics layout");
        }
        self.numrows = nrow;
        self.numcols = ncol;
        self.order = order.to_vec();
        self.current_figure = num_figures;
        self.last_figure = num_figures;
        self.default_figure = true;
        self.layout_grid = true;
        self.reset(cexbase, mex, font_scale);
    }

    /// GNewPlot region half: advance to the next figure (wrapping to a
    /// new page), then GReset with the base character expansion and
    /// line spacing captured for this plot. Returns true when the
    /// advance started a new page.
    pub(crate) fn begin_figure(&mut self, cexbase: f64, mex: f64, font_scale: f64) -> bool {
        self.refresh_device_geometry();
        let mut new_page = false;
        if self.current_figure >= self.last_figure {
            self.current_figure = 1;
            new_page = true;
        } else {
            self.current_figure += 1;
        }
        self.reset(cexbase, mex, font_scale);
        new_page
    }

    /// Y-down raster rectangles for the drawing layer.
    pub(crate) fn regions(&self) -> Regions {
        let height = self.height;
        let device = [0.0, 0.0, self.width, height];
        let inner = [
            self.inner2dev[0],
            height - (self.inner2dev[2] + self.inner2dev[3]),
            self.inner2dev[0] + self.inner2dev[1],
            height - self.inner2dev[2],
        ];
        let figure = [
            self.fig2dev[0],
            height - (self.fig2dev[2] + self.fig2dev[3]),
            self.fig2dev[0] + self.fig2dev[1],
            height - self.fig2dev[2],
        ];
        let x0 = self.fig2dev[0] + self.plt[0] * self.fig2dev[1];
        let x1 = self.fig2dev[0] + self.plt[1] * self.fig2dev[1];
        let y_top = self.fig2dev[2] + self.plt[3] * self.fig2dev[3];
        let y_bottom = self.fig2dev[2] + self.plt[2] * self.fig2dev[3];
        let plot = [x0, height - y_top, x1, height - y_bottom];
        Regions {
            device,
            inner,
            figure,
            plot,
        }
    }

    pub(crate) fn query(&self, name: &str) -> Option<ParValue> {
        let value = match name {
            "mai" => ParValue::Real(self.mai.to_vec()),
            "mar" => ParValue::Real(self.mar.to_vec()),
            "oma" => ParValue::Real(self.oma.to_vec()),
            "omi" => ParValue::Real(self.omi.to_vec()),
            "omd" => ParValue::Real(self.omd.to_vec()),
            "fig" => ParValue::Real(self.fig.to_vec()),
            "fin" => ParValue::Real(self.fin.to_vec()),
            "plt" => ParValue::Real(self.plt.to_vec()),
            "pin" => ParValue::Real(self.pin.to_vec()),
            "din" => ParValue::Real(vec![
                self.x_dev_to_inches(1.0 * self.width),
                self.y_dev_to_inches(1.0 * self.height),
            ]),
            // GConvertYUnits(1, CHARS, INCHES) with the restored cex of 1.
            "csi" => ParValue::Real(vec![
                self.y_dev_to_inches(1.0 * self.y_ndc_per_char * self.height),
            ]),
            "cra" => ParValue::Real(CRA.to_vec()),
            "cin" => ParValue::Real(vec![CRA[0] * IPR[0], CRA[1] * IPR[1]]),
            "mfrow" | "mfcol" => ParValue::Integer(vec![self.numrows, self.numcols]),
            "mfg" => {
                let (row, col) = self.figure_row_col();
                ParValue::Integer(vec![row + 1, col + 1, self.numrows, self.numcols])
            }
            _ => return None,
        };
        Some(value)
    }

    /// par("cxy"): character size in user units from the current usr
    /// and plot region (GNU's ldexp overflow-safe form).
    pub(crate) fn query_cxy(&self, usr: [f64; 4]) -> ParValue {
        let span = |hi: f64, lo: f64| hi * 0.5 - lo * 0.5;
        ParValue::Real(vec![
            CRA[0] * IPR[0] / self.pin[0] * span(usr[1], usr[0]) * 2.0,
            CRA[1] * IPR[1] / self.pin[1] * span(usr[3], usr[2]) * 2.0,
        ])
    }
}

#[cfg(feature = "renderplot-device")]
fn device_size_raster() -> (f64, f64) {
    crate::sexp::instance::with_current_instance(|inst| unsafe {
        match (*inst).current_renderplot_backend {
            Some(backend) => {
                let (w, h) = (*backend).dimensions();
                (f64::from(w), f64::from(h))
            }
            None => (DEFAULT_PAGE[0], DEFAULT_PAGE[1]),
        }
    })
    .unwrap_or((DEFAULT_PAGE[0], DEFAULT_PAGE[1]))
}

#[cfg(not(feature = "renderplot-device"))]
fn device_size_raster() -> (f64, f64) {
    (DEFAULT_PAGE[0], DEFAULT_PAGE[1])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reals(value: ParValue) -> Vec<f64> {
        match value {
            ParValue::Real(values) => values,
            _ => panic!("expected real par value"),
        }
    }

    fn real(values: &[f64]) -> ParValue {
        ParValue::Real(values.to_vec())
    }

    /// Constants below are %.17g oracle outputs from R-devel (pdf 7x7,
    /// 72 dpi) captured via the pinned Rscript oracle; they pin GNU's
    /// floating-point operation order.
    #[test]
    fn defaults_match_oracle_pdf_page() {
        let layout = LayoutState::default();
        assert_eq!(
            reals(layout.query("mai").unwrap()),
            vec![
                1.0199999999999998,
                0.81999999999999984,
                0.81999999999999984,
                0.41999999999999998
            ]
        );
        assert_eq!(
            reals(layout.query("plt").unwrap()),
            vec![
                0.11714285714285712,
                0.94000000000000006,
                0.14571428571428569,
                0.8828571428571429
            ]
        );
        assert_eq!(
            reals(layout.query("pin").unwrap()),
            vec![5.7600000000000007, 5.160000000000001]
        );
        assert_eq!(reals(layout.query("fin").unwrap()), vec![7.0, 7.0]);
        assert_eq!(reals(layout.query("din").unwrap()), vec![7.0, 7.0]);
        assert_eq!(
            reals(layout.query("csi").unwrap()),
            vec![0.19999999999999998]
        );
        assert_eq!(
            reals(layout.query("cin").unwrap()),
            vec![0.14999999999999999, 0.19999999999999996]
        );
    }

    #[test]
    fn mai_mar_omi_oma_couple_like_gnu() {
        let mut layout = LayoutState::default();
        layout.set_mai(&real(&[1.0, 2.0, 3.0, 4.0]), 1.0, 1.0, 1.0);
        assert_eq!(
            reals(layout.query("mar").unwrap()),
            vec![
                5.0000000000000009,
                10.000000000000002,
                15.000000000000002,
                20.000000000000004
            ]
        );
        layout.set_omi(&real(&[1.0, 2.0, 3.0, 4.0]), 1.0, 1.0, 1.0);
        assert_eq!(
            reals(layout.query("oma").unwrap()),
            vec![
                5.0000000000000009,
                10.000000000000002,
                15.000000000000002,
                20.000000000000004
            ]
        );
        assert_eq!(
            reals(layout.query("fin").unwrap()),
            vec![1.0000000000000004, 3.0]
        );
        layout.set_oma(&real(&[4.0, 3.0, 2.0, 1.0]), 1.0, 1.0, 1.0);
        assert_eq!(
            reals(layout.query("omi").unwrap()),
            vec![
                0.79999999999999993,
                0.59999999999999998,
                0.39999999999999997,
                0.19999999999999998
            ]
        );
    }

    #[test]
    fn mfrow_split_policy_rescales_margins_and_csi() {
        let mut layout = LayoutState::default();
        layout.set_mfrow(&ParValue::Integer(vec![2, 2]), false);
        assert_eq!(
            reals(layout.query("csi").unwrap()),
            vec![0.16599999999999998]
        );
        assert_eq!(
            reals(layout.query("mai").unwrap()),
            vec![
                0.84659999999999991,
                0.68059999999999998,
                0.68059999999999998,
                0.34860000000000002
            ]
        );
        layout.begin_figure(0.83, 1.0, 1.0);
        assert_eq!(
            reals(layout.query("fig").unwrap()),
            vec![0.0, 0.5, 0.5, 1.0]
        );
        assert_eq!(reals(layout.query("fin").unwrap()), vec![3.5, 3.5]);
        assert_eq!(
            reals(layout.query("pin").unwrap()),
            vec![2.4708000000000006, 1.9728000000000001]
        );
        assert_eq!(
            layout.query("mfg").unwrap(),
            ParValue::Integer(vec![1, 1, 2, 2])
        );
    }

    #[test]
    fn maximum_figure_count_has_safe_page_and_mfg_arithmetic() {
        let mut layout = LayoutState::default();
        let max = f64::from(i32::MAX);
        layout.set_mfrow(&real(&[1.0, max]), false);
        assert!(layout.starts_new_page());
        assert_eq!(
            layout.query("mfg").unwrap(),
            ParValue::Integer(vec![1, i32::MAX, 1, i32::MAX])
        );

        layout.set_mfg(&real(&[1.0, max]), 1.0, 1.0, 1.0);
        assert!(layout.starts_new_page());
        assert!(layout.begin_figure(1.0, 1.0, 1.0));
        assert_eq!(
            layout.query("mfg").unwrap(),
            ParValue::Integer(vec![1, 1, 1, i32::MAX])
        );
    }

    #[test]
    fn figure_count_overflow_is_rejected_before_mutating_the_grid() {
        let mut layout = LayoutState::default();
        layout.set_mfrow(&ParValue::Integer(vec![2, 3]), false);
        let before = layout.query("mfg").unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            layout.set_mfrow(&real(&[50_000.0, 50_000.0]), false);
        }));
        assert!(result.is_err());
        assert_eq!(layout.query("mfg").unwrap(), before);
        assert_eq!(
            layout.query("mfrow").unwrap(),
            ParValue::Integer(vec![2, 3])
        );
    }

    #[test]
    fn mex_doubles_the_line_unit_without_touching_csi() {
        let mut layout = LayoutState::default();
        let mex = layout.check_mex(&real(&[2.0]));
        layout.refresh(1.0, mex, 1.0);
        assert_eq!(
            reals(layout.query("csi").unwrap()),
            vec![0.19999999999999998]
        );
        assert_eq!(
            reals(layout.query("mai").unwrap()),
            vec![
                2.0399999999999996,
                1.6399999999999997,
                1.6399999999999997,
                0.83999999999999997
            ]
        );
    }
}
