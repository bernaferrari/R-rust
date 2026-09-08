use r_embed::RSession;
use std::io::Cursor;

fn ink(bytes: &[u8]) -> usize {
    let mut reader = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    pixels[..info.buffer_size()]
        .chunks_exact(4)
        .filter(|p| p[0] < 128 || p[1] < 128 || p[2] < 128)
        .count()
}

#[test]
fn text_check_overlap_suppresses_intersecting_labels() {
    let mut session = RSession::new().unwrap();
    let overlap = session
        .render_with_dimensions(
            "library(grid); grid.newpage(); grid.text(c('AAAA','BBBB'), x=unit(c(.5,.5),'npc'), y=unit(c(.5,.5),'npc'), check.overlap=TRUE, gp=gpar(fontsize=24))",
            200,
            100,
        )
        .unwrap();
    let separate = session
        .render_with_dimensions(
            "library(grid); grid.newpage(); grid.text(c('AAAA','BBBB'), x=unit(c(.35,.65),'npc'), y=unit(c(.5,.5),'npc'), check.overlap=TRUE, gp=gpar(fontsize=24))",
            200,
            100,
        )
        .unwrap();
    assert!(ink(&overlap) > 0);
    assert!(ink(&separate) > ink(&overlap));
}

#[test]
fn text_check_overlap_uses_rotated_label_rectangles() {
    let mut session = RSession::new().unwrap();
    let overlap = session
        .render_with_dimensions(
            "library(grid); grid.newpage(); grid.text(c('AAAA','BBBB'), x=unit(c(.5,.5),'npc'), y=unit(c(.5,.5),'npc'), rot=45, check.overlap=TRUE, gp=gpar(fontsize=24))",
            200,
            100,
        )
        .unwrap();
    let separate = session
        .render_with_dimensions(
            "library(grid); grid.newpage(); grid.text(c('AAAA','BBBB'), x=unit(c(.35,.65),'npc'), y=unit(c(.5,.5),'npc'), rot=45, check.overlap=TRUE, gp=gpar(fontsize=24))",
            200,
            100,
        )
        .unwrap();
    assert!(ink(&separate) > ink(&overlap));
}

#[test]
fn filled_point_symbols_use_col_for_15_through_20_and_fill_for_21_through_25() {
    let mut session = RSession::new().unwrap();
    let png = session
        .render_with_dimensions(
            "library(grid); grid.newpage(); pushViewport(viewport(xscale=c(0,6))); grid.points(1:6,rep(.5,6),pch=c(15,16,17,18,19,20),size=unit(16,'points'),gp=gpar(col='black',fill='red')); popViewport()",
            360,
            100,
        )
        .unwrap();
    assert!(ink(&png) > 200);
}
