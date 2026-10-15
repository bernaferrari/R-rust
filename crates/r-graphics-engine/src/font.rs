//! Shared font bytes and layout metrics, independent of a rendering backend.
//!
//! The bundled DejaVu family supplies real plain, bold, oblique, and
//! bold-oblique faces. Custom one-face books use that face for every logical
//! style. Complex-script shaping is not provided here.
use crate::{FontFace, TextMetrics};
use std::io::{self, Read};
use std::sync::{Arc, OnceLock};

/// Ink bounds for one glyph in baseline coordinates.  Advances and line
/// metrics are deliberately kept separate: plotmath uses the ink box when
/// placing accents and scripts, while rows continue to advance by the font's
/// horizontal metrics.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GlyphInkMetrics {
    pub x_min: f32,
    pub y_min: f32,
    pub width: f32,
    pub height: f32,
    pub advance_width: f32,
}

#[derive(Clone)]
pub struct FontBook {
    bytes: Arc<Vec<u8>>,
    metrics: Arc<fontdue::Font>,
    face_bytes: [Arc<Vec<u8>>; 4],
    face_metrics: [Arc<fontdue::Font>; 4],
}
impl std::fmt::Debug for FontBook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontBook").finish_non_exhaustive()
    }
}
impl FontBook {
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, String> {
        let metrics = fontdue::Font::from_bytes(bytes.clone(), fontdue::FontSettings::default())
            .map_err(|error| error.to_string())?;
        let bytes = Arc::new(bytes);
        let metrics = Arc::new(metrics);
        Ok(Self {
            bytes: bytes.clone(),
            metrics: metrics.clone(),
            face_bytes: std::array::from_fn(|_| bytes.clone()),
            face_metrics: std::array::from_fn(|_| metrics.clone()),
        })
    }
    pub fn bytes(&self) -> Arc<Vec<u8>> {
        self.bytes.clone()
    }
    pub fn glyph_index(&self, ch: char) -> u16 {
        self.metrics.lookup_glyph_index(ch)
    }
    fn face_index(face: FontFace) -> usize {
        match face {
            FontFace::Plain => 0,
            FontFace::Bold => 1,
            FontFace::Italic => 2,
            FontFace::BoldItalic => 3,
        }
    }
    pub fn bytes_for_face(&self, face: FontFace) -> Arc<Vec<u8>> {
        self.face_bytes[Self::face_index(face)].clone()
    }
    pub fn glyph_index_for_face(&self, ch: char, face: FontFace) -> u16 {
        self.face_metrics[Self::face_index(face)].lookup_glyph_index(ch)
    }
    pub fn advance_width_for_face(&self, ch: char, size: f32, face: FontFace) -> f32 {
        self.face_metrics[Self::face_index(face)]
            .metrics(ch, normalized_size(size))
            .advance_width
    }
    pub fn advance_width(&self, ch: char, size: f32) -> f32 {
        self.metrics.metrics(ch, size).advance_width
    }
    pub fn glyph_ink_metrics(&self, ch: char, size: f32) -> GlyphInkMetrics {
        let m = self.metrics.metrics(ch, normalized_size(size));
        GlyphInkMetrics {
            x_min: m.bounds.xmin,
            y_min: m.bounds.ymin,
            width: m.bounds.width,
            height: m.bounds.height,
            advance_width: m.advance_width,
        }
    }
    /// Advance width and visible vertical bounds, for mathematical composition.
    pub fn measure_math_text(&self, text: &str, size: f32, face: FontFace) -> TextMetrics {
        let size = normalized_size(size);
        let mut out = TextMetrics::default();
        let mut has_ink = false;
        let font = &self.face_metrics[Self::face_index(face)];
        for ch in text.chars().filter(|c| !c.is_control()) {
            let m = font.metrics(ch, size);
            let ink = GlyphInkMetrics {
                x_min: m.bounds.xmin,
                y_min: m.bounds.ymin,
                width: m.bounds.width,
                height: m.bounds.height,
                advance_width: m.advance_width,
            };
            out.width += ink.advance_width;
            if ink.height > 0. {
                if has_ink {
                    out.ascent = out.ascent.max(ink.y_min + ink.height);
                    out.descent = out.descent.max(-ink.y_min);
                } else {
                    // Glyph metrics may have negative descent (e.g. an accent
                    // entirely above the baseline). GNU R uses that signed
                    // depth to position accents without adding an extra gap.
                    out.ascent = ink.y_min + ink.height;
                    out.descent = -ink.y_min;
                    has_ink = true;
                }
            }
        }
        out
    }
    pub fn measure_text(&self, text: &str, size: f32, _face: FontFace) -> TextMetrics {
        let size = normalized_size(size);
        let line = self.face_metrics[Self::face_index(_face)].horizontal_line_metrics(size);
        TextMetrics {
            width: text
                .chars()
                .filter(|c| !c.is_control())
                .map(|c| self.advance_width_for_face(c, size, _face))
                .sum(),
            ascent: line.map_or(size * 0.8, |m| m.ascent),
            descent: line.map_or(size * 0.2, |m| -m.descent),
        }
    }
}
impl Default for FontBook {
    fn default() -> Self {
        default_font_book().clone()
    }
}

pub fn normalized_size(size: f32) -> f32 {
    if size.is_finite() && size > 0.0 {
        size
    } else {
        12.0
    }
}

