//! Grid navigation and string-unit contracts pinned against GNU R 4.6.1.
//! Oracle: `/opt/homebrew/Cellar/r/4.6.1/bin/Rscript --vanilla`.
use r_embed::RSession;

fn report(session: &mut RSession, width: u32, height: u32, code: &str) -> String {
    session.render_with_dimensions(code, width, height).unwrap();
    // `eval` prints a length-1 character as `[1] "..."`. `cat` is the value.
    session.eval("cat(parity)").unwrap().trim().to_string()
}

#[test]
fn named_viewport_navigation_matches_gnu_r_4_6_1() {
    // 192x96 px at 96 dpi is a 2 by 1 inch device, so inch units stay exact.
    let mut session = RSession::new().unwrap();
    let parity = report(
        &mut session,
        192,
        96,
        r#"
        library(grid)
        grid.newpage()
        pushViewport(viewport(name='outer', width=unit(1,'inches')))
        pushViewport(viewport(name='mid'))
        pushViewport(viewport(name='leaf', width=unit(.25,'npc')))
        upViewport(2)
        dleaf <- downViewport('leaf')
        leaf_w <- convertWidth(unit(1,'npc'), 'inches', TRUE)
        leaf_name <- current.viewport()$name
        leaf_path <- as.character(current.vpPath())
        seek_outer <- seekViewport('outer')
        outer_w <- convertWidth(unit(1,'npc'), 'inches', TRUE)
        miss <- tryCatch(seekViewport('missing'), error=function(e) conditionMessage(e))
        root_name <- current.viewport()$name
        root_null <- is.null(current.vpPath())
        root_w <- convertWidth(unit(1,'npc'), 'inches', TRUE)

        grid.newpage()
        pushViewport(viewport(name='outer'))
        pushViewport(viewport(name='mid'))
        pushViewport(viewport(name='leaf'))
        upViewport(2)
        strict <- tryCatch(downViewport('leaf', strict=TRUE), error=function(e) conditionMessage(e))
        nastrict <- tryCatch(downViewport('leaf', strict=NA), error=function(e) conditionMessage(e))
        still <- current.viewport()$name

        grid.newpage()
        pushViewport(viewport(name='z'))
        pushViewport(viewport(name='t', width=unit(1,'inches')))
        upViewport(2)
        pushViewport(viewport(name='a'))
        pushViewport(viewport(name='t', width=unit(2,'inches')))
        upViewport(0)
        dt <- downViewport('t')
        t_path <- as.character(current.vpPath())
        t_w <- convertWidth(unit(1,'npc'), 'inches', TRUE)

        grid.newpage()
        pushViewport(viewport(name='a'))
        pushViewport(viewport(name='b'))
        pushViewport(viewport(name='c', width=unit(1,'inches')))
        upViewport(0)
        dstrict <- downViewport(c('a','b','c'), strict=TRUE)
        upViewport(0)
        dpart <- downViewport('b::c')
        part_path <- as.character(current.vpPath())
        upViewport(0)
        pushViewport(viewport(name='ab'))
        pushViewport(viewport(name='c', width=unit(4,'inches')))
        upViewport(0)
        dre <- downViewport('a.:b::c')
        re_path <- as.character(current.vpPath())
        re_w <- convertWidth(unit(1,'npc'), 'inches', TRUE)

        grid.newpage()
        pushViewport(viewport(name='p'))
        pushViewport(viewport(name='a', width=unit(1,'inches')))
        upViewport()
        pushViewport(viewport(name='a', width=unit(3,'inches')))
        rep_w <- convertWidth(unit(1,'npc'), 'inches', TRUE)
        popViewport()
        replaced <- tryCatch(downViewport('a'), error=function(e) conditionMessage(e))

        grid.newpage()
        pushViewport(viewport(name='a'))
        pushViewport(viewport(name='b'))
        pushViewport(viewport(name='c'))
        up_path <- as.character(upViewport(2))
        up_name <- current.viewport()$name
        up0 <- as.character(upViewport(0))
        up0_name <- current.viewport()$name

        grid.newpage()
        pushViewport(viewport())
        pushViewport(viewport(name='x'))
        auto <- as.character(current.vpPath())

        push_err <- tryCatch(pushViewport(), error=function(e) conditionMessage(e))
        grid.newpage()
        pushViewport(viewport(name='stay', width=unit(2,'inches')))
        down_err <- tryCatch(downViewport('nope'), error=function(e) conditionMessage(e))
        stay_name <- current.viewport()$name
        stay_w <- convertWidth(unit(1,'npc'), 'inches', TRUE)
        up_neg <- tryCatch(upViewport(-1), error=function(e) conditionMessage(e))
        pop_neg <- tryCatch(popViewport(-1), error=function(e) conditionMessage(e))
        up_far <- tryCatch(upViewport(2), error=function(e) conditionMessage(e))
        up_far_name <- current.viewport()$name

        fmt <- function(x) sprintf('%.6f', x)
        parity <- paste(
            dleaf, fmt(leaf_w), leaf_name, leaf_path, seek_outer, fmt(outer_w),
            miss, root_name, root_null, fmt(root_w),
            strict, nastrict, still,
            dt, t_path, fmt(t_w),
            dstrict, dpart, part_path, dre, re_path, fmt(re_w),
            fmt(rep_w), replaced,
            up_path, up_name, up0, up0_name, auto,
            push_err, down_err, stay_name, fmt(stay_w), up_neg, pop_neg, up_far, up_far_name,
            sep='|')
    "#,
    );
    assert_eq!(
        parity,
        [
            "2|0.250000|leaf|outer::mid::leaf|1|1.000000",
            "Viewport 'missing' was not found|ROOT|TRUE|2.000000",
            "Viewport 'leaf' was not found|Viewport 'leaf' was not found|outer",
            "2|a::t|2.000000",
            "3|3|a::b::c|3|a::b::c|1.000000",
            "3.000000|Viewport 'a' was not found",
            "b::c|a|a|ROOT|GRID.VP.1::x",
            "must specify at least one viewport|Viewport 'nope' was not found|stay|2.000000",
            "must navigate up at least one viewport|must pop at least one viewport",
            "cannot pop the top-level viewport ('grid' and 'graphics' output mixed?)|stay",
        ]
        .join("|")
    );
}

