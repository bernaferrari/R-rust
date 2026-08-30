//! Plot result surface for the UniFFI boundary.

use super::error::RError;

/// A rendered plot. `png_bytes` always contains a complete, PNG-encoded image.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PlotResult {
    pub width: u32,
    pub height: u32,
    /// PNG-encoded image bytes (starts with the `0x89 'P' 'N' 'G'` signature).
    pub png_bytes: Vec<u8>,
}

/// Reject plot dimensions the interpreter's headless device cannot honor.
pub(crate) fn validate_plot_dimensions(width: u32, height: u32) -> Result<(), RError> {
    if width == 0 || height == 0 {
        return Err(RError::InvalidInput(
            "plot width and height must be greater than zero".to_string(),
        ));
    }
    if width < 32 || height < 32 {
        return Err(RError::InvalidInput(
            "plot width and height must be at least 32 pixels".to_string(),
        ));
    }
    Ok(())
}
