//! End-to-end recordPlot/replayPlot coverage for the portable renderer.

use r_embed::RSession;
use std::io::Cursor;

struct DecodedPng {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl DecodedPng {
    fn pixels_matching(&self, matches: impl Fn(&[u8]) -> bool) -> usize {
        self.rgba
            .chunks_exact(4)
            .filter(|pixel| matches(pixel) && pixel[3] > 0)
            .count()
    }
}

fn decode_png(png_bytes: &[u8]) -> DecodedPng {
    let decoder = png::Decoder::new(Cursor::new(png_bytes));
    let mut reader = decoder.read_info().expect("png reader");
    let mut buffer = vec![0; reader.output_buffer_size().expect("png output size")];
    let info = reader.next_frame(&mut buffer).expect("png frame");
    let bytes = &buffer[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => bytes.to_vec(),
        png::ColorType::Rgb => bytes
            .chunks_exact(3)
            .flat_map(|rgb| [rgb[0], rgb[1], rgb[2], 255])
            .collect(),
        other => panic!("unexpected png color type: {other:?}"),
    };
    DecodedPng {
        width: info.width,
        height: info.height,
        rgba,
    }
}

#[test]
fn record_plot_replays_owned_commands_to_the_active_device() {
    let mut session = RSession::new().expect("session");
    let png = session
        .render_with_dimensions(
            "plot(c(1,2,3), c(1,4,9), type='l', col='red'); p <- recordPlot(); plot.new(); replayPlot(p)",
            320,
            240,
        )
        .expect("recorded plot should replay");
    assert!(png.starts_with(&[0x89, 0x50, 0x4e, 0x47]));
    assert!(png.len() > 256, "replayed plot should contain drawing data");
}

#[test]
fn serialized_recording_survives_unserialize_before_replay() {
    let mut session = RSession::new().expect("session");
    let png = session
        .render_with_dimensions(
            "plot(c(1,2,3), c(3,1,2), pch=21, bg='gold'); saved <- serialize(recordPlot(), NULL); plot.new(); replayPlot(unserialize(saved))",
            320,
            240,
        )
        .expect("serialized recording should replay");
    assert!(png.len() > 256);
}

#[test]
fn replay_of_a_recording_scales_to_a_resized_device() {
    let mut session = RSession::new().expect("session");
    session
        .render_with_dimensions(
            "plot(c(1,2,3), c(1,4,9), type='l', col='blue'); saved <- serialize(recordPlot(), NULL)",
            640,
            480,
        )
        .expect("source recording");
    let png = session
        .render_with_dimensions(
            "plot.new(); replayPlot(unserialize(saved)); points(2, 4, pch=19, col='red')",
            160,
            120,
        )
        .expect("resized replay");
    let decoded = decode_png(&png);
    assert_eq!((decoded.width, decoded.height), (160, 120));
    assert!(
        decoded.pixels_matching(|pixel| pixel[0] > 180 && pixel[1] < 120 && pixel[2] < 120) > 5
    );
}

#[test]
fn malformed_recordings_fail_without_poisoning_the_session() {
    let mut session = RSession::new().expect("session");
    let error = session
        .render_with_dimensions(
            "replayPlot(structure(raw(0), class='recordedplot'))",
            320,
            240,
        )
        .expect_err("empty recording should be rejected");
    assert!(error.to_string().contains("invalid recorded plot"));

    let png = session
        .render_with_dimensions("plot(1:3, c(1,4,9), col='red')", 320, 240)
        .expect("session should recover after malformed replay");
    assert!(png.len() > 256);
}

#[test]
fn malformed_portable_state_metadata_is_rejected() {
    let mut session = RSession::new().expect("session");
    let error = session
        .render_with_dimensions(
            "plot(1:3, c(1,4,9)); p <- recordPlot(); attr(p, 'rport.graphics.state') <- rep(0, 18); plot.new(); replayPlot(p)",
            320,
            240,
        )
        .expect_err("invalid portable state should be rejected");
    assert!(error.to_string().contains("graphics state metadata"));
}

#[test]
fn record_serialization_survives_gc_torture() {
    let mut session = RSession::new().expect("session");
    let png = session
        .render_with_dimensions(
            "gctorture(TRUE); plot(1:3, c(1,4,9), col='blue'); saved <- serialize(recordPlot(), NULL); gctorture(FALSE); plot.new(); replayPlot(unserialize(saved))",
            320,
            240,
        )
        .expect("record serialization under gc torture");
    assert!(png.len() > 256);
}
