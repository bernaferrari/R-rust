//! The par() state machine exactly as GNU's par.c semantics pin it —
//! dumb override storage for most parameters, the layout cascade for
//! the region-coupled ones, and the new/page/pending-mfg interactions
//! the oracle fixtures byte-pin.

use super::layout::{LayoutResult, LayoutState, ParValue, Regions};
use std::collections::BTreeMap;
use r_graphics_engine::{Color, DrawOperation, Scene};

/// GNU's default logical values — the small set the fixtures read.
fn default_logical(name: &str) -> Option<bool> {
    match name {
        "xlog" | "ylog" | "ask" | "new" | "xpd" => Some(false),
        "ann" => Some(true),
        _ => None,
    }
}

/// The whole par/device state a session carries — one device per
/// session exactly as the portable contract's single render target.
#[derive(Debug)]
pub(crate) struct GraphicsParState {
    pub layout: LayoutState,
    overrides: BTreeMap<String, ParValue>,
    /// GNU keeps `par(mfg=)`'s implicit `new=TRUE` on an unused device
    /// until the first GNewPlot consumes it.
    pending_mfg_before_first_plot: bool,
    /// Whether any plot frame has begun on this device — GNU's
    /// plot.new-called device flag.
    frame_started: bool,
    /// The device's raster geometry at 72dpi — owner-installed.
    device_width: f64,
    device_height: f64,
    /// The frame's owned display list — the pure world's rendering
    /// output exactly as GNU's device bitmap, backend-neutral so
    /// embedders replay onto any target.
    scene: Scene,
    /// Pages begun on this device — the clear events the reset display
    /// list itself can no longer distinguish.
    page_count: u32,
}

/// The validated figure transition exactly as the old world's
/// prepare/commit split — a failed region check consumes nothing.
pub(crate) struct PlotFigureTransition {
    layout: Option<LayoutState>,
    pending_mfg_before_first_plot: bool,
}

impl Default for GraphicsParState {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphicsParState {
    pub(crate) fn new() -> Self {
        Self {
            layout: LayoutState::default(),
            overrides: BTreeMap::new(),
            pending_mfg_before_first_plot: false,
            frame_started: false,
            device_width: 504.0,
            device_height: 504.0,
            scene: Scene::new(504, 504),
            page_count: 0,
        }
    }

    /// Read the frame's display list — embedders replay or diff it.
    pub(crate) fn scene(&self) -> &Scene {
        &self.scene
    }

    /// Mutable scene access for primitive emission exactly as GNU's
    /// device holds the frame under construction.
    pub(crate) fn scene_mut(&mut self) -> &mut Scene {
        &mut self.scene
    }

    /// Begin a fresh page exactly as GNU's device clear: the display
    /// list resets with the background clear as its first operation.
    pub(crate) fn clear_scene(&mut self) {
        self.scene.reset();
        self.scene.push(DrawOperation::Clear(Color::WHITE));
        self.page_count += 1;
    }

    /// How many pages this device has begun — embedders diffing the
    /// scene across evals use the counter to detect page turns.
    pub(crate) fn page_count(&self) -> u32 {
        self.page_count
    }

    /// Install the device raster geometry and refresh the cascade
    /// exactly as a device replacement does.
    pub(crate) fn set_device_size(&mut self, width: f64, height: f64) {
        self.device_width = width;
        self.device_height = height;
        self.layout.set_device_size(width, height);
        self.layout.refresh(self.cexbase(), self.mex(), 1.0);
        self.scene = Scene::new(width.round().max(1.0) as u32, height.round().max(1.0) as u32);
    }

    pub(crate) fn frame_started(&self) -> bool {
        self.frame_started
    }

    fn cexbase(&self) -> f64 {
        self.real_override("cex", 1.0)
    }

    fn mex(&self) -> f64 {
        self.real_override("mex", 1.0)
    }

    pub(crate) fn logical_override(&self, name: &str) -> bool {
        match self.overrides.get(name) {
            Some(ParValue::Logical(values)) if values.first().is_some_and(|v| *v != 0) => true,
            _ => default_logical(name).unwrap_or(false),
        }
    }

    fn real_override(&self, name: &str, fallback: f64) -> f64 {
        match self.overrides.get(name) {
            Some(ParValue::Real(values)) if values.len() == 1 && values[0].is_finite() => values[0],
            Some(ParValue::Integer(values)) if values.len() == 1 && values[0] != i32::MIN => {
                f64::from(values[0])
            }
            _ => fallback,
        }
    }

    /// GNU's par.c page logic exactly: with `new` set the next plot
    /// starts a page only when no frame ever began; otherwise the
    /// layout's wrap position decides.
    pub(crate) fn starts_new_page(&self) -> bool {
        if self.logical_override("new") {
            !self.frame_started
        } else {
            self.layout.starts_new_page()
        }
    }

    /// Read a parameter exactly as par("name") — layout-owned names
    /// cascade, page computes, everything else reads the override or
    /// GNU's default.
    pub(crate) fn query(&self, name: &str) -> Option<ParValue> {
        match name {
            "page" => Some(ParValue::Logical(vec![i32::from(self.starts_new_page())])),
            "new" => Some(ParValue::Logical(vec![i32::from(self.logical_override("new"))])),
            "cex" => Some(ParValue::Real(vec![self.cexbase()])),
            "mex" => Some(ParValue::Real(vec![self.mex()])),
            _ => {
                if let Some(value) = self.layout.query(name) {
                    return Some(value);
                }
                if let Some(value) = self.overrides.get(name) {
                    return Some(value.clone());
                }
                default_logical(name).map(|v| ParValue::Logical(vec![i32::from(v)]))
            }
        }
    }

