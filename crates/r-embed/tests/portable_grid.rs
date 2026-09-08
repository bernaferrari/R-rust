//! Public grid contracts exercised through the interpreter and actual PNG device.
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
#[test]
fn nested_viewports_transform_clip_and_restore_parent() {
    let mut session = RSession::new().unwrap();
    let png=session.render_with_dimensions("library(grid); grid.newpage(); pushViewport(viewport(x=.25,y=.5,width=.5,height=.5,clip='on')); grid.rect(width=2,height=2,gp=gpar(fill='red',col=NA)); popViewport(); grid.rect(x=.8,y=.8,width=.1,height=.1,gp=gpar(fill='blue',col=NA))", 400,200).unwrap();
    let (w, p) = pixels(&png);
    assert_eq!(at(w, &p, 50, 100), [255, 0, 0, 255]);
    assert_eq!(at(w, &p, 250, 100), [255, 255, 255, 255]);
    assert_eq!(at(w, &p, 50, 20), [255, 255, 255, 255]);
    assert_eq!(at(w, &p, 320, 40), [0, 0, 255, 255]);
}
#[test]
fn units_and_layout_match_pinned_r_oracle_values() {
    // GNU R at png(width=384,height=192,res=96): 4 by 2 inches.
    // Viewport xscale=c(10,30), width=0.5 npc: native x=15 is 0.5 in.
    let mut session = RSession::new().unwrap();
    session.render_with_dimensions("library(grid); grid.newpage(); a<-convertWidth(unit(c(1,2.54,25.4,72.27,72),c('inches','cm','mm','points','bigpts')),'inches',TRUE); pushViewport(viewport(width=.5,height=.5,xscale=c(10,30))); b<-c(convertX(unit(15,'native'),'inches',TRUE),convertWidth(unit(5,'native'),'inches',TRUE),convertY(unit(.5,'npc'),'inches',TRUE)); popViewport(); pushViewport(viewport(layout=grid.layout(2,2,widths=unit(c(1,3),'null'),heights=unit(c(1,1),'null')))); pushViewport(viewport(layout.pos.row=1,layout.pos.col=2)); c1<-c(convertWidth(unit(1,'npc'),'inches',TRUE),convertHeight(unit(1,'npc'),'inches',TRUE)); popViewport(2)",384,192).unwrap();
    assert_eq!(session.eval("all(abs(a-rep(1,5))<1e-12) && all(abs(b-c(.5,.5,.5))<1e-12) && all(abs(c1-c(3,1))<1e-12)").unwrap(),"[1] TRUE");
}
#[test]
fn layout_positions_draw_in_top_right_cell() {
    let mut session = RSession::new().unwrap();
    let png=session.render_with_dimensions("library(grid); grid.newpage(); pushViewport(viewport(layout=grid.layout(2,2,widths=unit(c(1,3),'null')))); pushViewport(viewport(layout.pos.row=1,layout.pos.col=2)); grid.rect(gp=gpar(fill='red',col=NA)); popViewport(2)",400,200).unwrap();
    let (w, p) = pixels(&png);
    assert_eq!(at(w, &p, 50, 50), [255, 255, 255, 255]);
    assert_eq!(at(w, &p, 200, 50), [255, 0, 0, 255]);
    assert_eq!(at(w, &p, 200, 150), [255, 255, 255, 255]);
}

#[test]
fn layout_respect_centers_square_cells_and_empty_coordinates_are_noop() {
    let mut session = RSession::new().unwrap();
    let png = session.render_with_dimensions("library(grid); grid.newpage(); pushViewport(viewport(layout=grid.layout(1,2,widths=unit(c(1,1),'null'),heights=unit(1,'null'),respect=TRUE))); pushViewport(viewport(layout.pos.col=1)); grid.rect(gp=gpar(fill='red',col=NA)); popViewport(); pushViewport(viewport(layout.pos.col=2)); grid.rect(gp=gpar(fill='blue',col=NA)); popViewport(2)",400,200).unwrap();
    let (w, p) = pixels(&png);
    assert_eq!(at(w, &p, 50, 50), [255, 0, 0, 255]);
    assert_eq!(at(w, &p, 100, 100), [255, 0, 0, 255]);
    assert_eq!(at(w, &p, 300, 100), [0, 0, 255, 255]);
    assert!(
        session
            .render_with_dimensions(
                "library(grid); grid.newpage(); grid.segments(numeric(),0,1,1)",
                100,
                100
            )
            .is_ok()
    );
}