#[test]
fn string_and_grob_widths_match_gnu_r_4_6_1() {
    // 192x96 px at 96 dpi is a 2 by 1 inch device. Font pins are ratios against
    // this engine's own string metrics (DejaVu), which is what GNU scales too.
    // Absolute Helvetica inches are not the contract.
    let mut session = RSession::new().unwrap();
    let parity = report(
        &mut session,
        192,
        96,
        r#"
        library(grid)
        grid.newpage()
        near <- function(a, b) isTRUE(all.equal(as.numeric(a), as.numeric(b), tolerance=1e-4))
        w <- function(x) as.numeric(convertWidth(x, 'inches', TRUE))
        h <- function(x) as.numeric(convertHeight(x, 'inches', TRUE))
        sw <- w(stringWidth('abc'))
        sh <- h(stringHeight('abc'))
        wa <- w(stringWidth('a'))
        pushViewport(viewport(gp=gpar(fontsize=20)))
        sw20 <- w(stringWidth('abc'))
        sh20 <- h(stringHeight('abc'))
        popViewport()
        pushViewport(viewport(gp=gpar(cex=2)))
        swcex <- w(stringWidth('abc'))
        popViewport()
        pushViewport(viewport(gp=gpar(fontsize=20)))
        override <- w(grobWidth(textGrob('abc', gp=gpar(fontsize=12))))
        popViewport()
        cells <- numeric(8)
        pushViewport(viewport(width=unit(6,'inches'), height=unit(2,'inches'),
            layout=grid.layout(2, 2, widths=unit(c(1,2),'null'), heights=unit(c(1,3),'null'),
                respect=matrix(c(1,0,0,0), 2, 2))))
        k <- 1
        for (r in 1:2) for (col in 1:2) {
            pushViewport(viewport(layout.pos.row=r, layout.pos.col=col))
            cells[k] <- w(unit(1,'npc'))
            cells[k+1] <- h(unit(1,'npc'))
            k <- k+2
            popViewport()
        }
        popViewport()
        flags <- c(
            near(w(stringWidth('')), 0),
            near(h(stringHeight('')), sh),
            near(h(stringHeight('a')), sh),
            near(h(stringHeight(c('a','bb'))), c(sh, sh)),
            length(stringWidth(c('a','bb','ccc'))) == 3L,
            near(w(stringWidth(c('a','bb')))[1], wa),
            near(sw20 / sw, 20/12),
            near(sh20 / sh, 20/12),
            near(swcex / sw, 2),
            near(w(grobWidth(textGrob('abc'))), sw),
            near(h(grobHeight(textGrob('abc'))), sh),
            near(w(grobWidth(textGrob('abc', gp=gpar(fontsize=20)))), sw20),
            near(h(grobHeight(textGrob('abc', gp=gpar(fontsize=20)))), sh20),
            near(override, sw),
            near(w(grobWidth(textGrob(c('a','abcdef')))), wa),
            w(grobWidth(textGrob(c('a','abcdef')))) < w(stringWidth('abcdef')),
            near(w(grobWidth(textGrob(c('a','a'), x=unit(c(.25,.75),'npc')))), 1 + wa),
            near(w(grobWidth(textGrob('abc', rot=90))), sh),
            near(h(grobHeight(textGrob('abc', rot=90))), sw),
            near(w(grobWidth(textGrob('abc', rot=45))), (sw + sh) / sqrt(2)),
            near(h(grobHeight(textGrob('abc', rot=45))), (sw + sh) / sqrt(2)),
            near(w(grobWidth(textGrob(''))), 0),
            near(h(grobHeight(textGrob(''))), sh),
            near(w(stringWidth(12)), w(stringWidth('12'))),
            near(w(stringWidth(NA)), w(stringWidth('NA'))),
            near(w(stringWidth(factor('abc'))), sw),
            near(w(stringWidth(expression(x+y))), w(grobWidth(textGrob(expression(x+y))))),
            near(h(stringHeight(expression(x+y))), h(grobHeight(textGrob(expression(x+y))))),
            length(stringWidth(expression(a, abc))) == 2L,
            w(stringWidth(expression(x+y))) > 0,
            near(w(grobWidth(rectGrob(width=unit(.25,'npc')))), .5),
            near(h(grobHeight(rectGrob(height=unit(.5,'npc')))), .5),
            near(w(grobWidth(circleGrob(r=unit(.2,'snpc')))), .4),
            near(h(grobHeight(circleGrob(r=unit(.2,'snpc')))), .4),
            near(cells, c(.5,.5, 5.5,.5, .5,1.5, 5.5,1.5))
        )
        empty <- tryCatch(stringWidth(character(0)), error=function(e) conditionMessage(e))
        nulls <- tryCatch(stringWidth(NULL), error=function(e) conditionMessage(e))
        parity <- paste(c(ifelse(flags, 'T', 'F'), empty, nulls), collapse='|')
    "#,
    );
    let mut expected = vec!["T"; 35];
    expected.push("'x' and 'units' must have length > 0");
    expected.push("'x' and 'units' must have length > 0");
    assert_eq!(parity, expected.join("|"));
}
