//! Device-owned page capture with an arena charge for its retained geometry.
use crate::mainutils::portable_grid::replay::{Command, CommandLog};
use crate::sexp::{
    instance::with_current_instance,
    memory::{WorkspaceReservation, is_arena_lent},
};
use r_graphics_engine::{
    Color, DrawOperation, DrawTarget, Path, PlotParameters, Point, RasterImage, Scene,
};

const MAX_PAGES: usize = 256;
const CAPTURE_LIMIT: &str = "PDF drawing data exceeds the session memory limit";

pub(super) struct PdfCapture {
    dimensions: (u32, u32),
    pages: Vec<Scene>,
    operations: Vec<DrawOperation>,
    display_list_start: Option<usize>,
    grid_commands: Option<(usize, CommandLog)>,
    revision: u64,
    admission: WorkspaceReservation,
    bytes: usize,
    error: Option<&'static str>,
}

pub(super) struct PdfPages {
    pub(super) pages: Vec<Scene>,
    _admission: WorkspaceReservation,
}

impl PdfCapture {
    pub(super) fn new(dimensions: (u32, u32)) -> Result<Self, &'static str> {
        let mut pages = Vec::new();
        let mut bytes = MAX_PAGES * size_of::<Scene>();
        let mut admission = with_current_instance(|instance| {
            if is_arena_lent(instance) {
                return None;
            }
            // Only the ledger is lent, and admission cannot collect or call R.
            unsafe { (*instance).arena.reserve_workspace(bytes) }
        })
        .flatten()
        .ok_or(CAPTURE_LIMIT)?;
        pages
            .try_reserve_exact(MAX_PAGES)
            .map_err(|_| CAPTURE_LIMIT)?;
        bytes = pages.capacity() * size_of::<Scene>();
        admission.try_resize(bytes).ok_or(CAPTURE_LIMIT)?;
        Ok(Self {
            dimensions,
            pages,
            operations: Vec::new(),
            display_list_start: None,
            grid_commands: None,
            revision: 0,
            admission,
            bytes,
            error: None,
        })
    }

    fn finish_page(&mut self) {
        if self.operations.is_empty() || self.error.is_some() {
            return;
        }
        if self.pages.len() >= MAX_PAGES {
            self.error = Some("too many pages for the PDF device");
            return;
        }
        self.pages.push(Scene::from_operations(
            self.dimensions,
            std::mem::take(&mut self.operations),
        ));
    }

    pub(super) fn write_current_page(
        &self,
        output: &mut dyn std::io::Write,
    ) -> std::io::Result<bool> {
        if let Some(error) = self.error {
            return Err(std::io::Error::other(error));
        }
        let operations = self
            .display_list_start
            .and_then(|start| self.operations.get(start..))
            .unwrap_or_default();
        Scene::write_operations(self.dimensions, operations, output)
            .map_err(std::io::Error::other)?;
        Ok(!operations.is_empty())
    }

    pub(super) fn reset_display_list(&mut self, enabled: bool) {
        self.display_list_start = enabled.then_some(self.operations.len());
        self.grid_commands = self
            .display_list_start
            .map(|start| (start, CommandLog::default()));
        self.revision = self.revision.wrapping_add(1);
    }

    pub(super) fn start_grid_command(&mut self, command: Command, at_root: bool) -> Option<usize> {
        self.display_list_start?;
        if !command.starts_page() {
            let (covered, log) = self.grid_commands.as_ref()?;
            if *covered != self.operations.len() || (log.head().is_none() && !at_root) {
                self.grid_commands = None;
                return None;
            }
        }
        Some(if command.starts_page() {
            0
        } else {
            self.operations.len()
        })
    }

    pub(super) fn grid_prefix(&self, start: usize) -> Option<(u64, CommandLog)> {
        let (covered, log) = self.grid_commands.as_ref()?;
        (*covered == start).then(|| (self.revision, log.clone()))
    }

    pub(super) fn finish_grid_command(&mut self, revision: u64, log: Option<CommandLog>) {
        if self.revision == revision {
            self.grid_commands = log.map(|log| (self.operations.len(), log));
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub(super) fn recorded_grid(&self) -> Option<&crate::sexp::object::Sexp<'static>> {
        let (covered, log) = self.grid_commands.as_ref()?;
        if *covered == self.operations.len() {
            log.head()
        } else {
            None
        }
    }

    pub(super) fn install_grid_replay(
        &mut self,
        mut staged: Self,
        log: CommandLog,
    ) -> Result<(), &'static str> {
        if let Some(error) = self.error.or(staged.error) {
            return Err(error);
        }
        if self.pages.len()
            + usize::from(!self.operations.is_empty())
            + staged.pages.len()
            + usize::from(!staged.operations.is_empty())
            > MAX_PAGES
        {
            return Err("too many pages for the PDF device");
        }
        let bytes = self
            .bytes
            .checked_add(staged.bytes - staged.pages.capacity() * size_of::<Scene>())
            .ok_or(CAPTURE_LIMIT)?;
        self.admission.try_resize(bytes).ok_or(CAPTURE_LIMIT)?;
        self.begin_replay_page();
        self.pages.append(&mut staged.pages);
        self.operations = staged.operations;
        self.bytes = bytes;
        self.grid_commands = self
            .display_list_start
            .map(|_| (self.operations.len(), log));
        Ok(())
    }

    pub(super) fn finish_grid_retry_prefix(&mut self) {
        if self.operations.is_empty() {
            self.pages
                .push(Scene::new(self.dimensions.0, self.dimensions.1));
        } else {
            self.finish_page();
        }
    }

    pub(super) fn begin_replay_page(&mut self) {
        self.finish_page();
        self.reset_display_list(self.display_list_start.is_some());
    }

    pub(super) fn reject_raster(&mut self) {
        self.error
            .get_or_insert("PDF raster images are not supported");
    }

    pub(super) fn finish(mut self) -> Result<PdfPages, &'static str> {
        self.finish_page();
        if let Some(error) = self.error {
            return Err(error);
        }
        Ok(PdfPages {
            pages: self.pages,
            _admission: self.admission,
        })
    }

    fn record(
        &mut self,
        payload: usize,
        make: impl FnOnce() -> Result<DrawOperation, &'static str>,
    ) {
        if self.error.is_some() {
            return;
        }
        if let Err(error) = self.record_on_page(payload, make) {
            // Device methods can run from C callbacks. Defer the diagnostic to
            // close, which can report it without publishing a partial file.
            self.error = Some(error);
        }
        self.revision = self.revision.wrapping_add(1);
    }

    fn record_on_page(
        &mut self,
        payload: usize,
        make: impl FnOnce() -> Result<DrawOperation, &'static str>,
    ) -> Result<(), &'static str> {
        let old_capacity = self.operations.capacity();
        let old_bytes = old_capacity * size_of::<DrawOperation>();
        if self.operations.len() == old_capacity {
            let new_capacity = old_capacity.max(2).checked_mul(2).ok_or(CAPTURE_LIMIT)?;
            let growth = new_capacity
                .checked_mul(size_of::<DrawOperation>())
                .ok_or(CAPTURE_LIMIT)?;
            self.admission
                .try_resize(self.bytes.checked_add(growth).ok_or(CAPTURE_LIMIT)?)
                .ok_or(CAPTURE_LIMIT)?;
            self.operations
                .try_reserve_exact(new_capacity - old_capacity)
                .map_err(|_| CAPTURE_LIMIT)?;
        }
        let used = self.bytes - old_bytes;
        let bytes = self
            .operations
            .capacity()
            .checked_mul(size_of::<DrawOperation>())
            .and_then(|current| used.checked_add(current))
            .and_then(|current| current.checked_add(payload))
            .ok_or(CAPTURE_LIMIT)?;
        self.admission.try_resize(bytes).ok_or(CAPTURE_LIMIT)?;
        self.bytes = bytes;
        self.operations.push(make()?);
        Ok(())
    }
}

