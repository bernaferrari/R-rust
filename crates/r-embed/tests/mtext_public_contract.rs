use r_embed::{RSession, RuntimePathPolicy};
use r_graphics_engine::{
    Color, DrawOperation, DrawTarget, FontFace, Path, PlotParameters, Point, Scene, TextMetrics,
};

// Reproduce the independently captured PDF device's Helvetica metrics. This
// tests GNU placement in unadjusted device coordinates without substituting
// the portable device's different font or normalizing away margin differences.
struct GnuPdfMetrics(Scene);

impl DrawTarget for GnuPdfMetrics {
    fn dimensions(&self) -> (u32, u32) {
        self.0.dimensions()
    }
    fn clear(&mut self, color: Color) {
        self.0.clear(color);
    }
    fn set_clip(&mut self, rect: Option<[f32; 4]>) {
        self.0.set_clip(rect);
    }
    fn draw_path(&mut self, path: &Path) {
        self.0.draw_path(path);
    }
    fn draw_text(&mut self, text: &str, position: Point, params: &PlotParameters) {
        self.0.draw_text(text, position, params);
    }
    fn measure_text(&self, text: &str, params: &PlotParameters) -> TextMetrics {
        assert_eq!(text, "label");
        assert_eq!(params.font_face, FontFace::Plain);
        TextMetrics {
            width: 25.344 * params.font_size / 12.,
            ascent: 8.616 * params.font_size / 12.,
            descent: 0.,
        }
    }
    fn measure_math_text(&self, text: &str, params: &PlotParameters) -> TextMetrics {
        assert_eq!(text, "M");
        TextMetrics {
            width: 9.996 * params.font_size / 12.,
            ascent: 8.616 * params.font_size / 12.,
            descent: 0.,
        }
    }
}

#[test]
fn margin_text_device_coordinates_match_all_gnu_sides_rotations_and_outer_margins() {
    let oracle = include_str!("../../rmath/src/mainutils/portable_plot/mtext-oracle-trace.tsv");
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        let mut target = GnuPdfMetrics(Scene::new(504, 504));
        session
            .render_to(
                "par(mar=c(5.1,4.1,4.1,2.1),oma=rep(2,4));plot.new();plot.window(c(0,1),c(0,1),xaxs='i',yaxs='i');for (outer in c(FALSE,TRUE)) for (side in 1:4) for (las in 0:3) graphics::mtext('label',side=side,las=las,line=.5,outer=outer)",
                &mut target,
            )
            .unwrap();
        let actual: Vec<_> = target
            .0
            .operations()
            .iter()
            .filter_map(|op| match op {
                DrawOperation::Text {
                    text,
                    position,
                    params,
                } => Some((text, position, params)),
                _ => None,
            })
            .collect();
        assert_eq!(actual.len(), 32);
        for outer in 0..=1 {
            for side in 1..=4 {
                for las in 0..=3 {
                    let case = format!("side{side}-las{las}-outer{outer}");
                    let row = oracle
                        .lines()
                        .find(|line| line.starts_with(&format!("{case}\t")))
                        .unwrap();
                    let columns: Vec<_> = row.split('\t').collect();
                    let expected_x: f32 = columns[2].parse().unwrap();
                    let expected_y: f32 = 504. - columns[3].parse::<f32>().unwrap();
                    let expected_angle: f32 = columns[4].parse().unwrap();
                    let (text, position, params) = actual[outer * 16 + (side - 1) * 4 + las];
                    assert_eq!(text, "label");
                    assert!(
                        (position.x - expected_x).abs() < 0.001,
                        "{case} portable={portable}: x={} expected={expected_x}",
                        position.x
                    );
                    assert!(
                        (position.y - expected_y).abs() < 0.001,
                        "{case} portable={portable}: y={} expected={expected_y}",
                        position.y
                    );
                    assert_eq!(params.text_angle, expected_angle, "{case}");
                }
            }
        }
        session.close();
    }
}

#[test]
fn margin_units_capture_base_cex_without_expanding_margin_text_font() {
    let oracle =
        include_str!("../../rmath/src/mainutils/portable_plot/mtext-margin-units-oracle-trace.tsv");
    for portable in [false, true] {
        let mut session = if portable {
            RSession::new_with_path_policy(RuntimePathPolicy::new(Vec::new(), std::env::temp_dir()))
        } else {
            RSession::new()
        }
        .unwrap();
        for row in oracle.lines().skip(1) {
            let columns: Vec<_> = row.split('\t').collect();
            let cex = columns[0];
            let mut target = GnuPdfMetrics(Scene::new(504, 504));
            session.render_to(&format!("par(cex={cex},font=1,mar=c(5.1,4.1,4.1,2.1),oma=rep(2,4));plot.new();plot.window(c(0,1),c(0,1),xaxs='i',yaxs='i');mtext('label',side=1,line=.5,at=0,adj=0,padj=0,cex=1)"),&mut target).unwrap();
            let labels: Vec<_> = target
                .0
                .operations()
                .iter()
                .filter_map(|op| match op {
                    DrawOperation::Text {
                        position, params, ..
                    } => Some((position, params)),
                    _ => None,
                })
                .collect();
            assert_eq!(labels.len(), 1);
            let expected_x: f32 = columns[2].parse().unwrap();
            let expected_y = 504. - columns[3].parse::<f32>().unwrap();
            assert!(
                (labels[0].0.x - expected_x).abs() < 0.001,
                "cex={cex}: x={} expected={expected_x}",
                labels[0].0.x
            );
            assert!(
                (labels[0].0.y - expected_y).abs() < 0.001,
                "cex={cex}: y={} expected={expected_y}",
                labels[0].0.y
            );
            assert_eq!(labels[0].1.font_size, 12.);
        }
        session.close();
    }
}

#[test]
fn margin_text_public_arguments_and_drawing_match_gnu_under_both_policies() {
    for portable in [false, true] {
        for generation in 0..2 {
            let mut session = if portable {
                RSession::new_with_path_policy(RuntimePathPolicy::new(
                    Vec::new(),
                    std::env::temp_dir(),
                ))
            } else {
                RSession::new()
            }
            .unwrap();
            let result = session
                .eval_interactive(include_str!("fixtures/mtext-public-contract.R"), 504, 504)
                .unwrap();
            assert_eq!(
                result.output,
                include_str!("fixtures/mtext-public-contract.out"),
                "portable={portable}, generation={generation}"
            );
            assert!(
                result.png.is_some(),
                "margin labels must issue real drawing"
            );
            let scene = session
                .record_scene(
                    "par(cex=2,col='green',font=2);plot.new();plot.window(c(0,1),c(0,1));graphics::mtext(c(NA_character_,'present'),side=1,cex=NA);mtext('',side=2)",
                    504,
                    504,
                )
                .unwrap();
            let labels: Vec<_> = scene
                .operations()
                .iter()
                .filter_map(|op| match op {
                    DrawOperation::Text { text, params, .. } => Some((text.as_str(), params)),
                    _ => None,
                })
                .collect();
            assert_eq!(
                labels.len(),
                1,
                "missing and empty text must not draw glyphs"
            );
            assert_eq!(labels[0].0, "present");
            assert_eq!(labels[0].1.font_size, 12., "mtext cex is absolute");
            assert_eq!(labels[0].1.font_face, FontFace::Bold);
            assert_eq!(labels[0].1.text_color.g, 255);
            assert_eq!(session.eval("1 + 1").unwrap(), "[1] 2\n");
            session.close();
        }
    }
}
