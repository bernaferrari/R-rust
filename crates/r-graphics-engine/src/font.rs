//! Shared font bytes and layout metrics, independent of a rendering backend.
//!
//! Bold uses an additional outline stroke of 3% of the font size; italic uses
//! a 12 degree shear. Both backends apply these synthetic faces to the same
//! outlines, including custom fonts, so layout does not depend on installed
//! bold/italic variants. Complex-script shaping is not provided here.
use crate::{FontFace, TextMetrics};
use std::sync::{Arc, OnceLock};

#[derive(Clone)]
pub struct FontBook {
    bytes: Arc<Vec<u8>>,
    metrics: Arc<fontdue::Font>,
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
        Ok(Self {
            bytes: Arc::new(bytes),
            metrics: Arc::new(metrics),
        })
    }
    pub fn bytes(&self) -> Arc<Vec<u8>> {
        self.bytes.clone()
    }
    pub fn glyph_index(&self, ch: char) -> u16 {
        self.metrics.lookup_glyph_index(ch)
    }
    pub fn advance_width(&self, ch: char, size: f32) -> f32 {
        self.metrics.metrics(ch, size).advance_width
    }
    pub fn measure_text(&self, text: &str, size: f32, _face: FontFace) -> TextMetrics {
        let size = normalized_size(size);
        let line = self.metrics.horizontal_line_metrics(size);
        TextMetrics {
            width: text
                .chars()
                .filter(|c| !c.is_control())
                .map(|c| self.advance_width(c, size))
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

/// Cached system font with a bundled, licensed fallback for Wasm and mobile.
pub fn default_font_book() -> &'static FontBook {
    static FONT: OnceLock<FontBook> = OnceLock::new();
    FONT.get_or_init(|| {
        for path in [
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            "/System/Library/Fonts/Geneva.ttf",
            "/System/Library/Fonts/SFNSDisplay.ttf",
            "/Library/Fonts/Arial.ttf",
            "/system/fonts/NotoSans-Regular.ttf",
            "/system/fonts/DroidSans.ttf",
            "C:\\Windows\\Fonts\\arial.ttf",
        ] {
            if let Ok(bytes) = std::fs::read(path)
                && let Ok(font) = FontBook::from_bytes(bytes)
            {
                return font;
            }
        }
        FontBook::from_bytes(include_bytes!("../assets/NotoSans.ttf").to_vec())
            .expect("bundled Noto Sans is a valid font")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn invalid_custom_font_is_rejected() {
        assert!(FontBook::from_bytes(vec![1, 2, 3]).is_err());
    }
}