#[test]
fn layout_fixed_null_spans_and_invalid_respect_shapes_are_checked() {
    let mut session = RSession::new().unwrap();
    assert_eq!(session.eval("library(grid); z<-grid.layout(2,2,widths=unit(c(1,2),'null'),heights=unit(c(1,20),'points')); isTRUE(z$nrow==2 && z$ncol==2 && length(z$widths$value)==2L)").unwrap(), "[1] TRUE");
    assert!(
        session
            .eval("library(grid); grid.layout(2,2,respect=matrix(c(TRUE,FALSE),1,2))")
            .is_ok()
    );
    assert!(session.eval("library(grid); grid.layout(0,2)").is_err());
    assert!(session.eval("library(grid); grid.layout(2,2,widths=unit(c(1,2),'null'),heights=unit(c(1,2),'null'))").is_ok());
}

#[test]
fn respected_layout_uses_common_cell_scale_on_wide_device() {
    let mut session = RSession::new().unwrap();
    let png = session.render_with_dimensions("library(grid); grid.newpage(); pushViewport(viewport(layout=grid.layout(1,2,respect=TRUE))); pushViewport(viewport(layout.pos.col=1)); grid.rect(gp=gpar(fill='red',col=NA)); popViewport(); pushViewport(viewport(layout.pos.col=2)); grid.rect(gp=gpar(fill='blue',col=NA)); popViewport(2)",600,200).unwrap();
    let (w, p) = pixels(&png);
    assert_eq!(at(w, &p, 50, 100), [255, 255, 255, 255]);
    assert_eq!(at(w, &p, 200, 100), [255, 0, 0, 255]);
    assert_eq!(at(w, &p, 400, 100), [0, 0, 255, 255]);
    assert_eq!(at(w, &p, 550, 100), [255, 255, 255, 255]);
    assert_eq!(at(w, &p, 200, 10), [255, 0, 0, 255]);
    assert_eq!(at(w, &p, 200, 190), [255, 0, 0, 255]);
}
#[test]
fn grob_trees_replay_with_inherited_styles_and_plotmath() {
    let mut session = RSession::new().unwrap();
    let png=session.render_with_dimensions("library(grid); grid.newpage(); g<-grobTree(rectGrob(width=.8,height=.8),textGrob(expression(frac(alpha[1],sqrt(x^2+1))),gp=gpar(col='black',fontsize=24)),gp=gpar(fill='red',col=NA),vp=viewport(width=.5,height=.5)); grid.draw(g); p<-recordPlot(); grid.newpage(); replayPlot(p)",400,200).unwrap();
    let (w, p) = pixels(&png);
    assert_eq!(at(w, &p, 150, 100), [255, 0, 0, 255]);
    assert_eq!(at(w, &p, 50, 100), [255, 255, 255, 255]);
    assert!(
        p.chunks_exact(4)
            .filter(|p| p[0] < 60 && p[1] < 60 && p[2] < 60)
            .count()
            > 20
    );
    assert_eq!(
        session
            .eval("is.grob(g) && inherits(g,'gTree') && length(g$children)==2L")
            .unwrap(),
        "[1] TRUE"
    );
}
#[test]
fn failed_grob_restores_viewport_and_session_remains_usable() {
    let mut session = RSession::new().unwrap();
    let png=session.render_with_dimensions("library(grid); grid.newpage(); tryCatch(grid.draw(linesGrob(arrow=1,vp=viewport(width=.25))),error=function(e) NULL); grid.rect(x=.75,width=.1,height=.1,gp=gpar(fill='blue',col=NA))",400,200).unwrap();
    let (w, p) = pixels(&png);
    assert_eq!(at(w, &p, 300, 100), [0, 0, 255, 255]);
    assert!(
        session
            .render_with_dimensions("library(grid); grid.newpage(); popViewport()", 400, 200)
            .is_err()
    );
    assert!(
        session
            .render_with_dimensions("library(grid); grid.newpage(); grid.circle(r=.2)", 400, 200)
            .is_ok()
    );
}

