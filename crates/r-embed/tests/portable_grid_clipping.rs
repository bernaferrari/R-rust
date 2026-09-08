use r_embed::RSession;
use std::io::Cursor;

fn pixels(bytes: &[u8]) -> (usize, Vec<u8>) {
    let mut reader = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    (info.width as usize, pixels[..info.buffer_size()].to_vec())
}

fn at(width: usize, pixels: &[u8], x: usize, y: usize) -> &[u8] {
    &pixels[(y * width + x) * 4..(y * width + x + 1) * 4]
}

#[test]
fn rotated_viewport_clip_warns_and_keeps_parent_clip() {
    let mut session = RSession::new().unwrap();
    let png = session
        .render_with_dimensions(
            "library(grid); grid.newpage(); pushViewport(viewport(x=.5,y=.5,width=.5,height=.5,clip='on',angle=45)); grid.rect(width=2,height=2,gp=gpar(fill='red',col=NA)); popViewport()",
            200,
            200,
        )
        .unwrap();
    let (width, pixels) = pixels(&png);
    // GNU R warns and falls back to the parent clip for rotated viewports;
    // the rectangle itself remains transformed by the viewport.
    assert_eq!(at(width, &pixels, 100, 100), [255, 0, 0, 255]);
    assert_eq!(at(width, &pixels, 2, 2), [255, 255, 255, 255]);
}

#[test]
fn nested_clip_is_restored_after_rotated_child() {
    let mut session = RSession::new().unwrap();
    let png = session
        .render_with_dimensions(
            "library(grid); grid.newpage(); pushViewport(viewport(x=.5,y=.5,width=.5,height=.5,clip='on')); pushViewport(viewport(width=.8,height=.8,angle=45,clip='on')); grid.rect(width=2,height=2,gp=gpar(fill='red',col=NA)); popViewport(); grid.rect(width=2,height=2,gp=gpar(fill='blue',col=NA)); popViewport()",
            200,
            200,
        )
        .unwrap();
    let (width, pixels) = pixels(&png);
    // The rotated child inherits the parent's rectangle, and popping it
    // restores that same rectangle for the following draw.
    assert_eq!(at(width, &pixels, 10, 10), [255, 255, 255, 255]);
    assert_eq!(at(width, &pixels, 100, 100), [0, 0, 255, 255]);
    assert_eq!(at(width, &pixels, 190, 190), [255, 255, 255, 255]);
}
