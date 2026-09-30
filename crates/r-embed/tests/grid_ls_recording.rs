//! grid.ls listing and recording=FALSE, pinned to GNU R 4.6.1.
//! Oracle: `/opt/homebrew/Cellar/r/4.6.1/bin/Rscript --vanilla`.
use r_embed::RSession;
use std::io::Cursor;

fn pixels(bytes: &[u8]) -> (usize, Vec<u8>) {
    let mut reader = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    let pixels = match info.color_type {
        png::ColorType::Rgba => pixels[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => pixels[..info.buffer_size()]
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        other => panic!("unexpected PNG {other:?}"),
    };
    (info.width as usize, pixels)
}

fn at(width: usize, pixels: &[u8], x: usize, y: usize) -> &[u8] {
    &pixels[(y * width + x) * 4..(y * width + x + 1) * 4]
}

fn report(session: &mut RSession, code: &str) -> String {
    session
        .render_with_dimensions(code, 192, 96)
        .unwrap_or_else(|err| panic!("{err}"));
    session.eval("cat(out)").unwrap().trim().to_string()
}

#[test]
fn grid_ls_matches_gnu_listing_and_recording_false_omits_grobs() {
    // GNU R 4.6.1 prints this for
    // grid.newpage(); grid.text("hi", name="label"); grid.rect(name="box"); grid.ls()
    // and indents gTree children by two spaces. An empty display list prints one blank line.
    // grid.newpage(recording=FALSE) clears the page but keeps the display list.
    let mut session = RSession::new().unwrap();
    let out = report(&mut session, r#"
        library(grid)
        grid.newpage()
        grid.text('hi', name='label')
        grid.rect(name='box')
        scene <- paste(capture.output(grid.ls()), collapse='\n')

        grid.newpage()
        empty_lines <- capture.output(grid.ls())
        empty_first <- if (length(empty_lines)) empty_lines[[1L]] else 'MISSING'

        grid.newpage()
        grid.text('hi', name='label')
        grid.draw(rectGrob(name='box'), recording=FALSE)
        drawn_false <- paste(capture.output(grid.ls()), collapse='\n')

        grid.newpage()
        grid.draw(textGrob('hi', name='label'), recording=TRUE)
        grid.draw(rectGrob(name='box'), recording=TRUE)
        drawn_true <- paste(capture.output(grid.ls()), collapse='\n')

        grid.newpage()
        grid.text('hi', name='label')
        grid.rect(name='box')
        grid.newpage(recording=FALSE)
        kept <- paste(capture.output(grid.ls()), collapse='\n')
        grid.draw(textGrob('no', name='hidden'), recording=FALSE)
        still <- paste(capture.output(grid.ls()), collapse='\n')

        grid.newpage()
        grid.draw(grobTree(textGrob('hi', name='label'), rectGrob(name='box'), name='tree'))
        tree <- paste(capture.output(grid.ls()), collapse='\n')

        pushViewport(viewport(name='inner'), recording=FALSE)
        pushed <- current.viewport()$name
        popViewport(recording=FALSE)
        popped <- current.viewport()$name
        pushViewport(viewport(name='a'))
        pushViewport(viewport(name='b'))
        upViewport(1, recording=FALSE)
        upn <- current.viewport()$name
        downViewport('b', recording=FALSE)
        downn <- current.viewport()$name
        seekViewport('a', recording=FALSE)
        seekn <- current.viewport()$name
        nav <- paste(pushed, popped, upn, downn, seekn, sep='|')

        grid.newpage()
        grid.text('hi', name='label')
        grid.draw(rectGrob(name='painted', gp=gpar(fill='red')), recording=FALSE)
        final_ls <- paste(capture.output(grid.ls()), collapse='\n')

        out <- paste(scene, length(empty_lines), empty_first, drawn_false, drawn_true, kept, still, tree, nav, final_ls, sep='||')
    "#);
    assert_eq!(
        out,
        [
            "label\nbox",
            "1",
            "",
            "label",
            "label\nbox",
            "label\nbox",
            "label\nbox",
            "tree\n  label\n  box",
            "inner|ROOT|a|b|a",
            "label",
        ]
        .join("||")
    );
    let png = session
        .render_with_dimensions(
            "library(grid); grid.newpage(); grid.draw(rectGrob(name='painted', gp=gpar(fill='red')), recording=FALSE); listed <- paste(capture.output(grid.ls()), collapse='\n')",
            192,
            96,
        )
        .unwrap();
    let (width, pixels) = pixels(&png);
    assert_eq!(at(width, &pixels, 10, 10), &[255, 0, 0, 255]);
    assert_eq!(session.eval("cat(listed)").unwrap().trim(), "");
}

#[test]
fn current_viewport_inches_follow_the_pushed_viewport() {
    // 192x96 px at 96 dpi is 2 by 1 inch. GNU convertX/convertY/convertWidth/convertHeight
    // of unit(0.5,'npc'), unit(0.5,'npc'), unit(0.5,'npc'), unit(0.25,'npc') on that
    // device are 1, 0.5, 1, 0.25 inches. ROOT is the full page (centre 1, 0.5; size 2 by 1).
    let mut session = RSession::new().unwrap();
    let out = report(&mut session, r#"
        library(grid)
        grid.newpage()
        root <- current.viewport()
        pushViewport(viewport(x=0.5, y=0.5, width=0.5, height=0.25, default.units='npc', name='inner'))
        inner <- current.viewport()
        fmt <- function(v) sprintf('%.8f', as.numeric(v))
        near <- length(c(inner$x, inner$y, inner$width, inner$height)) == 4L && isTRUE(all(abs(as.numeric(c(inner$x, inner$y, inner$width, inner$height)) - c(1, 0.5, 1, 0.25)) < 1e-6))
        root_near <- length(c(root$x, root$y, root$width, root$height)) == 4L && isTRUE(all(abs(as.numeric(c(root$x, root$y, root$width, root$height)) - c(1, 0.5, 2, 1)) < 1e-6))
        out <- paste(root$name, fmt(root$x), fmt(root$y), fmt(root$width), fmt(root$height), root_near,
            inner$name, fmt(inner$x), fmt(inner$y), fmt(inner$width), fmt(inner$height), near,
            sep='|')
    "#);
    assert_eq!(
        out,
        "ROOT|1.00000000|0.50000000|2.00000000|1.00000000|TRUE|inner|1.00000000|0.50000000|1.00000000|0.25000000|TRUE"
    );
}

#[test]
fn fractional_up_and_pop_truncate_toward_zero_like_gnu() {
    // GNU R 4.6.1 on a::b::c::d. as.integer truncates toward 0, and the C
    // viewport walk still moves one viewport when that integer is 0.
    // Exact 0 means the whole stack. n < 0 stops. Five steps past a depth of
    // four stops. up(1) at ROOT is left as this port's top-level stop.
    let mut session = RSession::new().unwrap();
    let out = report(&mut session, r#"
        library(grid)
        step <- function(label, which, n) {
          grid.newpage()
          pushViewport(viewport(name='a'))
          pushViewport(viewport(name='b'))
          pushViewport(viewport(name='c'))
          pushViewport(viewport(name='d'))
          tryCatch({
            if (which == 'up') upViewport(n) else popViewport(n)
            paste0(label, '=', current.viewport()$name)
          }, error = function(e) paste0(label, '=ERR:', conditionMessage(e)))
        }
        root_pop <- tryCatch({
          grid.newpage()
          popViewport(1)
          'pop-root=OK'
        }, error = function(e) paste0('pop-root=ERR:', conditionMessage(e)))
        out <- paste(c(
          step('up-1', 'up', -1),
          step('pop-1', 'pop', -1),
          step('up0', 'up', 0),
          step('pop0', 'pop', 0),
          step('up0.1', 'up', 0.1),
          step('pop0.1', 'pop', 0.1),
          step('up0.9', 'up', 0.9),
          step('pop0.9', 'pop', 0.9),
          step('up1', 'up', 1),
          step('pop1', 'pop', 1),
          step('up1.1', 'up', 1.1),
          step('pop1.1', 'pop', 1.1),
          step('up1.9', 'up', 1.9),
          step('pop1.9', 'pop', 1.9),
          step('up2', 'up', 2),
          step('pop2', 'pop', 2),
          step('up2.1', 'up', 2.1),
          step('pop2.1', 'pop', 2.1),
          step('up3', 'up', 3),
          step('pop3', 'pop', 3),
          step('up4', 'up', 4),
          step('pop4', 'pop', 4),
          step('up5', 'up', 5),
          step('pop5', 'pop', 5),
          root_pop
        ), collapse='|')
    "#);
    let expected = [
        "up-1=ERR:must navigate up at least one viewport",
        "pop-1=ERR:must pop at least one viewport",
        "up0=ROOT",
        "pop0=ROOT",
        "up0.1=c",
        "pop0.1=c",
        "up0.9=c",
        "pop0.9=c",
        "up1=c",
        "pop1=c",
        "up1.1=c",
        "pop1.1=c",
        "up1.9=c",
        "pop1.9=c",
        "up2=b",
        "pop2=b",
        "up2.1=b",
        "pop2.1=b",
        "up3=a",
        "pop3=a",
        "up4=ROOT",
        "pop4=ROOT",
        "up5=ERR:cannot pop the top-level viewport ('grid' and 'graphics' output mixed?)",
        "pop5=ERR:cannot pop the top-level viewport ('grid' and 'graphics' output mixed?)",
        "pop-root=ERR:cannot pop the top-level viewport ('grid' and 'graphics' output mixed?)",
    ]
    .join("|");
    assert_eq!(out, expected);
}
