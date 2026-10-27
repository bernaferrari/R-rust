use r_embed::{RSession, RuntimePathPolicy};
use std::io::Cursor;

const TITLE_MULTI_VALUE_SCRIPT: &str = r#"
plot(1:2)
title("foo", col = 1:3)
title("foo", cex = 1:3)
title("foo", lty = 1:3)
title("foo", lwd = 1:3)
title("foo", bg = 4:7)
"graphics-title-multi-value-ok"
"#;

fn check_title_multi_values(session: &mut RSession, policy: &str) {
    session
        .render_with_dimensions(TITLE_MULTI_VALUE_SCRIPT, 320, 240)
        .unwrap_or_else(|error| panic!("title multi-value arguments; policy={policy}; {error}"));
}

fn decode_rgba(png: &[u8]) -> (usize, usize, Vec<u8>) {
    let mut reader = png::Decoder::new(Cursor::new(png))
        .read_info()
        .expect("png reader");
    let mut bytes = vec![0; reader.output_buffer_size().expect("png size")];
    let info = reader.next_frame(&mut bytes).expect("png frame");
    let rgba = match info.color_type {
        png::ColorType::Rgba => bytes[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => bytes[..info.buffer_size()]
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        other => panic!("unexpected png color type: {other:?}"),
    };
    (info.width as usize, info.height as usize, rgba)
}

fn ink_bounds(png: &[u8], y_range: std::ops::Range<usize>) -> (usize, usize, usize, usize) {
    let (width, height, rgba) = decode_rgba(png);
    let mut bounds = (width, height, 0, 0);
    for y in y_range {
        for x in 0..width {
            let px = &rgba[(y * width + x) * 4..][..4];
            if px[3] > 0 && (px[0] < 220 || px[1] < 220 || px[2] < 220) {
                bounds.0 = bounds.0.min(x);
                bounds.1 = bounds.1.min(y);
                bounds.2 = bounds.2.max(x);
                bounds.3 = bounds.3.max(y);
            }
        }
    }
    bounds
}

fn render(session: &mut RSession, code: &str, policy: &str) -> Vec<u8> {
    session
        .render_with_dimensions(code, 320, 240)
        .unwrap_or_else(|error| panic!("title rendering; policy={policy}; {error}"))
}

fn check_title_cex_parity(session: &mut RSession, policy: &str) {
    let base = render(
        session,
        "plot(1:2, main='', sub='', xlab='', ylab=''); title('TITLE')",
        policy,
    );
    let scalar = render(
        session,
        "plot(1:2, main='', sub='', xlab='', ylab=''); title('TITLE', cex=2)",
        policy,
    );
    let vector = render(
        session,
        "plot(1:2, main='', sub='', xlab='', ylab=''); title('TITLE', cex=1:4)",
        policy,
    );
    assert_eq!(base, scalar, "GNU title cex=2 is accepted and ignored; policy={policy}");
    assert_eq!(base, vector, "GNU title vector cex is accepted and ignored; policy={policy}");

    let scaled = render(
        session,
        "plot(1:2, main='', sub='', xlab='', ylab=''); par(cex.main=2); title('TITLE')",
        policy,
    );
    let base_title = ink_bounds(&base, 0..70);
    let scaled_title = ink_bounds(&scaled, 0..70);
    let base_height = base_title.3.saturating_sub(base_title.1);
    let scaled_height = scaled_title.3.saturating_sub(scaled_title.1);
    assert!(
        scaled_height > base_height,
        "par(cex.main=2) must enlarge the title; policy={policy}, base={base_title:?}, scaled={scaled_title:?}"
    );
    let (_, _, base_pixels) = decode_rgba(&base);
    let (_, _, scaled_pixels) = decode_rgba(&scaled);
    // Data points and plot styling live below the title band and must remain
    // identical when only the title-specific cex parameter changes.
    let row_bytes = 320 * 4;
    assert_eq!(
        &base_pixels[70 * row_bytes..],
        &scaled_pixels[70 * row_bytes..],
        "cex.main must not change plotted data or axes; policy={policy}"
    );
}

#[test]
fn native_title_accepts_multi_value_style_arguments() {
    let mut session = RSession::new().expect("native session");
    check_title_multi_values(&mut session, "native");
    check_title_cex_parity(&mut session, "native");
}

#[test]
fn portable_title_accepts_multi_value_style_arguments() {
    let mut session =
        RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
            .expect("portable session");
    check_title_multi_values(&mut session, "portable");
    check_title_cex_parity(&mut session, "portable");
}