/// Stream each original face without retaining a second uncompressed archive.
/// The dictionary and total output are bounded independently of stream headers.
fn decompress_bundled_fonts(compressed: &[u8], lengths: [usize; 4]) -> io::Result<[Vec<u8>; 4]> {
    let total = lengths
        .into_iter()
        .try_fold(0usize, usize::checked_add)
        .filter(|total| *total <= 2_741_468)
        .ok_or_else(|| io::Error::other("bundled font length overflow"))?;
    let mut input =
        lzma_rust2::XzReader::new_mem_limit(compressed, false, 2048).take(total as u64 + 1);
    let mut faces: [Vec<u8>; 4] = std::array::from_fn(|_| Vec::new());
    for (face, length) in faces.iter_mut().zip(lengths) {
        face.try_reserve_exact(length).map_err(io::Error::other)?;
        face.resize(length, 0);
        input.read_exact(face)?;
    }
    if input.read(&mut [0])? != 0 {
        return Err(io::Error::other("bundled font length mismatch"));
    }
    Ok(faces)
}

/// Deterministic bundled font shared by every platform and renderer.
/// DejaVu Sans covers the mathematical operators used by plotmath.
pub fn default_font_book() -> &'static FontBook {
    static FONT: OnceLock<FontBook> = OnceLock::new();
    FONT.get_or_init(|| {
        let bytes = decompress_bundled_fonts(
            include_bytes!("../assets/DejaVuSans.faces.xz"),
            [757_076, 705_684, 635_416, 643_292],
        )
        .expect("bundled archive preserves every original DejaVu font byte");
        let parse = |bytes: &Vec<u8>| {
            Arc::new(
                fontdue::Font::from_bytes(bytes.clone(), fontdue::FontSettings::default())
                    .expect("bundled DejaVu Sans is a valid font"),
            )
        };
        FontBook {
            bytes: Arc::new(bytes[0].clone()),
            metrics: parse(&bytes[0]),
            face_bytes: std::array::from_fn(|i| Arc::new(bytes[i].clone())),
            face_metrics: std::array::from_fn(|i| parse(&bytes[i])),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_fonts_preserve_original_ttf_bytes_for_all_faces() {
        let book = default_font_book();
        for (face, original) in [
            (
                FontFace::Plain,
                include_bytes!("../assets/DejaVuSans.ttf").as_slice(),
            ),
            (
                FontFace::Bold,
                include_bytes!("../assets/DejaVuSans-Bold.ttf").as_slice(),
            ),
            (
                FontFace::Italic,
                include_bytes!("../assets/DejaVuSans-Oblique.ttf").as_slice(),
            ),
            (
                FontFace::BoldItalic,
                include_bytes!("../assets/DejaVuSans-BoldOblique.ttf").as_slice(),
            ),
        ] {
            let bytes = book.bytes_for_face(face);
            assert_eq!(bytes.as_slice(), original, "{face:?}");
            assert!(Arc::ptr_eq(
                &bytes,
                &default_font_book().bytes_for_face(face)
            ));
        }
    }

    #[test]
    fn bundled_font_decode_rejects_wrong_lengths_and_truncated_archive() {
        let compressed = include_bytes!("../assets/DejaVuSans.faces.xz");
        assert!(
            decompress_bundled_fonts(compressed, [757_075, 705_684, 635_416, 643_292]).is_err()
        );
        assert!(
            decompress_bundled_fonts(compressed, [757_077, 705_684, 635_416, 643_292]).is_err()
        );
        assert!(
            decompress_bundled_fonts(
                &compressed[..compressed.len() - 1],
                [757_076, 705_684, 635_416, 643_292]
            )
            .is_err()
        );
        assert!(decompress_bundled_fonts(compressed, [usize::MAX, 0, 0, 0]).is_err());
    }

    #[test]
    fn metrics_use_actual_advances_and_baseline() {
        let font = FontBook::default();
        let narrow = font.measure_text("iii", 20., FontFace::Plain);
        let wide = font.measure_text("WWW", 20., FontFace::Plain);
        assert!(wide.width > narrow.width * 2.);
        assert!(wide.ascent > 0. && wide.descent > 0.);
        assert_eq!(font.measure_text("", 20., FontFace::Plain).width, 0.);
        assert_eq!(
            font.measure_text("W", 40., FontFace::Plain).width,
            font.measure_text("W", 20., FontFace::Plain).width * 2.
        );
    }
    #[test]
    fn older_scene_text_parameters_default_to_plain() {
        let params = crate::PlotParameters::default();
        let mut value = serde_json::to_value(params).unwrap();
        value.as_object_mut().unwrap().remove("font_face");
        let decoded: crate::PlotParameters = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.font_face, FontFace::Plain);
    }

    #[test]
    fn bundled_font_covers_plotmath_operators_and_greek() {
        let font = default_font_book();
        for ch in "αβγδεζηθικλμνξοπρστυφχψω∑∏∫∂∇∞≠≤≥±×÷∈≈".chars()
        {
            assert_ne!(font.glyph_index(ch), 0, "missing glyph: {ch}");
        }
    }

    #[test]
    fn invalid_custom_font_is_rejected() {
        assert!(FontBook::from_bytes(vec![1, 2, 3]).is_err());
    }

    #[test]
    fn glyph_ink_box_is_available_alongside_advance() {
        let font = FontBook::default();
        let glyph = font.glyph_ink_metrics('A', 20.);
        assert!(glyph.width > 0. && glyph.height > 0.);
        assert!(glyph.advance_width >= glyph.width);
        assert_eq!(font.glyph_ink_metrics(' ', 20.).width, 0.);
    }
}