#[test]
fn grid_namespace_is_attached_cached_and_export_restricted() {
    let mut session = RSession::new().unwrap();
    assert_eq!(session.eval("library(grid); pkg<-'grid'; isTRUE(require(pkg,character.only=TRUE)) && requireNamespace('grid',quietly=TRUE) && is.environment(getNamespace('grid')) && is.unit(grid::unit(1,'npc')) && 'package:grid' %in% search() && 'grid' %in% loadedNamespaces()").unwrap(),"[1] TRUE");
    assert!(session.eval("grid::mean(1:3)").is_err());
}
#[test]
fn grid_objects_namespace_and_recording_survive_forced_collection() {
    let mut session = RSession::new().unwrap();
    let png=session.render_with_dimensions("gctorture(TRUE); library(grid); grid.newpage(); g<-grobTree(rectGrob(width=.8,height=.8,gp=gpar(fill='red')),textGrob(expression(alpha[1]^2))); grid.draw(g); saved<-serialize(recordPlot(),NULL); gc(); grid.newpage(); replayPlot(unserialize(saved)); gctorture(FALSE)",240,160).unwrap();
    let (_, p) = pixels(&png);
    assert!(
        p.chunks_exact(4)
            .filter(|p| p[0] > 200 && p[1] < 20 && p[2] < 20)
            .count()
            > 1000
    );
    assert_eq!(
        session
            .eval("is.grob(g) && is.environment(getNamespace('grid'))")
            .unwrap(),
        "[1] TRUE"
    );
}
#[test]
fn malformed_internal_conversion_reports_an_r_error() {
    let mut session = RSession::new().unwrap();
    for axis in ["2", "-1", "0.5"] {
        let script = format!(
            "library(grid); grid.newpage(); .rport_grid('convert',list(x=unit(1,'npc'),axis={axis},dimension=FALSE,to='inches'))"
        );
        assert!(session.render_with_dimensions(&script, 240, 160).is_err());
    }
    assert!(
        session
            .render_with_dimensions("library(grid); grid.newpage(); grid.rect()", 240, 160)
            .is_ok()
    );
}

#[test]
fn grouped_polygons_dash_styles_symbols_and_arrows_are_drawn() {
    let mut session = RSession::new().unwrap();
    let png = session
        .render_with_dimensions(
            "library(grid); grid.newpage(); grid.polygon(x=unit(c(.1,.4,.4,.1,.6,.9,.9,.6),'npc'), y=unit(c(.1,.1,.4,.4,.6,.6,.9,.9),'npc'), id.lengths=c(4,4), gp=gpar(fill=c('red','blue'),lty='dashed')); grid.points(x=seq(.15,.85,length.out=6),y=.5,pch=0:5,size=unit(.2,'npc'),gp=gpar(col='black')); grid.segments(.1,.05,.9,.05,arrow=list(ends='both',type='closed',angle=30,length=unit(.2,'npc')),gp=gpar(col='black'))",
            320,
            240,
        )
        .unwrap();
    let (_, p) = pixels(&png);
    assert!(
        p.chunks_exact(4)
            .filter(|p| p[0] > 180 && p[1] < 80)
            .count()
            > 500
    );
    assert!(
        p.chunks_exact(4)
            .filter(|p| p[2] > 120 && p[0] < 100)
            .count()
            > 500
    );
    assert!(
        p.chunks_exact(4)
            .filter(|p| p[0] < 60 && p[1] < 60 && p[2] < 60)
            .count()
            > 40
    );
}

#[test]
fn unit_arithmetic_summaries_and_named_navigation_have_bounded_contracts() {
    let mut session = RSession::new().unwrap();
    assert_eq!(session.eval("library(grid); a<-unit(c(1,2),'npc'); b<-unit(3,'npc'); identical((a+b)$value,c(4,5)) && identical((a*2)$value,c(2,4))").unwrap(), "[1] TRUE");
    assert_eq!(
        session
            .eval("library(grid); a<-unit(c(1,2),'npc'); c(sum(a)$value,min(a)$value,max(a)$value)")
            .unwrap(),
        "[1] 3 1 2"
    );
    assert!(
        session
            .eval("library(grid); unit(1,'npc') + unit(1,'inches')")
            .is_err()
    );
    assert!(
        session
            .eval("library(grid); min(unit(1,'npc'),unit(1,'inches'))")
            .is_err()
    );
    assert_eq!(
        session
            .eval("library(grid); is.unit(unit(1,'strwidth',data='abcd'))")
            .unwrap(),
        "[1] TRUE"
    );
    assert!(session.render_with_dimensions("library(grid); grid.newpage(); pushViewport(viewport(name='outer')); pushViewport(viewport(name='inner')); seekViewport('outer'); grid.rect(gp=gpar(fill='red')); upViewport()", 160, 120).is_ok());
}

