//! Contract checks for the public histogram, barplot, and boxplot methods.

use std::io::Cursor;

use r_embed::RSession;

fn decode_rgba(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(Cursor::new(bytes));
    let mut reader = decoder.read_info().expect("png reader");
    let mut data = vec![0; reader.output_buffer_size().expect("png buffer")];
    let info = reader.next_frame(&mut data).expect("png frame");
    let pixels = match info.color_type {
        png::ColorType::Rgba => data[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => data[..info.buffer_size()]
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        other => panic!("unexpected PNG color type: {other:?}"),
    };
    (info.width, info.height, pixels)
}

#[test]
fn hist_returns_breaks_counts_density_and_mids() {
    let mut session = RSession::new().expect("session");
    let result = session
        .eval("h <- hist(c(0.1,0.2,0.8,1.2,1.9), breaks=c(0,1,2), plot=FALSE); identical(h$counts, c(3L,2L)) && identical(h$density, c(.6,.4)) && identical(h$mids, c(.5,1.5)) && isTRUE(h$equidist)")
        .expect("hist");
    assert_eq!(result, "[1] TRUE");
    let boundaries = session
        .eval("c(hist(c(0,1,2), breaks=c(0,1,2), fuzz=0, right=TRUE, include.lowest=TRUE, plot=FALSE)$counts, hist(c(0,1,2), breaks=c(0,1,2), fuzz=0, right=FALSE, include.lowest=TRUE, plot=FALSE)$counts)")
        .expect("hist boundaries");
    assert_eq!(boundaries, "[1] 2 1 1 2");
    let xname = session
        .eval("foo <- c(0, 1); hist(foo, breaks=c(0, 1), plot=FALSE)$xname")
        .expect("hist xname");
    assert_eq!(xname, "[1] \"foo\"");
}

#[test]
fn pretty_public_frontend_matches_r_pretty_contract() {
    let mut session = RSession::new().expect("session");
    let result = session
        .eval("identical(pretty(c(1,4)), c(1,1.5,2,2.5,3,3.5,4)) && identical(pretty(c(1,4), bounds=FALSE), c(1,1.5,2,2.5,3,3.5,4)) && length(pretty(NULL)) == 0L && identical(pretty(c(1,4), n=5.7), pretty(c(1,4))) && identical(pretty.default(x=c(1,4), 5L), pretty(c(1,4)))")
        .expect("pretty");
    assert_eq!(result, "[1] TRUE");
}

#[test]
fn par_background_and_label_colors_reach_native_pixels() {
    let mut session = RSession::new().expect("session");
    let png = session
        .render_with_dimensions(
            "par(bg='black', fg='white', col.axis='white', col.lab='white', col.main='white'); plot(1:3, type='n', main='DARK', xlab='X', ylab='Y')",
            320,
            240,
        )
        .expect("dark plot render");
    let (width, _height, pixels) = decode_rgba(&png);
    let pixel = |x: u32, y: u32| {
        let i = ((y * width + x) * 4) as usize;
        &pixels[i..i + 4]
    };
    assert_eq!(pixel(0, 0), &[0, 0, 0, 255]);
    let white_ink = pixels
        .chunks_exact(4)
        .filter(|p| p[0] > 220 && p[1] > 220 && p[2] > 220 && p[3] > 200)
        .count();
    assert!(
        white_ink > 100,
        "expected visible white axes/labels, found {white_ink}"
    );

    let red_png = session
        .render_with_dimensions(
            "palette(c('black','red')); par(bg='black', fg=2, col.axis=2, col.lab=2, col.main=2); plot(1:3, type='n', main='RED')",
            320,
            240,
        )
        .expect("palette foreground render");
    let (_, _, red_pixels) = decode_rgba(&red_png);
    let red_ink = red_pixels
        .chunks_exact(4)
        .filter(|p| p[0] > 180 && p[1] < 90 && p[2] < 90 && p[3] > 100)
        .count();
    assert!(
        red_ink > 50,
        "expected palette index 2 red ink, found {red_ink}"
    );

    let alpha_png = session
        .render_with_dimensions(
            "par(bg='black', fg='#ff000080', col.axis='#ff000080', col.lab='#ff000080', col.main='#ff000080'); plot(1:3, type='n', main='ALPHA')",
            320,
            240,
        )
        .expect("alpha foreground render");
    let (_, _, alpha_pixels) = decode_rgba(&alpha_png);
    let alpha_ink = alpha_pixels
        .chunks_exact(4)
        .filter(|p| p[0] > 60 && p[0] < 220 && p[1] < 80 && p[2] < 80)
        .count();
    assert!(
        alpha_ink > 30,
        "expected semi-transparent red ink, found {alpha_ink}"
    );

    let transparent_png = session
        .render_with_dimensions(
            "par(bg='black', fg='transparent', col.axis='transparent', col.lab='transparent', col.main='transparent'); plot(1:3, type='n', main='HIDDEN')",
            320,
            240,
        )
        .expect("transparent foreground render");
    let (_, _, transparent_pixels) = decode_rgba(&transparent_png);
    let visible_ink = transparent_pixels
        .chunks_exact(4)
        .filter(|p| p[0] > 8 || p[1] > 8 || p[2] > 8)
        .count();
    assert!(
        visible_ink < 100,
        "transparent foreground left {visible_ink} visible pixels"
    );
}

#[test]
fn barplot_plot_false_returns_bar_centers_and_plotting_works() {
    let mut session = RSession::new().expect("session");
    let result = session
        .eval("identical(round(barplot(c(2,4,3), plot=FALSE), 10), c(.7,1.9,3.1))")
        .expect("barplot");
    assert_eq!(result, "[1] TRUE");
    let matrix_positions = session
        .eval("m <- matrix(c(2,4,3,1,5,2), nrow=2); all(abs(as.vector(barplot(m, beside=TRUE, plot=FALSE)) - c(1.5,2.5,4.5,5.5,7.5,8.5)) < 1e-12) && all(abs(as.vector(barplot(m, beside=FALSE, plot=FALSE)) - c(.7,1.9,3.1)) < 1e-12)")
        .expect("barplot positions");
    assert_eq!(matrix_positions, "[1] TRUE", "matrix positions: {}", session.eval("m <- matrix(c(2,4,3,1,5,2), nrow=2); paste(as.vector(barplot(m, beside=TRUE, plot=FALSE)), collapse=','); paste(as.vector(barplot(m, beside=FALSE, plot=FALSE)), collapse=',')").unwrap());
    let png = session
        .render_with_dimensions("barplot(c(2,4,3), col='red', main='bars')", 320, 240)
        .expect("barplot render");
    assert!(png.len() > 100, "rendered PNG should contain a plot");
    let signed = session
        .eval("p <- barplot(c(-2,0,3), names.arg=c('neg','zero','pos'), plot=FALSE); length(p) == 3L && p[1] < p[2] && p[2] < p[3]")
        .expect("signed bars");
    assert_eq!(signed, "[1] TRUE");
    let signed_png = session
        .render_with_dimensions(
            "barplot(c(-2,0,3), names.arg=c('neg','zero','pos'), main='signed')",
            320,
            240,
        )
        .expect("signed barplot render");
    assert!(
        signed_png.len() > 100,
        "signed bars should render with default log=''"
    );
}

#[test]
fn boxplot_plot_false_returns_upstream_statistic_fields() {
    let mut session = RSession::new().expect("session");
    let result = session
        .eval("b <- boxplot(c(1,2,3,4,100), plot=FALSE); identical(dim(b$stats), c(5L,1L)) && identical(b$n, 5) && identical(b$stats[,1], c(1,2,3,4,4)) && identical(round(b$conf[,1],6), c(1.586805,4.413195)) && identical(b$out, 100) && identical(b$group, 1)")
        .expect("boxplot");
    assert_eq!(result, "[1] TRUE", "box contract: {}", session.eval("b <- boxplot(c(1,2,3,4,100), plot=FALSE); paste(identical(dim(b$stats), c(5L,1L)), identical(b$n, 5L), identical(b$stats[,1], c(1,2,3,4,4)), identical(round(b$conf[,1],6), c(1.586805,4.413195)), identical(b$out, 100), identical(b$group, 1), sep='|')").unwrap());
    let empty = session
        .eval("b <- boxplot(numeric(), plot=FALSE); identical(b$n, 0) && all(is.na(b$stats)) && length(b$out) == 0L")
        .expect("empty boxplot");
    assert_eq!(empty, "[1] TRUE");
    let hinges = session
        .eval("b <- boxplot(1:4, plot=FALSE); identical(b$stats[,1], c(1,1.5,2.5,3.5,4)) && identical(boxplot(1:5, plot=FALSE)$stats[,1], c(1,2,3,4,5))")
        .expect("Tukey hinges");
    assert_eq!(hinges, "[1] TRUE");
    let coef_zero = session
        .eval("b <- boxplot(c(1,2,3,4,100), range=0, plot=FALSE); length(b$out) == 0L && identical(b$stats[,1], c(1,2,3,4,100))")
        .expect("range zero");
    assert_eq!(coef_zero, "[1] TRUE");
    let groups = session
        .eval("b <- boxplot(c(1,2), c(3,4), plot=FALSE); identical(dim(b$stats), c(5L,2L)) && identical(b$n, c(2,2))")
        .expect("multiple boxplot groups");
    assert_eq!(groups, "[1] TRUE");
    let png = session
        .render_with_dimensions(
            "boxplot(c(1,2,3,4,100), col='skyblue', main='box')",
            320,
            240,
        )
        .expect("boxplot render");
    assert!(png.len() > 100, "rendered PNG should contain a plot");
    let custom_at = session
        .render_with_dimensions(
            "boxplot(c(1,2,3), c(4,5,6), at=c(10,20), col='skyblue')",
            320,
            240,
        )
        .expect("boxplot custom at render");
    assert!(
        custom_at.len() > 100,
        "custom-at render should contain a plot"
    );
}

#[test]
fn complete_statistics_fixture_matches_pinned_oracle() {
    let mut session = RSession::new().unwrap();
    let actual = session
        .eval(include_str!(
            "../../../tests/conformance/cases/572_graphics_statistics.R"
        ))
        .unwrap();
    let normalized = |text: &str| {
        text.lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_owned()
    };
    assert_eq!(
        normalized(&actual),
        normalized(include_str!(
            "../../../tests/conformance/golden/572_graphics_statistics.out"
        ))
    );
}

#[test]
fn barplot_accepts_one_dimensional_tables_like_numeric_vectors() {
    let mut session = RSession::new().unwrap();
    let result = session.eval("counts <- table(c(1,1,2,3,3,3)); max(abs(as.vector(barplot(counts, plot=FALSE)) - c(.7,1.9,3.1))) < 1e-12").unwrap();
    assert_eq!(result, "[1] TRUE");
    let png = session
        .render_with_dimensions("barplot(counts)", 320, 240)
        .unwrap();
    assert!(!png.is_empty());
}