    /// Apply a parameter exactly as GNU's Specify/Specify2 cascade —
    /// region-coupled names drive the layout, mfrow rewrites cex/mex,
    /// mfg establishes the implicit new, and everything else is plain
    /// override storage. Warnings surface as strings exactly as GNU's
    /// mfg nr/nc mismatches.
    pub(crate) fn apply(&mut self, name: &str, value: &ParValue) -> LayoutResult<Vec<String>> {
        let cexbase = self.cexbase();
        let mex = self.mex();
        let mut warnings = Vec::new();
        match name {
            "mai" => self.layout.set_mai(value, cexbase, mex, 1.0)?,
            "mar" => self.layout.set_mar(value, cexbase, mex, 1.0)?,
            "oma" => self.layout.set_oma(value, cexbase, mex, 1.0)?,
            "omi" => self.layout.set_omi(value, cexbase, mex, 1.0)?,
            "omd" => self.layout.set_omd(value, cexbase, mex, 1.0)?,
            "fig" => self.layout.set_fig(value, cexbase, mex, 1.0)?,
            "fin" => self.layout.set_fin(value, cexbase, mex, 1.0)?,
            "pin" => self.layout.set_pin(value, cexbase, mex, 1.0)?,
            "plt" => self.layout.set_plt(value, cexbase, mex, 1.0)?,
            "pty" => {
                if let ParValue::String(text) = value {
                    self.layout.set_pty(text)?;
                } else {
                    return Err("invalid value specified for graphical parameter \"pty\"".into());
                }
            }
            "mfrow" | "mfcol" => {
                let (nrow, ncol) = self.layout.set_mfrow(value, name == "mfcol")?;
                let cex = LayoutState::split_cex(nrow, ncol);
                self.overrides.insert("cex".into(), ParValue::Real(vec![cex]));
                self.overrides.insert("mex".into(), ParValue::Real(vec![1.0]));
            }
            "mfg" => {
                warnings = self.layout.set_mfg(value, cexbase, mex, 1.0)?;
                self.overrides.insert("new".into(), ParValue::Logical(vec![1]));
                self.pending_mfg_before_first_plot = !self.frame_started;
            }
            "mex" => {
                let new_mex = self.layout.check_mex(value)?;
                self.overrides.insert("mex".into(), ParValue::Real(vec![new_mex]));
                self.layout.refresh(cexbase, new_mex, 1.0);
            }
            "cex" => {
                let values = match value {
                    ParValue::Real(v) if v.len() == 1 => v[0],
                    ParValue::Integer(v) | ParValue::Logical(v) if v.len() == 1 => {
                        f64::from(v[0])
                    }
                    _ => {
                        return Err(
                            "invalid value specified for graphical parameter \"cex\"".into()
                        )
                    }
                };
                self.overrides.insert("cex".into(), ParValue::Real(vec![values]));
                self.layout.refresh(values, mex, 1.0);
            }
            "new" => {
                let truthy = match value {
                    ParValue::Logical(values) | ParValue::Integer(values) => {
                        values.first().is_some_and(|v| *v != 0)
                    }
                    _ => return Err("invalid value specified for graphical parameter \"new\"".into()),
                };
                // GNU keeps the implicit `new=TRUE` established by
                // par(mfg=) on an unused device; the first GNewPlot
                // still resets to cell 1.
                let forced = !self.frame_started && self.pending_mfg_before_first_plot && !truthy;
                self.overrides.insert(
                    "new".into(),
                    ParValue::Logical(vec![i32::from(truthy || forced)]),
                );
            }
            _ => {
                self.overrides.insert(name.to_owned(), value.clone());
            }
        }
        Ok(warnings)
    }

    /// GNewPlot exactly as the old world's prepare half — the clone
    /// means a failed region check consumes neither cell nor page.
    /// Returns the candidate regions, whether the page clears, and the
    /// transition to commit after validation.
    pub(crate) fn prepare_plot_figure(&mut self) -> (Regions, bool, PlotFigureTransition) {
        let cexbase = self.cexbase();
        let mex = self.mex();
        if self.logical_override("new") && self.frame_started {
            return (
                self.layout.regions(),
                false,
                PlotFigureTransition {
                    layout: None,
                    pending_mfg_before_first_plot: self.pending_mfg_before_first_plot,
                },
            );
        }
        let mut layout = self.layout.clone();
        let mut pending = self.pending_mfg_before_first_plot;
        let clear = if !self.frame_started {
            pending = false;
            layout.begin_first_figure(cexbase, mex, 1.0)
        } else {
            layout.begin_figure(cexbase, mex, 1.0)
        };
        let regions = layout.regions();
        (
            regions,
            clear,
            PlotFigureTransition {
                layout: Some(layout),
                pending_mfg_before_first_plot: pending,
            },
        )
    }

    /// Commit after the candidate regions validate — the frame has
    /// now begun exactly as GNU's device flag.
    pub(crate) fn commit_plot_figure(&mut self, transition: PlotFigureTransition) {
        if let Some(layout) = transition.layout {
            self.layout = layout;
            self.pending_mfg_before_first_plot = transition.pending_mfg_before_first_plot;
        }
        self.frame_started = true;
    }
}