#[test]
fn unit_arithmetic_matches_gnu_r_contract_and_survives_gc() {
    let mut session = RSession::new().unwrap();
    assert_eq!(session.eval("library(grid); test_units<-function(){gctorture(TRUE); on.exit(gctorture(FALSE)); a<-unit(c(1,2),'npc'); b<-unit(c(3,4),'npc'); identical((a+b)$value,c(4,6)) && identical((-a)$value,c(-1,-2)) && identical((0*a)$value,c(0,0))};test_units()").unwrap(), "[1] TRUE");
    assert!(
        session
            .eval("library(grid); unit(1,'npc') * unit(2,'npc')")
            .is_err()
    );
    assert!(session.eval("library(grid); 2 / unit(1,'npc')").is_err());
    assert_eq!(
        session
            .eval("library(grid); length((unit(1,'npc') * numeric(0))$value)")
            .unwrap(),
        "[1] 0"
    );
    assert_eq!(session.eval("library(grid); a<-unit(c(1,NA_real_,3),'npc'); is.na(sum(a,na.rm=TRUE)$value) && is.na(sum(a,na.rm=FALSE)$value)").unwrap(), "[1] TRUE");
    assert_eq!(session.eval("library(grid); a<-unit(c(1,2),'npc'); b<-unit(c(3,4),'npc'); identical(sum(a,b)$value,10) && identical(min(a,b)$value,1) && identical(max(a,b)$value,4)").unwrap(), "[1] TRUE");
}

#[test]
fn grid_exports_require_attachment_but_namespace_calls_do_not() {
    let mut session = RSession::new().unwrap();
    assert!(session.eval("unit(1,'npc')").is_err());
    assert!(
        session
            .render_with_dimensions("grid.rect()", 240, 160)
            .is_err()
    );
    assert!(
        session
            .render_with_dimensions(
                "grid::grid.newpage(); grid::grid.rect(gp=grid::gpar(fill='red'))",
                240,
                160
            )
            .is_ok()
    );
    assert_eq!(
        session
            .eval("inherits(grid::unit(1,'npc'),'unit') && !('package:grid' %in% search())")
            .unwrap(),
        "[1] TRUE"
    );
    assert_eq!(
        session
            .eval("library(grid); is.unit(unit(1,'npc'))")
            .unwrap(),
        "[1] TRUE"
    );
}

