use super::{
    Color, DashPattern, DrawTarget, Path, PathCommand, PlotParameters, Point, RasterImage, Stroke,
};
use serde::{Deserialize, Serialize};

const MAX_SCENE_DIMENSION: u32 = u16::MAX as u32;

/// An owned graphics operation suitable for recording and replay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DrawOperation {
    Clear(Color),
    Clip(Option<[f32; 4]>),
    Path(Path),
    Text {
        text: String,
        position: Point,
        params: PlotParameters,
    },
    DrawImage {
        image: RasterImage,
        transform: [f64; 6],
        interpolate: bool,
    },
}

/// An owned, backend-neutral sequence of drawing operations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DisplayListWire")]
pub struct DisplayList {
    dimensions: (u32, u32),
    operations: Vec<DrawOperation>,
}

/// A scene is an owned display list.
pub type Scene = DisplayList;

#[derive(Deserialize)]
struct DisplayListWire {
    dimensions: (u32, u32),
    operations: Vec<DrawOperation>,
}

impl TryFrom<DisplayListWire> for DisplayList {
    type Error = &'static str;

    fn try_from(value: DisplayListWire) -> Result<Self, Self::Error> {
        let scene = Self {
            dimensions: value.dimensions,
            operations: value.operations,
        };
        scene.validate()?;
        Ok(scene)
    }
}

impl DisplayList {
    /// Create an empty recording for a device of `width` by `height` pixels.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            dimensions: (width, height),
            operations: Vec::new(),
        }
    }

    /// Create an empty recording with the dimensions reported by a target.
    pub fn for_target(target: &dyn DrawTarget) -> Self {
        let (width, height) = target.dimensions();
        Self::new(width, height)
    }

    /// Create a recording from already-owned commands.
    pub fn from_operations(dimensions: (u32, u32), operations: Vec<DrawOperation>) -> Self {
        Self {
            dimensions,
            operations,
        }
    }

    /// Dimensions of the device space in which this list was recorded.
    pub fn dimensions(&self) -> (u32, u32) {
        self.dimensions
    }

    /// Borrow the owned command stream.
    pub fn operations(&self) -> &[DrawOperation] {
        &self.operations
    }

    /// Consume this list and return its owned command stream.
    pub fn into_operations(self) -> Vec<DrawOperation> {
        self.operations
    }

    /// Append an already-owned operation.
    pub fn push(&mut self, operation: DrawOperation) {
        self.operations.push(operation);
    }

    /// Discard all commands recorded for the current page.
    pub fn reset(&mut self) {
        self.operations.clear();
    }

    /// Replay in the original device coordinate space.
    pub fn replay(&self, target: &mut dyn DrawTarget) {
        replay_operations(&self.operations, target, 1.0, 1.0)
    }

    /// Replay scaled to the target's reported dimensions.
    pub fn replay_scaled(&self, target: &mut dyn DrawTarget) {
        let (width, height) = target.dimensions();
        self.replay_scaled_to(target, width, height);
    }

    /// Replay scaled to an explicitly supplied device size.
    pub fn replay_scaled_to(&self, target: &mut dyn DrawTarget, width: u32, height: u32) {
        let scale_x = scale_axis(width, self.dimensions.0);
        let scale_y = scale_axis(height, self.dimensions.1);
        replay_operations(&self.operations, target, scale_x, scale_y);
    }

    /// Encode the owned scene as UTF-8 JSON.
    pub fn encode(&self) -> Result<Vec<u8>, serde_json::Error> {
        self.validate().map_err(validation_error)?;
        serde_json::to_vec(self)
    }

    /// Decode a scene previously returned by [`DisplayList::encode`].
    pub fn decode(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        let scene: Self = serde_json::from_slice(bytes)?;
        scene.validate().map_err(validation_error)?;
        Ok(scene)
    }

    /// Encode the scene as JSON text.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        self.validate().map_err(validation_error)?;
        serde_json::to_string(self)
    }

    /// Decode a scene from JSON text.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        Self::decode(json.as_bytes())
    }

    fn validate(&self) -> Result<(), &'static str> {
        let (width, height) = self.dimensions;
        if width == 0 || height == 0 {
            return Err("scene dimensions must be nonzero");
        }
        if width > MAX_SCENE_DIMENSION || height > MAX_SCENE_DIMENSION {
            return Err("scene dimensions are too large");
        }
        for operation in &self.operations {
            match operation {
                DrawOperation::Clear(_) => {}
                DrawOperation::Clip(rect) => validate_clip(*rect)?,
                DrawOperation::Path(path) => validate_path(path)?,
                DrawOperation::Text {
                    position, params, ..
                } => {
                    validate_point(*position)?;
                    validate_parameters(params)?;
                }
                DrawOperation::DrawImage {
                    image, transform, ..
                } => {
                    image
                        .validate()
                        .map_err(|_| "scene contains malformed raster image")?;
                    if transform.iter().any(|value| !value.is_finite()) {
                        return Err("scene contains a nonfinite image transform");
                    }
                }
            }
        }
        Ok(())
    }
}