impl DrawTarget for PdfCapture {
    fn dimensions(&self) -> (u32, u32) {
        self.dimensions
    }

    fn clear(&mut self, color: Color) {
        if self
            .operations
            .iter()
            .any(|op| !matches!(op, DrawOperation::Clip(_)))
        {
            self.finish_page();
        } else {
            self.operations.clear();
        }
        self.reset_display_list(self.display_list_start.is_some());
        self.record(0, || Ok(DrawOperation::Clear(color)));
    }

    fn set_clip(&mut self, rect: Option<[f32; 4]>) {
        self.record(0, || Ok(DrawOperation::Clip(rect)));
    }

    fn draw_path(&mut self, path: &Path) {
        let size = size_of_val(path.commands.as_slice())
            + path
                .stroke
                .dash_pattern
                .as_ref()
                .map_or(0, |dash| size_of_val(dash.intervals.as_slice()));
        self.record(size, || {
            let commands = copy_slice(&path.commands)?;
            let dash_pattern = path
                .stroke
                .dash_pattern
                .as_ref()
                .map(|dash| {
                    Ok::<_, &'static str>(r_graphics_engine::DashPattern {
                        intervals: copy_slice(&dash.intervals)?,
                        offset: dash.offset,
                    })
                })
                .transpose()?;
            Ok(DrawOperation::Path(Path {
                commands,
                fill: path.fill,
                anti_alias: path.anti_alias,
                stroke: r_graphics_engine::Stroke {
                    color: path.stroke.color,
                    width: path.stroke.width,
                    cap: path.stroke.cap,
                    join: path.stroke.join,
                    miter_limit: path.stroke.miter_limit,
                    dash_pattern,
                },
            }))
        });
    }

    fn draw_text(&mut self, text: &str, position: Point, params: &PlotParameters) {
        self.record(text.len(), || {
            let mut buffer = String::new();
            buffer
                .try_reserve_exact(text.len())
                .map_err(|_| CAPTURE_LIMIT)?;
            buffer.push_str(text);
            Ok(DrawOperation::Text {
                text: buffer,
                position,
                params: params.clone(),
            })
        });
    }

    fn draw_image(&mut self, _: &RasterImage, _: [f64; 6], _: bool) {
        self.error
            .get_or_insert("PDF raster images are not supported");
    }
}