/// Pinned GNU R deviceLoc/convertWidth/convertHeight measurements from
/// tests/grid-layout-oracle.R. Our device uses 96 pixels/inch; oracle uses 100.
#[test]
fn layout_cell_bounds_match_gnu_r_oracle() {
    let cases: &[(&str, &str, [f64; 4])] = &[
        (
            "grid.layout(1,2,widths=unit(c(1,2),'null'),respect=TRUE)",
            "layout.pos.col=1",
            [0., 1., 2., 2.],
        ),
        (
            "grid.layout(1,2,widths=unit(c(1,2),'null'),respect=TRUE)",
            "layout.pos.col=2",
            [2., 1., 4., 2.],
        ),
        (
            "grid.layout(2,1,heights=unit(c(1,2),'null'),respect=TRUE)",
            "layout.pos.row=1",
            [7. / 3., 8. / 3., 4. / 3., 4. / 3.],
        ),
        (
            "grid.layout(2,1,heights=unit(c(1,2),'null'),respect=TRUE)",
            "layout.pos.row=2",
            [7. / 3., 0., 4. / 3., 8. / 3.],
        ),
        (
            "grid.layout(2,3,widths=unit(c(1,2,3),'null'),heights=unit(c(1,1),'null'),respect=matrix(c(TRUE,FALSE,FALSE,FALSE,TRUE,FALSE),2,3))",
            "layout.pos.row=1,layout.pos.col=1",
            [0., 3., 1., 1.],
        ),
        (
            "grid.layout(2,3,widths=unit(c(1,2,3),'null'),heights=unit(c(1,1),'null'),respect=matrix(c(TRUE,FALSE,FALSE,FALSE,TRUE,FALSE),2,3))",
            "layout.pos.row=2,layout.pos.col=2",
            [1., 0., 2., 3.],
        ),
        (
            "grid.layout(1,3,widths=unit.c(unit(1,'inches'),unit(c(1,2),'null')))",
            "layout.pos.col=1",
            [0., 0., 1., 4.],
        ),
        (
            "grid.layout(1,3,widths=unit.c(unit(1,'inches'),unit(c(1,2),'null')))",
            "layout.pos.col=2",
            [1., 0., 5. / 3., 4.],
        ),
        (
            "grid.layout(1,3,widths=unit.c(unit(1,'inches'),unit(c(1,2),'null')))",
            "layout.pos.col=3",
            [8. / 3., 0., 10. / 3., 4.],
        ),
        (
            "grid.layout(1,2,widths=unit(c(1,2),'null'))",
            "layout.pos.col=2",
            [2., 0., 4., 4.],
        ),
        (
            "grid.layout(1,2,widths=unit(c(1,1),'inches'),just=c(.25,.75))",
            "layout.pos.col=1",
            [1., 0., 1., 4.],
        ),
        (
            "grid.layout(2,3,widths=unit(c(1,2,1),'null'))",
            "layout.pos.row=1:2,layout.pos.col=1:2",
            [0., 0., 4.5, 4.],
        ),
        (
            "grid.layout(1,2,widths=unit(c(1,1),'inches'))",
            "layout.pos.col=1",
            [2., 0., 1., 4.],
        ),
        (
            "grid.layout(1,2,widths=unit(c(5,7),'inches'))",
            "layout.pos.col=2",
            [2., 0., 7., 4.],
        ),
        (
            "grid.layout(1,2,widths=unit(c(1,1),'inches'),heights=unit(1,'inches'),just=c('right','top'))",
            "layout.pos.col=1",
            [4., 3., 1., 1.],
        ),
    ];
    let mut session = RSession::new().unwrap();
    for (layout, position, [left, bottom, width, height]) in cases {
        let code = format!(
            "library(grid); grid.newpage(); pushViewport(viewport(layout={layout})); pushViewport(viewport({position})); grid.rect(gp=gpar(fill='red',col=NA)); popViewport(2)"
        );
        let png = session
            .render_with_dimensions(&code, 576, 384)
            .unwrap_or_else(|e| panic!("{code}: {e}"));
        let (w, p) = pixels(&png);
        let mut bounds = [usize::MAX, usize::MAX, 0, 0];
        for (i, pixel) in p.chunks_exact(4).enumerate() {
            if pixel[0] > 240 && pixel[1] < 15 && pixel[2] < 15 {
                let x = i % w;
                let y = i / w;
                bounds[0] = bounds[0].min(x);
                bounds[1] = bounds[1].min(y);
                bounds[2] = bounds[2].max(x + 1);
                bounds[3] = bounds[3].max(y + 1);
            }
        }
        let expected = [
            left * 96.,
            (4. - bottom - height) * 96.,
            (left + width) * 96.,
            (4. - bottom) * 96.,
        ];
        for i in 0..4 {
            let maximum = if i % 2 == 0 { 576. } else { 384. };
            assert!(
                (bounds[i] as f64 - expected[i].clamp(0., maximum)).abs() <= 1.,
                "{code}: edge {i}: {bounds:?} vs {expected:?}"
            );
        }
    }
}

#[test]
fn zero_and_negative_null_units_match_gnu_r_geometry() {
    let mut session = RSession::new().unwrap();
    for (weights, expected) in [("c(0,0)", "[1] 0"), ("c(-1,2)", "[1] -6")] {
        session.render_with_dimensions(&format!("library(grid); grid.newpage(); pushViewport(viewport(layout=grid.layout(1,2,widths=unit({weights},'null')))); pushViewport(viewport(layout.pos.col=1)); measured<-convertWidth(unit(1,'npc'),'inches',valueOnly=TRUE); grid.rect(); popViewport(2)"),576,384).unwrap();
        assert_eq!(session.eval("measured").unwrap(), expected);
    }
}