impl Default for DisplayList {
    fn default() -> Self {
        Self::new(640, 480)
    }
}

impl DrawTarget for DisplayList {
    fn dimensions(&self) -> (u32, u32) {
        self.dimensions
    }

    fn clear(&mut self, background: Color) {
        self.reset();
        self.push(DrawOperation::Clear(background));
    }

    fn set_clip(&mut self, rect: Option<[f32; 4]>) {
        self.push(DrawOperation::Clip(rect));
    }

    fn draw_path(&mut self, path: &Path) {
        self.push(DrawOperation::Path(path.clone()));
    }

    fn draw_text(&mut self, text: &str, position: Point, params: &PlotParameters) {
        self.push(DrawOperation::Text {
            text: text.to_owned(),
            position,
            params: params.clone(),
        });
    }

    fn draw_image(&mut self, image: &RasterImage, transform: [f64; 6], interpolate: bool) {
        self.push(DrawOperation::DrawImage {
            image: image.clone(),
            transform,
            interpolate,
        });
    }
}

fn validation_error(message: &'static str) -> serde_json::Error {
    serde_json::Error::io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        message,
    ))
}

fn finite(value: f32) -> bool {
    value.is_finite()
}

fn validate_point(point: Point) -> Result<(), &'static str> {
    if finite(point.x) && finite(point.y) {
        Ok(())
    } else {
        Err("scene contains nonfinite coordinates")
    }
}

fn validate_clip(rect: Option<[f32; 4]>) -> Result<(), &'static str> {
    let Some([left, top, right, bottom]) = rect else {
        return Ok(());
    };
    if ![left, top, right, bottom].into_iter().all(finite) {
        return Err("scene contains a nonfinite clip rectangle");
    }
    if left > right || top > bottom {
        return Err("scene contains an inverted clip rectangle");
    }
    Ok(())
}

fn validate_path(path: &Path) -> Result<(), &'static str> {
    for command in &path.commands {
        match *command {
            PathCommand::MoveTo(x, y) | PathCommand::LineTo(x, y) => {
                validate_point(Point { x, y })?;
            }
            PathCommand::QuadTo(x1, y1, x2, y2) => {
                validate_point(Point { x: x1, y: y1 })?;
                validate_point(Point { x: x2, y: y2 })?;
            }
            PathCommand::CubicTo(x1, y1, x2, y2, x3, y3) => {
                validate_point(Point { x: x1, y: y1 })?;
                validate_point(Point { x: x2, y: y2 })?;
                validate_point(Point { x: x3, y: y3 })?;
            }
            PathCommand::ArcTo { rx, ry, x, y } => {
                validate_point(Point { x, y })?;
                if !finite(rx) || !finite(ry) {
                    return Err("scene contains nonfinite arc radii");
                }
            }
            PathCommand::Close => {}
        }
    }
    validate_stroke(&path.stroke)
}

fn validate_stroke(stroke: &Stroke) -> Result<(), &'static str> {
    if !finite(stroke.width) || !finite(stroke.miter_limit) {
        return Err("scene contains nonfinite stroke style");
    }
    if let Some(DashPattern { intervals, offset }) = &stroke.dash_pattern
        && (!finite(*offset) || intervals.iter().any(|interval| !finite(*interval)))
    {
        return Err("scene contains nonfinite dash style");
    }
    Ok(())
}

fn validate_parameters(params: &PlotParameters) -> Result<(), &'static str> {
    if finite(params.font_size) && finite(params.dpi) && finite(params.text_angle) {
        Ok(())
    } else {
        Err("scene contains nonfinite text style")
    }
}

