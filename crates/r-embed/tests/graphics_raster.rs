//! Portable `rasterImage` coverage through the public embedding boundary.

use r_embed::RSession;
use std::io::Cursor;

struct DecodedPng {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl DecodedPng {
    fn matching(&self, predicate: impl Fn([u8; 4]) -> bool) -> usize {
        self.rgba
            .chunks_exact(4)
            .map(|pixel| pixel.try_into().unwrap())
            .filter(|pixel| predicate(*pixel))
            .count()
    }

    fn bounds(&self, predicate: impl Fn([u8; 4]) -> bool) -> Option<(u32, u32, u32, u32)> {
        let mut points = self
            .rgba
            .chunks_exact(4)
            .enumerate()
            .filter_map(|(index, pixel)| {
                let pixel = pixel.try_into().unwrap();
                predicate(pixel).then_some((index as u32 % self.width, index as u32 / self.width))
            });
        let first = points.next()?;
        let mut bounds = (first.0, first.1, first.0, first.1);
        for (x, y) in points {
            bounds.0 = bounds.0.min(x);
            bounds.1 = bounds.1.min(y);
            bounds.2 = bounds.2.max(x);
            bounds.3 = bounds.3.max(y);
        }
        Some(bounds)
    }
}

fn image_bounds(image: &DecodedPng, predicates: &[fn([u8; 4]) -> bool]) -> (u32, u32, u32, u32) {
    predicates
        .iter()
        .map(|predicate| image.bounds(*predicate).expect("color is rendered"))
        .fold((u32::MAX, u32::MAX, 0, 0), |bounds, color| {
            (
                bounds.0.min(color.0),
                bounds.1.min(color.1),
                bounds.2.max(color.2),
                bounds.3.max(color.3),
            )
        })
}

fn assert_quadrant(
    image: &DecodedPng,
    predicate: fn([u8; 4]) -> bool,
    split_x: f64,
    split_y: f64,
    left: bool,
    top: bool,
) {
    let mut count = 0;
    let mut sum_x = 0u64;
    let mut sum_y = 0u64;
    for (index, pixel) in image.rgba.chunks_exact(4).enumerate() {
        if predicate(pixel.try_into().unwrap()) {
            let x = index as u32 % image.width;
            let y = index as u32 / image.width;
            count += 1;
            sum_x += u64::from(x);
            sum_y += u64::from(y);
        }
    }
    assert!(
        count > 100,
        "expected a substantial rendered color, got {count}"
    );
    let x = sum_x as f64 / count as f64;
    let y = sum_y as f64 / count as f64;
    assert_eq!(x < split_x, left, "unexpected horizontal color quadrant");
    assert_eq!(y < split_y, top, "unexpected vertical color quadrant");
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

fn plot_setup() -> &'static str {
    "par(mar=c(0,0,0,0), xaxs='i', yaxs='i'); plot.new(); plot.window(xlim=c(0,1), ylim=c(0,1))"
}

fn is_red(pixel: [u8; 4]) -> bool {
    pixel[0] > 200 && pixel[1] < 80 && pixel[2] < 80 && pixel[3] > 200
}

fn is_green(pixel: [u8; 4]) -> bool {
    pixel[1] > 140 && pixel[0] < 100 && pixel[2] < 100 && pixel[3] > 200
}

fn is_blue(pixel: [u8; 4]) -> bool {
    pixel[2] > 140 && pixel[0] < 100 && pixel[1] < 100 && pixel[3] > 200
}

fn is_black(pixel: [u8; 4]) -> bool {
    pixel[0] < 60 && pixel[1] < 60 && pixel[2] < 60 && pixel[3] > 200
}

#[test]
fn raster_image_color_matrix_renders_in_expected_quadrants() {
    let mut session = RSession::new().expect("session");
    let code = format!(
        "{}; rasterImage(matrix(c('red','green','blue','black'), nrow=2), 0, 0, 1, 1, interpolate=FALSE)",
        plot_setup()
    );
    let decoded = decode_png(
        &session
            .render_with_dimensions(&code, 160, 120)
            .expect("color matrix raster render"),
    );
    assert_eq!((decoded.width, decoded.height), (160, 120));
    let bounds = image_bounds(&decoded, &[is_red, is_green, is_blue, is_black]);
    let split_x = (bounds.0 + bounds.2) as f64 / 2.;
    let split_y = (bounds.1 + bounds.3) as f64 / 2.;
    assert_quadrant(&decoded, is_red, split_x, split_y, true, true);
    assert_quadrant(&decoded, is_green, split_x, split_y, true, false);
    assert_quadrant(&decoded, is_blue, split_x, split_y, false, true);
    assert_quadrant(&decoded, is_black, split_x, split_y, false, false);
}

#[test]
fn raster_image_numeric_and_native_raster_inputs_render() {
    let mut session = RSession::new().expect("session");
    let grayscale_code = format!(
        "{}; rasterImage(matrix(c(0, .5, 1, .25), nrow=2), 0, 0, 1, 1)",
        plot_setup()
    );
    let grayscale = decode_png(
        &session
            .render_with_dimensions(&grayscale_code, 160, 120)
            .expect("numeric grayscale raster render"),
    );
    assert!(
        grayscale.matching(|pixel| {
            pixel[3] > 200
                && pixel[0] > 90
                && pixel[0] < 210
                && pixel[0] == pixel[1]
                && pixel[1] == pixel[2]
        }) > 100
    );

    let native_code = format!(
        "{}; par(xpd=TRUE); x <- structure(as.integer(c(-16776961L,-16711936L,-65536L,-16777216L)), dim=c(2L,2L), class='nativeRaster'); rasterImage(x, .5, 0, 1, 1, angle=90, interpolate=FALSE)",
        plot_setup()
    );
    let native = decode_png(
        &session
            .render_with_dimensions(&native_code, 160, 120)
            .expect("native raster render"),
    );
    let bounds = image_bounds(&native, &[is_red, is_green, is_blue, is_black]);
    let split_x = (bounds.0 + bounds.2) as f64 / 2.;
    let split_y = (bounds.1 + bounds.3) as f64 / 2.;
    // A 90-degree rotation about bottom-left keeps the source top-left at
    // the output bottom-left in the device's top-down pixel coordinates.
    assert_quadrant(&native, is_red, split_x, split_y, true, false);
    assert_quadrant(&native, is_green, split_x, split_y, false, false);
    assert_quadrant(&native, is_blue, split_x, split_y, true, true);
    assert_quadrant(&native, is_black, split_x, split_y, false, true);
}

#[test]
fn raster_image_xpd_clips_outside_plot_and_session_recovers() {
    let mut session = RSession::new().expect("session");
    let clipped_code = format!(
        "{}; par(xpd=FALSE); rasterImage(matrix(c('red','green','blue','black'), nrow=2), -.25, 0, .75, 1, angle=90, interpolate=FALSE)",
        plot_setup()
    );
    let clipped = decode_png(
        &session
            .render_with_dimensions(&clipped_code, 160, 120)
            .expect("clipped raster render"),
    );
    let visible_code = format!(
        "{}; par(xpd=TRUE); rasterImage(matrix(c('red','green','blue','black'), nrow=2), -.25, 0, .75, 1, angle=90, interpolate=FALSE)",
        plot_setup()
    );
    let visible = decode_png(
        &session
            .render_with_dimensions(&visible_code, 160, 120)
            .expect("xpd raster render"),
    );
    let colored = |pixel| is_red(pixel) || is_green(pixel) || is_blue(pixel) || is_black(pixel);
    assert!(visible.matching(colored) > clipped.matching(colored));

    let error = session.render_with_dimensions(
        &format!(
            "{}; rasterImage(matrix(c(-1, 2), nrow=1), 0, 0, 1, 1)",
            plot_setup()
        ),
        160,
        120,
    );
    assert!(error.is_err());
    assert_eq!(session.eval("1 + 1").expect("session recovery"), "[1] 2");
}