fn copy_slice<T: Clone>(source: &[T]) -> Result<Vec<T>, &'static str> {
    let mut data = Vec::new();
    data.try_reserve_exact(source.len())
        .map_err(|_| CAPTURE_LIMIT)?;
    data.extend_from_slice(source);
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::session::RSession;

    #[test]
    fn pdf_recording_starts_at_enable_and_resets_on_a_new_page() {
        let mut session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let mut capture = PdfCapture::new((432, 288)).unwrap();
            capture.draw_path(&Path::rect(1., 2., 3., 4.));
            let read = |capture: &PdfCapture| {
                let mut output = Vec::new();
                let present = capture.write_current_page(&mut output).unwrap();
                let scene = Scene::decode(&output).unwrap();
                assert_eq!(present, !scene.operations().is_empty());
                scene
            };
            assert!(read(&capture).operations().is_empty());
            capture.reset_display_list(true);
            capture.draw_path(&Path::rect(5., 6., 7., 8.));
            assert_eq!(read(&capture).operations().len(), 1);
            capture.begin_replay_page();
            assert!(read(&capture).operations().is_empty());
            capture.draw_path(&Path::rect(9., 10., 11., 12.));
            assert_eq!(read(&capture).operations().len(), 1);
            capture.reset_display_list(false);
            assert!(read(&capture).operations().is_empty());
            let pdf = capture.finish().unwrap();
            assert_eq!(pdf.pages.len(), 2);
            assert_eq!(pdf.pages[0].operations().len(), 2);
            assert_eq!(pdf.pages[1].operations().len(), 1);
        });
    }

    #[test]
    fn a_new_page_retains_prior_geometry_until_finish() {
        let mut session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let mut capture = PdfCapture::new((432, 288)).unwrap();
            capture.clear(Color::WHITE);
            capture.set_clip(Some([0.0, 0.0, 432.0, 288.0]));
            capture.draw_path(&Path::rect(25.0, 30.0, 20.0, 20.0));
            capture.clear(Color::WHITE);
            capture.draw_path(&Path::rect(25.0, 50.0, 20.0, 20.0));
            let pdf = capture.finish().unwrap();
            assert_eq!(pdf.pages.len(), 2);
            assert_eq!(pdf.pages[0].operations().len(), 3);
            assert_eq!(pdf.pages[1].operations().len(), 2);
            assert_eq!(pdf.pages[0].dimensions(), (432, 288));
        });
    }

    #[test]
    fn denied_geometry_keeps_the_output_unpublishable() {
        use crate::sexp::memory::ArenaBudget;
        let mut session = RSession::new_for_gc_tests();
        let mut capture = session.with_active(|| PdfCapture::new((432, 288))).unwrap();
        let baseline = session
            .with_arena(|arena| arena.total_bytes_allocated() + arena.workspace_bytes())
            .unwrap();
        session.set_arena_budget(ArenaBudget::new(baseline, 0));
        session.with_active(|| {
            capture.draw_path(&Path::rect(25.0, 30.0, 20.0, 20.0));
            assert!(matches!(capture.finish(), Err(CAPTURE_LIMIT)));
        });
        assert!(session.with_arena(|arena| arena.workspace_bytes()).unwrap() < baseline);
    }

    #[test]
    fn unsupported_raster_marks_the_capture_as_failed() {
        let mut session = RSession::new_for_gc_tests();
        session.with_active(|| {
            let mut capture = PdfCapture::new((432, 288)).unwrap();
            let pixel = RasterImage::new(1, 1, vec![0; 4]).unwrap();
            capture.draw_image(&pixel, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], false);
            assert!(matches!(
                capture.finish(),
                Err("PDF raster images are not supported")
            ));
        });
    }
}