fn scale_axis(destination: u32, source: u32) -> f32 {
    if source == 0 {
        1.0
    } else {
        destination as f32 / source as f32
    }
}

fn scalar_scale(scale_x: f32, scale_y: f32) -> f32 {
    (scale_x.abs() * scale_y.abs()).sqrt()
}

fn replay_operations(
    operations: &[DrawOperation],
    target: &mut dyn DrawTarget,
    scale_x: f32,
    scale_y: f32,
) {
    let scalar = scalar_scale(scale_x, scale_y);
    for operation in operations {
        match operation {
            DrawOperation::Clear(color) => target.clear(*color),
            DrawOperation::Clip(rect) => target.set_clip(rect.map(|[left, top, right, bottom]| {
                [
                    left * scale_x,
                    top * scale_y,
                    right * scale_x,
                    bottom * scale_y,
                ]
            })),
            DrawOperation::Path(path) => {
                if scale_x == 1.0 && scale_y == 1.0 {
                    target.draw_path(path);
                } else {
                    let scaled = scale_path(path, scale_x, scale_y, scalar);
                    target.draw_path(&scaled);
                }
            }
            DrawOperation::Text {
                text,
                position,
                params,
            } => {
                if scale_x == 1.0 && scale_y == 1.0 {
                    target.draw_text(text, *position, params);
                } else {
                    let mut scaled_params = params.clone();
                    scaled_params.font_size *= scalar;
                    target.draw_text(
                        text,
                        Point {
                            x: position.x * scale_x,
                            y: position.y * scale_y,
                        },
                        &scaled_params,
                    );
                }
            }
            DrawOperation::DrawImage {
                image,
                transform,
                interpolate,
            } => {
                let transform = [
                    transform[0] * scale_x as f64,
                    transform[1] * scale_y as f64,
                    transform[2] * scale_x as f64,
                    transform[3] * scale_y as f64,
                    transform[4] * scale_x as f64,
                    transform[5] * scale_y as f64,
                ];
                target.draw_image(image, transform, *interpolate);
            }
        }
    }
}

