use super::pdf_capture::PdfCapture;
use crate::sexp::instance::with_current_instance;
use r_graphics_engine::{Color, DrawTarget, Path, PlotParameters, Point, RasterImage, TextMetrics};
use std::{cell::RefCell, rc::Rc};

pub(super) type Capture = Rc<RefCell<Option<PdfCapture>>>;

/// A scoped UI target or a retained PDF recorder. A recorder handle can cross
/// interpreter calls without borrowing the registry or keeping a closed device live.
pub(crate) struct GraphicsTarget(Backend);

enum Backend {
    Ui {
        pointer: *mut dyn DrawTarget,
        owner: crate::sexp::heap::HeapIdentity,
    },
    Pdf(Capture),
}

impl GraphicsTarget {
    pub(super) fn pdf(capture: Capture) -> Self {
        Self(Backend::Pdf(capture))
    }

    pub(super) fn ui() -> Option<Self> {
        with_current_instance(|instance| unsafe {
            (*instance).current_renderplot_backend.map(|pointer| {
                Self(Backend::Ui {
                    pointer,
                    owner: (*instance).heap_identity.clone(),
                })
            })
        })
        .flatten()
    }

    pub(crate) fn is_pdf(&self) -> bool {
        matches!(self.0, Backend::Pdf(_))
    }

    pub(crate) fn write_pdf_page(
        &self,
        output: &mut dyn std::io::Write,
    ) -> Option<std::io::Result<()>> {
        if let Backend::Pdf(capture) = &self.0 {
            return Some((|| {
                capture
                    .try_borrow()
                    .map_err(|_| std::io::Error::other("PDF device is busy"))?
                    .as_ref()
                    .ok_or_else(|| std::io::Error::other("PDF device is closed"))?
                    .write_current_page(output)
            })());
        }
        None
    }

    pub(crate) fn same_device(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (
                Backend::Ui {
                    pointer: a,
                    owner: original,
                },
                Backend::Ui {
                    pointer: b,
                    owner: selected,
                },
            ) => original.same_domain(selected) && std::ptr::eq(*a, *b),
            (Backend::Pdf(a), Backend::Pdf(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }

    fn with_target<T>(&self, f: impl FnOnce(&mut dyn DrawTarget) -> T) -> T {
        match &self.0 {
            Backend::Ui { pointer, .. } => {
                // The UI guard owns the borrowed renderer for this evaluation.
                // Check the pointer is still installed before taking a short borrow.
                if !Self::ui().is_some_and(|active| active.same_device(self)) {
                    crate::sexp::context::r_error("graphics device changed during drawing");
                }
                unsafe { f(&mut **pointer) }
            }
            Backend::Pdf(capture) => {
                let mut borrowed = capture
                    .try_borrow_mut()
                    .unwrap_or_else(|_| crate::sexp::context::r_error("PDF device is busy"));
                let capture = borrowed.as_mut().unwrap_or_else(|| {
                    crate::sexp::context::r_error("PDF device closed during drawing")
                });
                f(capture)
            }
        }
    }
}

impl DrawTarget for GraphicsTarget {
    fn dimensions(&self) -> (u32, u32) {
        self.with_target(|target| target.dimensions())
    }
    fn clear(&mut self, color: Color) {
        self.with_target(|target| target.clear(color));
    }
    fn set_clip(&mut self, rect: Option<[f32; 4]>) {
        self.with_target(|target| target.set_clip(rect));
    }
    fn draw_path(&mut self, path: &Path) {
        self.with_target(|target| target.draw_path(path));
    }
    fn draw_text(&mut self, text: &str, position: Point, params: &PlotParameters) {
        self.with_target(|target| target.draw_text(text, position, params));
    }
    fn measure_text(&self, text: &str, params: &PlotParameters) -> TextMetrics {
        self.with_target(|target| target.measure_text(text, params))
    }
    fn measure_math_text(&self, text: &str, params: &PlotParameters) -> TextMetrics {
        self.with_target(|target| target.measure_math_text(text, params))
    }
    fn draw_image(&mut self, image: &RasterImage, transform: [f64; 6], interpolate: bool) {
        self.with_target(|target| target.draw_image(image, transform, interpolate));
    }
}
