use crate::mainutils::{browser_files, platform};
use r_graphics_engine::Scene;
use std::{io, path::Path};

pub(super) enum PdfOutput {
    Browser(String),
    Host(String),
}

impl PdfOutput {
    pub(super) fn new(path: &str) -> io::Result<Self> {
        if browser_files::enabled() {
            return browser_files::absolute_current(path)
                .map(Self::Browser)
                .map_err(io::Error::other);
        }
        let path = Path::new(path);
        let name = path
            .file_name()
            .ok_or_else(|| io::Error::other("invalid PDF destination"))?;
        let parent = path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .canonicalize()?;
        let absolute = parent.join(name);
        absolute
            .into_os_string()
            .into_string()
            .map(Self::Host)
            .map_err(|_| io::Error::other("invalid PDF destination encoding"))
    }

    pub(super) fn publish(&self, pages: &[Scene], compress_pages: bool) -> io::Result<()> {
        match self {
            Self::Browser(path) => browser_files::publish_generated_current(path, |mut output| {
                r_graphics_engine::write_pdf_with_compression(pages, &mut output, compress_pages)
            }),
            Self::Host(path) => platform::publish_host_file(path, |file| {
                r_graphics_engine::write_pdf_with_compression(pages, file, compress_pages)
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexp::session::RSession;
    use r_graphics_engine::{Color, DrawTarget, Path, RasterImage, Stroke};

    #[test]
    fn unsupported_content_preserves_the_previous_browser_file() {
        let mut session = RSession::new_for_gc_tests();
        session
            .put_browser_file("reports/plot.pdf", b"previous result")
            .unwrap();
        session.with_active(|| {
            browser_files::chdir_current("reports").unwrap();
            let output = PdfOutput::new("plot.pdf").unwrap();
            browser_files::chdir_current("/").unwrap();

            let mut page = Scene::new(432, 288);
            let pixel = RasterImage::new(1, 1, vec![0; 4]).unwrap();
            page.draw_image(&pixel, [1., 0., 0., 1., 0., 0.], false);
            assert!(output.publish(&[page], true).is_err());
            assert_eq!(
                browser_files::read_current("reports/plot.pdf").unwrap(),
                b"previous result"
            );

            let mut page = Scene::new(432, 288);
            let mut rectangle = Path::rect(25., 30., 20., 20.);
            rectangle.stroke = Stroke::new(1.0, Color::BLACK);
            page.draw_path(&rectangle);
            output.publish(&[page], true).unwrap();
            let produced = browser_files::read_current("reports/plot.pdf").unwrap();
            assert!(produced.starts_with(b"%PDF-"));
            assert!(produced.ends_with(b"%%EOF\n"));
            assert!(!browser_files::contains_current("plot.pdf"));
        });
    }
}