fn scale_path(path: &Path, scale_x: f32, scale_y: f32, scalar: f32) -> Path {
    let commands = path
        .commands
        .iter()
        .map(|command| match *command {
            PathCommand::MoveTo(x, y) => PathCommand::MoveTo(x * scale_x, y * scale_y),
            PathCommand::LineTo(x, y) => PathCommand::LineTo(x * scale_x, y * scale_y),
            PathCommand::QuadTo(x1, y1, x2, y2) => {
                PathCommand::QuadTo(x1 * scale_x, y1 * scale_y, x2 * scale_x, y2 * scale_y)
            }
            PathCommand::CubicTo(x1, y1, x2, y2, x3, y3) => PathCommand::CubicTo(
                x1 * scale_x,
                y1 * scale_y,
                x2 * scale_x,
                y2 * scale_y,
                x3 * scale_x,
                y3 * scale_y,
            ),
            PathCommand::ArcTo { rx, ry, x, y } => PathCommand::ArcTo {
                rx: rx * scale_x.abs(),
                ry: ry * scale_y.abs(),
                x: x * scale_x,
                y: y * scale_y,
            },
            PathCommand::Close => PathCommand::Close,
        })
        .collect();

    let stroke = Stroke {
        width: path.stroke.width * scalar,
        color: path.stroke.color,
        cap: path.stroke.cap,
        join: path.stroke.join,
        miter_limit: path.stroke.miter_limit,
        dash_pattern: path.stroke.dash_pattern.as_ref().map(|dash| DashPattern {
            intervals: dash
                .intervals
                .iter()
                .map(|interval| interval * scalar)
                .collect(),
            offset: dash.offset * scalar,
        }),
    };
    Path {
        commands,
        fill: path.fill,
        stroke,
        anti_alias: path.anti_alias,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default, PartialEq)]
    struct Target {
        dimensions: (u32, u32),
        operations: Vec<DrawOperation>,
    }

    impl Target {
        fn new(width: u32, height: u32) -> Self {
            Self {
                dimensions: (width, height),
                ..Self::default()
            }
        }
    }

    impl DrawTarget for Target {
        fn dimensions(&self) -> (u32, u32) {
            self.dimensions
        }

        fn clear(&mut self, background: Color) {
            self.operations.push(DrawOperation::Clear(background));
        }

        fn set_clip(&mut self, rect: Option<[f32; 4]>) {
            self.operations.push(DrawOperation::Clip(rect));
        }

        fn draw_path(&mut self, path: &Path) {
            self.operations.push(DrawOperation::Path(path.clone()));
        }

        fn draw_text(&mut self, text: &str, position: Point, params: &PlotParameters) {
            self.operations.push(DrawOperation::Text {
                text: text.to_owned(),
                position,
                params: params.clone(),
            });
        }

        fn draw_image(&mut self, image: &RasterImage, transform: [f64; 6], interpolate: bool) {
            self.operations.push(DrawOperation::DrawImage {
                image: image.clone(),
                transform,
                interpolate,
            });
        }
    }

    #[derive(Default)]
    struct QuadTarget {
        paths: Vec<Path>,
    }

    impl DrawTarget for QuadTarget {
        fn clear(&mut self, _background: Color) {}

        fn draw_path(&mut self, path: &Path) {
            self.paths.push(path.clone());
        }

        fn draw_text(&mut self, _text: &str, _position: Point, _params: &PlotParameters) {}
    }

    fn sample_scene() -> DisplayList {
        let mut scene = DisplayList::new(100, 50);
        scene.clear(Color::WHITE);
        scene.set_clip(Some([2.0, 4.0, 80.0, 40.0]));
        scene.draw_path(&Path {
            commands: vec![
                PathCommand::MoveTo(1.0, 2.0),
                PathCommand::QuadTo(3.0, 4.0, 5.0, 6.0),
                PathCommand::CubicTo(7.0, 8.0, 9.0, 10.0, 11.0, 12.0),
                PathCommand::ArcTo {
                    rx: 2.0,
                    ry: 3.0,
                    x: 14.0,
                    y: 15.0,
                },
                PathCommand::Close,
            ],
            fill: Color::BLUE,
            stroke: Stroke {
                width: 2.0,
                color: Color::RED,
                dash_pattern: Some(DashPattern {
                    intervals: vec![1.0, 2.0],
                    offset: 3.0,
                }),
                ..Stroke::new(2.0, Color::RED)
            },
            anti_alias: false,
        });
        scene.draw_text(
            "owned",
            Point { x: 20.0, y: 21.0 },
            &PlotParameters {
                font_size: 10.0,
                text_color: Color::BLACK,
                dpi: 96.0,
                text_anchor: super::super::TextAnchor::Middle,
                text_angle: 15.0,
            },
        );
        scene.set_clip(None);
        scene
    }

    #[test]
    fn records_owned_operations_and_replays_in_order() {
        let scene = sample_scene();
        assert_eq!(scene.dimensions(), (100, 50));
        assert_eq!(scene.operations().len(), 5);

        let mut target = Target::new(100, 50);
        scene.replay(&mut target);
        assert_eq!(target.operations, scene.operations);
    }

    #[test]
    fn clear_starts_a_fresh_page() {
        let mut scene = DisplayList::new(20, 20);
        scene.draw_path(&Path::rect(1.0, 1.0, 4.0, 4.0));
        scene.clear(Color::BLACK);

        assert_eq!(scene.operations(), &[DrawOperation::Clear(Color::BLACK)]);
    }

    #[test]
    fn json_roundtrip_preserves_owned_scene() {
        let scene = sample_scene();
        let encoded = scene.encode().expect("scene should encode");
        let decoded = DisplayList::decode(&encoded).expect("scene should decode");
        assert_eq!(decoded, scene);
    }

    #[test]
    fn decode_rejects_invalid_dimensions_and_clips() {
        assert!(DisplayList::decode(br#"{"dimensions":[0,50],"operations":[]}"#).is_err());
        assert!(DisplayList::decode(br#"{"dimensions":[65536,50],"operations":[]}"#).is_err());
        assert!(
            DisplayList::decode(
                br#"{"dimensions":[100,50],"operations":[{"Clip":[[10.0,4.0,2.0,8.0]]}]}"#
            )
            .is_err()
        );
    }

    #[test]
    fn raster_image_validates_exact_rgba_storage() {
        assert!(RasterImage::new(1, 1, vec![0; 3]).is_err());
        assert!(RasterImage::new(1, 1, vec![0; 4]).is_ok());
        assert!(
            serde_json::from_str::<RasterImage>(r#"{"width":0,"height":1,"pixels":[]}"#).is_err()
        );
        assert!(
            serde_json::from_str::<RasterImage>(r#"{"width":65536,"height":1,"pixels":[]}"#)
                .is_err()
        );
        assert!(DisplayList::decode(
            br#"{"dimensions":[100,50],"operations":[{"DrawImage":{"image":{"width":1,"height":1,"pixels":[0,0,0]},"transform":[1.0,0.0,0.0,1.0,0.0,0.0],"interpolate":false}}]}"#
        )
        .is_err());
    }

    #[test]
    fn records_and_scales_image_affine_transform() {
        let image = RasterImage::new(2, 1, vec![255, 0, 0, 255, 0, 255, 0, 255]).unwrap();
        let mut scene = DisplayList::new(100, 50);
        scene.draw_image(&image, [1.0, 0.25, 0.5, 1.0, 10.0, 20.0], true);

        let encoded = scene.encode().unwrap();
        assert_eq!(DisplayList::decode(&encoded).unwrap(), scene);

        let mut target = Target::new(200, 100);
        scene.replay_scaled(&mut target);
        assert_eq!(
            target.operations,
            vec![DrawOperation::DrawImage {
                image,
                transform: [2.0, 0.5, 1.0, 2.0, 20.0, 40.0],
                interpolate: true,
            }]
        );
    }

    #[test]
    fn default_image_replay_rasterizes_transformed_pixel_quads() {
        let image = RasterImage::new(2, 1, vec![255, 0, 0, 255, 0, 255, 0, 255]).unwrap();
        let mut target = QuadTarget::default();
        target.draw_image(&image, [2.0, 0.0, 0.0, 3.0, 4.0, 5.0], false);

        assert_eq!(target.paths.len(), 2);
        assert_eq!(
            target.paths[0].commands,
            vec![
                PathCommand::MoveTo(4.0, 5.0),
                PathCommand::LineTo(6.0, 5.0),
                PathCommand::LineTo(6.0, 8.0),
                PathCommand::LineTo(4.0, 8.0),
                PathCommand::Close,
            ]
        );
        assert_eq!(target.paths[0].fill, Color::RED);
        assert!(!target.paths[0].anti_alias);
        assert_eq!(
            target.paths[1].fill,
            Color {
                g: 255,
                ..Color::BLACK
            }
        );
    }

    #[test]
    fn scaled_replay_scales_geometry_strokes_fonts_and_clips() {
        let scene = sample_scene();
        let mut target = Target::new(200, 100);
        scene.replay_scaled(&mut target);

        assert_eq!(target.operations[0], DrawOperation::Clear(Color::WHITE));
        assert_eq!(
            target.operations[1],
            DrawOperation::Clip(Some([4.0, 8.0, 160.0, 80.0]))
        );
        let DrawOperation::Path(path) = &target.operations[2] else {
            panic!("expected path operation")
        };
        assert_eq!(
            path.commands,
            vec![
                PathCommand::MoveTo(2.0, 4.0),
                PathCommand::QuadTo(6.0, 8.0, 10.0, 12.0),
                PathCommand::CubicTo(14.0, 16.0, 18.0, 20.0, 22.0, 24.0),
                PathCommand::ArcTo {
                    rx: 4.0,
                    ry: 6.0,
                    x: 28.0,
                    y: 30.0,
                },
                PathCommand::Close,
            ]
        );
        assert_eq!(path.stroke.width, 4.0);
        assert_eq!(
            path.stroke.dash_pattern,
            Some(DashPattern {
                intervals: vec![2.0, 4.0],
                offset: 6.0,
            })
        );
        let DrawOperation::Text {
            text,
            position,
            params,
        } = &target.operations[3]
        else {
            panic!("expected text operation")
        };
        assert_eq!(text, "owned");
        assert_eq!(*position, Point { x: 40.0, y: 42.0 });
        assert_eq!(params.font_size, 20.0);
        assert_eq!(params.dpi, 96.0);
        assert_eq!(target.operations[4], DrawOperation::Clip(None));
    }
}
