use r_embed::RSession;

#[test]
fn interactive_grid_drawing_does_not_print_invisible_results() {
    let mut session = RSession::new().unwrap();
    let result = session.eval_interactive("library(grid); grid.newpage(); pushViewport(viewport()); grid.draw(rectGrob()); grid.rect(); grid.circle(); grid.lines(); grid.segments(); grid.polygon(); grid.points(x=.2,y=.2); grid.text('R'); popViewport()", 240, 160).unwrap();
    assert_eq!(result.output, "");
    assert!(result.png.is_some());
    let explicit = session.eval_interactive("print(NULL)", 240, 160).unwrap();
    assert_eq!(explicit.output.trim(), "NULL");
}
