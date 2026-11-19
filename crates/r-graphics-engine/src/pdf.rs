use crate::font::normalized_size;
use crate::{
    Color, DrawOperation, FontFace, LineCap, LineJoin, Path, PathCommand, PlotParameters, Point,
    Scene, Stroke, TextAnchor, default_font_book,
};
use flate2::{Compression, write::ZlibEncoder};
use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Write};

const FONT_OBJECTS: usize = 8;
const PAGE_OBJECTS: usize = 3;
const FIRST_RESOURCE: usize = 4;

/// Encode ordered scenes as a PDF 1.4 document, with each scene unit equal to
/// one PDF point. Geometry and labels stay as paths and searchable text.
///
/// This validates scene data before writing. Raster images and the legacy
/// arc operation are currently unsupported. Use a bounded sink to limit output
/// and publish the result only on success, since I/O errors can occur mid-write.
pub fn write_pdf<W: Write>(pages: &[Scene], output: &mut W) -> io::Result<()> {
    write_pdf_with_compression(pages, output, true)
}

/// Control page stream compression. Embedded font streams remain compressed.
pub fn write_pdf_with_compression<W: Write>(
    pages: &[Scene],
    output: &mut W,
    compress_pages: bool,
) -> io::Result<()> {
    let resources = Resources::collect(pages)?;
    let first_page = FIRST_RESOURCE + resources.font_ids.iter().flatten().count() * FONT_OBJECTS;
    let object_count = pages
        .len()
        .checked_mul(PAGE_OBJECTS)
        .and_then(|count| count.checked_add(first_page - 1))
        .ok_or_else(|| invalid("too many PDF pages"))?;
    let mut pdf = Pdf::new(output, object_count)?;
    pdf.output.write_all(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")?;
    pdf.object()?;
    pdf.output
        .write_all(b"<< /Type /Catalog /Pages 2 0 R >>\nendobj\n")?;
    pdf.object()?;
    write!(pdf.output, "<< /Type /Pages /Count {} /Kids [", pages.len())?;
    for index in 0..pages.len() {
        write!(pdf.output, " {} 0 R", first_page + index * PAGE_OBJECTS)?;
    }
    pdf.output.write_all(b" ] >>\nendobj\n")?;
    resources.write(&mut pdf)?;
    for page in pages {
        let id = pdf.object()?;
        let (width, height) = page.dimensions();
        writeln!(
            pdf.output,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Resources 3 0 R /Contents {} 0 R >>\nendobj",
            id + 1
        )?;
        if compress_pages {
            pdf.compressed_stream(format_args!(""), |out| write_page(out, page, &resources))?;
        } else {
            pdf.stream(format_args!(""), |out| write_page(out, page, &resources))?;
        }
    }
    pdf.finish()
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn unsupported(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, message)
}

struct PositionWriter<W> {
    sink: W,
    position: u64,
}

impl<W: Write> Write for PositionWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let limit = self.position + bytes.len() as u64;
        if limit > 9_999_999_999 {
            return Err(invalid("PDF exceeds the cross-reference offset limit"));
        }
        let written = self.sink.write(bytes)?;
        self.position += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.sink.flush()
    }
}

struct Pdf<W> {
    output: PositionWriter<W>,
    offsets: Vec<u64>,
}

impl<W: Write> Pdf<W> {
    fn new(sink: W, object_count: usize) -> io::Result<Self> {
        let mut offsets = Vec::new();
        offsets
            .try_reserve_exact(object_count)
            .map_err(io::Error::other)?;
        Ok(Self {
            output: PositionWriter { sink, position: 0 },
            offsets,
        })
    }

    fn object(&mut self) -> io::Result<usize> {
        self.offsets.push(self.output.position);
        let id = self.offsets.len();
        writeln!(self.output, "{id} 0 obj")?;
        Ok(id)
    }

    fn stream(
        &mut self,
        parameters: fmt::Arguments<'_>,
        write_body: impl FnOnce(&mut PositionWriter<W>) -> io::Result<()>,
    ) -> io::Result<()> {
        let id = self.object()?;
        writeln!(
            self.output,
            "<< /Length {} 0 R {parameters} >>\nstream",
            id + 1
        )?;
        let start = self.output.position;
        write_body(&mut self.output)?;
        let length = self.output.position - start;
        self.output.write_all(b"\nendstream\nendobj\n")?;
        self.object()?;
        writeln!(self.output, "{length}\nendobj")
    }

    fn compressed_stream(
        &mut self,
        parameters: fmt::Arguments<'_>,
        write_body: impl FnOnce(&mut ZlibEncoder<&mut PositionWriter<W>>) -> io::Result<()>,
    ) -> io::Result<()> {
        self.stream(format_args!("/Filter /FlateDecode {parameters}"), |out| {
            let mut encoder = ZlibEncoder::new(out, Compression::default());
            write_body(&mut encoder)?;
            encoder.finish()?;
            Ok(())
        })
    }

    fn finish(mut self) -> io::Result<()> {
        let start = self.output.position;
        let size = self.offsets.len() + 1;
        writeln!(self.output, "xref\n0 {size}\n0000000000 65535 f ")?;
        for offset in self.offsets {
            writeln!(self.output, "{offset:010} 00000 n ")?;
        }
        writeln!(
            self.output,
            "trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF"
        )?;
        self.output.flush()
    }
}

// Use decimal PDF number syntax and keep ordinary geometry compact.
struct Number(f64);

impl fmt::Display for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self.0;
        let rounded = (value * 1_000_000.).round() / 1_000_000.;
        fmt::Display::fmt(
            &if rounded == 0. && value != 0. {
                value
            } else {
                rounded
            },
            f,
        )
    }
}

fn face_slot(face: FontFace) -> usize {
    match face {
        FontFace::Plain => 0,
        FontFace::Bold => 1,
        FontFace::Italic => 2,
        FontFace::BoldItalic => 3,
    }
}

const FACES: [FontFace; 4] = [
    FontFace::Plain,
    FontFace::Bold,
    FontFace::Italic,
    FontFace::BoldItalic,
];

#[derive(Default)]
struct Glyph {
    code: u16,
    index: u16,
    width: f64,
}

type Glyphs = BTreeMap<char, Glyph>;

struct Resources {
    glyphs: [Glyphs; 4],
    font_ids: [Option<usize>; 4],
    fill_alpha: [bool; 256],
    stroke_alpha: [bool; 256],
}

impl Resources {
    fn collect(pages: &[Scene]) -> io::Result<Self> {
        let mut out = Self {
            glyphs: Default::default(),
            font_ids: [None; 4],
            fill_alpha: [false; 256],
            stroke_alpha: [false; 256],
        };
        for page in pages {
            page.validate().map_err(invalid)?;
            for (index, operation) in page.operations().iter().enumerate() {
                match operation {
                    DrawOperation::Clear(color) if index == 0 => {
                        out.fill_alpha[color.a as usize] = true
                    }
                    DrawOperation::Clear(_) => {
                        return Err(unsupported("PDF cannot clear an existing page"));
                    }
                    DrawOperation::Clip(_) => (),
                    DrawOperation::Path(path) => {
                        validate_path(path)?;
                        out.fill_alpha[path.fill.a as usize] = true;
                        if visible_stroke(&path.stroke) {
                            out.stroke_alpha[path.stroke.color.a as usize] = true;
                        }
                    }
                    DrawOperation::Text { text, params, .. } if params.text_color.a != 0 => {
                        out.fill_alpha[params.text_color.a as usize] = true;
                        out.collect_glyphs(text, params.font_face)?;
                    }
                    DrawOperation::Text { .. } => (),
                    DrawOperation::DrawImage { .. } => {
                        return Err(unsupported("PDF raster images are not supported"));
                    }
                }
            }
        }
        let mut next_font = FIRST_RESOURCE;
        for (glyphs, id) in out.glyphs.iter_mut().zip(&mut out.font_ids) {
            if glyphs.is_empty() {
                continue;
            }
            *id = Some(next_font);
            next_font += FONT_OBJECTS;
            for (index, glyph) in glyphs.values_mut().enumerate() {
                glyph.code = u16::try_from(index + 1)
                    .map_err(|_| invalid("too many distinct characters for a PDF font"))?;
            }
        }
        Ok(out)
    }

    fn collect_glyphs(&mut self, text: &str, face: FontFace) -> io::Result<()> {
        use std::collections::btree_map::Entry;
        let glyphs = &mut self.glyphs[face_slot(face)];
        for ch in text_chars(text) {
            if let Entry::Vacant(entry) = glyphs.entry(ch) {
                let font = default_font_book();
                let index = font.glyph_index_for_face(ch, face);
                if index == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        format!("PDF font has no glyph for U+{:04X}", u32::from(ch)),
                    ));
                }
                entry.insert(Glyph {
                    code: 0,
                    index,
                    width: f64::from(font.advance_width_for_face(ch, 1000., face)),
                });
            }
        }
        Ok(())
    }

    fn write<W: Write>(&self, pdf: &mut Pdf<W>) -> io::Result<()> {
        pdf.object()?;
        pdf.output.write_all(b"<< /Font <<")?;
        for (slot, id) in self.font_ids.iter().enumerate() {
            if let Some(id) = id {
                write!(pdf.output, " /T{slot} {id} 0 R")?;
            }
        }
        pdf.output.write_all(b" >> /ExtGState <<")?;
        for (prefix, key, alphas) in [
            ("F", "ca", &self.fill_alpha),
            ("S", "CA", &self.stroke_alpha),
        ] {
            for (alpha, used) in alphas.iter().enumerate().take(255).skip(1) {
                if *used {
                    write!(
                        pdf.output,
                        " /{prefix}{alpha} << /Type /ExtGState /{key} {} >>",
                        Number(alpha as f64 / 255.)
                    )?;
                }
            }
        }
        pdf.output.write_all(b" >> >>\nendobj\n")?;
        for ((face, glyphs), id) in FACES.into_iter().zip(&self.glyphs).zip(self.font_ids) {
            if let Some(id) = id {
                write_font(pdf, face, glyphs, id)?;
            }
        }
        Ok(())
    }
}

fn text_chars(text: &str) -> impl Iterator<Item = char> + '_ {
    text.chars().filter(|ch| !ch.is_control())
}

fn write_font<W: Write>(
    pdf: &mut Pdf<W>,
    face: FontFace,
    glyphs: &Glyphs,
    id: usize,
) -> io::Result<()> {
    // BBoxes are the bundled faces' head bounding boxes in PDF font units.
    let (name, bbox) = match face {
        FontFace::Plain => ("DejaVuSans", "-1022 -463 1794 1233"),
        FontFace::Bold => ("DejaVuSans-Bold", "-1069 -416 1976 1176"),
        FontFace::Italic => ("DejaVuSans-Oblique", "-1017 -351 1660 1069"),
        FontFace::BoldItalic => ("DejaVuSans-BoldOblique", "-1068 -386 2000 1122"),
    };
    pdf.object()?;
    writeln!(
        pdf.output,
        "<< /Type /Font /Subtype /Type0 /BaseFont /{name} /Encoding /Identity-H /DescendantFonts [{} 0 R] /ToUnicode {} 0 R >>\nendobj",
        id + 1,
        id + 6
    )?;
    pdf.object()?;
    write!(
        pdf.output,
        "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{name} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor {} 0 R /CIDToGIDMap {} 0 R /W [1 [",
        id + 2,
        id + 5
    )?;
    for glyph in glyphs.values() {
        write!(pdf.output, " {}", Number(glyph.width))?;
    }
    pdf.output.write_all(b" ]] >>\nendobj\n")?;

    let (flags, angle) = if face.is_italic() { (68, -11) } else { (4, 0) };
    let stem = if face.is_bold() { 120 } else { 80 };
    pdf.object()?;
    writeln!(
        pdf.output,
        "<< /Type /FontDescriptor /FontName /{name} /Flags {flags} /FontBBox [{bbox}] /ItalicAngle {angle} /Ascent 928 /Descent -236 /CapHeight 729 /StemV {stem} /FontFile2 {} 0 R >>\nendobj",
        id + 3
    )?;
    let bytes = default_font_book().bytes_for_face(face);
    pdf.compressed_stream(format_args!("/Length1 {}", bytes.len()), |out| {
        out.write_all(&bytes)
    })?;
    pdf.object()?;
    writeln!(
        pdf.output,
        "<< /Length {} >>\nstream",
        2 * (glyphs.len() + 1)
    )?;
    pdf.output.write_all(&0u16.to_be_bytes())?;
    for glyph in glyphs.values() {
        pdf.output.write_all(&glyph.index.to_be_bytes())?;
    }
    pdf.output.write_all(b"\nendstream\nendobj\n")?;
    pdf.stream(format_args!(""), |out| write_to_unicode(out, glyphs))
}

fn write_to_unicode(out: &mut impl Write, glyphs: &Glyphs) -> io::Result<()> {
    out.write_all(b"/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Rport-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n")?;
    let mut mapping = glyphs.iter();
    while mapping.len() != 0 {
        let count = mapping.len().min(100);
        writeln!(out, "{count} beginbfchar")?;
        for (&ch, glyph) in mapping.by_ref().take(count) {
            write!(out, "<{:04X}> <", glyph.code)?;
            for part in ch.encode_utf16(&mut [0; 2]) {
                write!(out, "{part:04X}")?;
            }
            out.write_all(b">\n")?;
        }
        out.write_all(b"endbfchar\n")?;
    }
    out.write_all(b"endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n")
}

fn visible_stroke(stroke: &Stroke) -> bool {
    stroke.width > 0. && stroke.color.a > 0
}

fn validate_path(path: &Path) -> io::Result<()> {
    let mut started = false;
    for command in &path.commands {
        match command {
            PathCommand::MoveTo(..) => started = true,
            PathCommand::ArcTo { .. } => {
                return Err(unsupported(
                    "PDF cannot render legacy arcs without sweep flags",
                ));
            }
            _ if !started => return Err(invalid("PDF path must begin with a move command")),
            _ => (),
        }
    }
    if visible_stroke(&path.stroke)
        && let Some(dash) = &path.stroke.dash_pattern
        && !dash.intervals.is_empty()
        && (dash.intervals.iter().any(|&interval| interval < 0.)
            || !dash.intervals.iter().any(|&interval| interval > 0.))
    {
        return Err(invalid("invalid PDF dash intervals"));
    }
    Ok(())
}

fn write_page(out: &mut impl Write, page: &Scene, resources: &Resources) -> io::Result<()> {
    let (width, height) = page.dimensions();
    let y = |value: f32| f64::from(height) - f64::from(value);
    out.write_all(b"q\n")?;
    for op in page.operations() {
        match op {
            DrawOperation::Clear(color) => {
                if color.a != 0 {
                    out.write_all(b"q\n")?;
                    write_color(out, *color, false)?;
                    writeln!(out, "0 0 {width} {height} re f\nQ")?;
                }
            }
            DrawOperation::Clip(rect) => {
                // A clip replaces the previous clip in the scene model.
                out.write_all(b"Q\nq\n")?;
                if let Some([left, top, right, bottom]) = *rect {
                    writeln!(
                        out,
                        "{} {} {} {} re W n",
                        Number(f64::from(left)),
                        Number(y(bottom)),
                        Number(f64::from(right) - f64::from(left)),
                        Number(f64::from(bottom) - f64::from(top))
                    )?;
                }
            }
            DrawOperation::Path(path) => write_path(out, path, f64::from(height))?,
            DrawOperation::Text {
                text,
                position,
                params,
            } => {
                write_text(out, text, *position, params, f64::from(height), resources)?;
            }
            DrawOperation::DrawImage { .. } => {
                return Err(unsupported("PDF raster images are not supported"));
            }
        }
    }
    out.write_all(b"Q\n")
}

fn write_color(out: &mut impl Write, color: Color, stroke: bool) -> io::Result<()> {
    let [r, g, b] =
        [color.r, color.g, color.b].map(|component| Number(f64::from(component) / 255.));
    let (operator, prefix) = if stroke { ("RG", 'S') } else { ("rg", 'F') };
    writeln!(out, "{r} {g} {b} {operator}")?;
    if color.a < 255 {
        writeln!(out, "/{prefix}{} gs", color.a)?;
    }
    Ok(())
}

fn write_stroke(out: &mut impl Write, stroke: &Stroke) -> io::Result<()> {
    write_color(out, stroke.color, true)?;
    let cap = match stroke.cap {
        LineCap::Butt => 0,
        LineCap::Round => 1,
        LineCap::Square => 2,
    };
    let join = match stroke.join {
        LineJoin::Miter => 0,
        LineJoin::Round => 1,
        LineJoin::Bevel => 2,
    };
    let width = Number(f64::from(stroke.width));
    let limit = Number(f64::from(stroke.miter_limit.max(1.)));
    writeln!(out, "{width} w {cap} J {join} j {limit} M")?;
    if let Some(dash) = &stroke.dash_pattern
        && !dash.intervals.is_empty()
    {
        let mut period = 0.;
        out.write_all(b"[")?;
        for &interval in &dash.intervals {
            let interval = f64::from(interval);
            write!(out, " {}", Number(interval))?;
            period += interval;
        }
        if !dash.intervals.len().is_multiple_of(2) {
            period *= 2.;
        }
        writeln!(
            out,
            " ] {} d",
            Number(f64::from(dash.offset).rem_euclid(period))
        )?;
    }
    Ok(())
}

fn write_path(out: &mut impl Write, path: &Path, height: f64) -> io::Result<()> {
    let fill = path.fill.a > 0;
    let stroke = visible_stroke(&path.stroke);
    let paint = match (fill, stroke) {
        (false, false) => return Ok(()),
        (false, true) => "S",
        (true, false) => "f",
        (true, true) => "B",
    };
    if path.commands.is_empty() {
        return Ok(());
    }
    out.write_all(b"q\n")?;
    if fill {
        write_color(out, path.fill, false)?;
    }
    if stroke {
        write_stroke(out, &path.stroke)?;
    }
    let x = |x: f32| Number(f64::from(x));
    let y = |y: f32| Number(height - f64::from(y));
    let mut cursor = (0., 0.);
    let mut origin = cursor;
    let mut closed = false;
    for command in &path.commands {
        if closed
            && matches!(
                command,
                PathCommand::LineTo(..) | PathCommand::QuadTo(..) | PathCommand::CubicTo(..)
            )
        {
            writeln!(out, "{} {} m", x(cursor.0), y(cursor.1))?;
            closed = false;
        }
        match *command {
            PathCommand::MoveTo(end_x, end_y) => {
                writeln!(out, "{} {} m", x(end_x), y(end_y))?;
                cursor = (end_x, end_y);
                origin = cursor;
                closed = false;
            }
            PathCommand::LineTo(end_x, end_y) => {
                writeln!(out, "{} {} l", x(end_x), y(end_y))?;
                cursor = (end_x, end_y);
            }
            PathCommand::CubicTo(x1, y1, x2, y2, end_x, end_y) => {
                writeln!(
                    out,
                    "{} {} {} {} {} {} c",
                    x(x1),
                    y(y1),
                    x(x2),
                    y(y2),
                    x(end_x),
                    y(end_y)
                )?;
                cursor = (end_x, end_y);
            }
            PathCommand::QuadTo(control_x, control_y, end_x, end_y) => {
                let control =
                    |point: f32, middle: f32| f64::from(point) / 3. + 2. * f64::from(middle) / 3.;
                let x1 = Number(control(cursor.0, control_x));
                let y1 = Number(height - control(cursor.1, control_y));
                let x2 = Number(control(end_x, control_x));
                let y2 = Number(height - control(end_y, control_y));
                writeln!(out, "{x1} {y1} {x2} {y2} {} {} c", x(end_x), y(end_y))?;
                cursor = (end_x, end_y);
            }
            PathCommand::Close => {
                if !closed {
                    out.write_all(b"h\n")?;
                }
                cursor = origin;
                closed = true;
            }
            PathCommand::ArcTo { .. } => {
                return Err(unsupported(
                    "PDF cannot render legacy arcs without sweep flags",
                ));
            }
        }
    }
    writeln!(out, "{paint}\nQ")
}

fn write_text(
    out: &mut impl Write,
    text: &str,
    position: Point,
    params: &PlotParameters,
    height: f64,
    resources: &Resources,
) -> io::Result<()> {
    if params.text_color.a == 0 || text_chars(text).next().is_none() {
        return Ok(());
    }
    let slot = face_slot(params.font_face);
    let glyphs = &resources.glyphs[slot];
    let size = f64::from(normalized_size(params.font_size));
    let width = text_chars(text).map(|ch| glyphs[&ch].width).sum::<f64>() * size / 1000.;
    let adjustment = match params.text_anchor {
        TextAnchor::Start => 0.,
        TextAnchor::Middle => -width / 2.,
        TextAnchor::End => -width,
    };
    let (sin, cos) = f64::from(params.text_angle).to_radians().sin_cos();
    let rotated = |value: f64| Number(if value.abs() < 1e-12 { 0. } else { value });
    let x = Number(f64::from(position.x) + adjustment * cos);
    let y = Number(height - f64::from(position.y) + adjustment * sin);
    out.write_all(b"q\n")?;
    write_color(out, params.text_color, false)?;
    writeln!(
        out,
        "BT /T{slot} {} Tf {} {} {} {} {x} {y} Tm",
        Number(size),
        rotated(cos),
        rotated(sin),
        rotated(-sin),
        rotated(cos)
    )?;
    out.write_all(b"<")?;
    for ch in text_chars(text) {
        write!(out, "{:04X}", glyphs[&ch].code)?;
    }
    out.write_all(b"> Tj ET\nQ\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DashPattern, DrawTarget, RasterImage};

    fn sample() -> Scene {
        let mut page = Scene::new(150, 100);
        page.clear(Color::WHITE);
        page.set_clip(Some([3., 5., 40., 85.]));
        page.draw_path(&Path {
            commands: vec![
                PathCommand::MoveTo(0., 0.),
                PathCommand::QuadTo(6., 12., 12., 0.),
                PathCommand::CubicTo(18., 6., 24., 6., 30., 0.),
                PathCommand::Close,
            ],
            fill: Color {
                a: 128,
                ..Color::BLUE
            },
            stroke: Stroke {
                cap: LineCap::Round,
                join: LineJoin::Bevel,
                dash_pattern: Some(DashPattern {
                    intervals: vec![2., 3.],
                    offset: -1.,
                }),
                ..Stroke::new(
                    2.,
                    Color {
                        a: 160,
                        ..Color::RED
                    },
                )
            },
            anti_alias: true,
        });
        page.set_clip(None);
        page.draw_text(
            "x / café − β",
            Point { x: 75., y: 85. },
            &PlotParameters {
                font_size: 12.,
                text_color: Color::BLACK,
                text_anchor: TextAnchor::Middle,
                ..PlotParameters::default()
            },
        );
        page.draw_text(
            "Results",
            Point { x: 80., y: 65. },
            &PlotParameters {
                font_size: 12.,
                font_face: FontFace::Bold,
                text_color: Color::BLACK,
                text_anchor: TextAnchor::End,
                text_angle: 90.,
                ..PlotParameters::default()
            },
        );
        page
    }

    fn encode(pages: &[Scene]) -> Vec<u8> {
        let mut out = Vec::new();
        write_pdf(pages, &mut out).unwrap();
        out
    }

    #[test]
    fn deterministic_multipage_pdf_has_valid_offsets_and_stream_lengths() {
        use std::io::Read;

        let pages = [sample(), Scene::new(50, 45)];
        let pdf = encode(&pages);
        assert_eq!(pdf, encode(&pages));
        assert!(pdf.len() < 1 << 20);
        assert!(pdf.starts_with(b"%PDF-1.4\n"));
        let tail = std::str::from_utf8(&pdf[pdf.len() - 80..]).unwrap();
        let xref_offset: usize = tail
            .split("startxref\n")
            .nth(1)
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let mut xref = std::str::from_utf8(&pdf[xref_offset..]).unwrap().lines();
        assert_eq!(xref.next(), Some("xref"));
        assert_eq!(xref.next(), Some("0 26"));
        assert_eq!(xref.next(), Some("0000000000 65535 f "));
        let mut offsets = Vec::new();
        for id in 1..26 {
            let offset: usize = xref.next().unwrap()[..10].parse().unwrap();
            assert!(pdf[offset..].starts_with(format!("{id} 0 obj\n").as_bytes()));
            offsets.push(offset);
        }
        let mut content = String::new();
        for (stream, length) in [(7, 8), (10, 11), (15, 16), (18, 19), (21, 22), (24, 25)] {
            let object = &pdf[offsets[stream - 1]..offsets[length - 1]];
            let start = object
                .windows(b"\nstream\n".len())
                .position(|body| body == b"\nstream\n")
                .unwrap()
                + b"\nstream\n".len();
            let end = offsets.get(length).copied().unwrap_or(xref_offset);
            let value: usize = std::str::from_utf8(&pdf[offsets[length - 1]..end])
                .unwrap()
                .lines()
                .nth(1)
                .unwrap()
                .parse()
                .unwrap();
            assert_eq!(&object[start + value..], b"\nendstream\nendobj\n");
            if stream == 21 {
                flate2::read::ZlibDecoder::new(&object[start..start + value])
                    .read_to_string(&mut content)
                    .unwrap();
            }
        }
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/Count 2 /Kids [ 20 0 R 23 0 R ]"));
        assert!(text.contains("/MediaBox [0 0 150 100]"));
        assert!(content.contains("3 15 37 80 re W n"));
        assert!(content.contains("4 92 8 92 12 100 c\n18 94 24 94 30 100 c\nh\nB"));
        assert!(content.contains("2 w 1 J 2 j 4 M\n[ 2 3 ] 4 d"));
        assert!(text.contains("/F128 << /Type /ExtGState /ca 0.501961 >>"));
        assert!(text.contains("/S160 << /Type /ExtGState /CA 0.627451 >>"));
        assert!(content.contains("Tf 0 1 -1 0 80 "));
        assert!(text.contains("<03B2>"));
        assert!(text.contains("/Filter /FlateDecode"));
    }

    #[test]
    fn unsupported_scenes_are_rejected_before_any_bytes_are_written() {
        let mut image = Scene::new(40, 30);
        image.draw_image(
            &RasterImage::new(1, 1, vec![42; 4]).unwrap(),
            [1., 0., 0., 1., 0., 0.],
            false,
        );
        let arc = Scene::from_operations(
            (40, 30),
            vec![DrawOperation::Path(Path {
                commands: vec![
                    PathCommand::MoveTo(1., 1.),
                    PathCommand::ArcTo {
                        rx: 1.,
                        ry: 1.,
                        x: 2.,
                        y: 2.,
                    },
                ],
                ..Path::default()
            })],
        );
        let no_glyph = Scene::from_operations(
            (40, 30),
            vec![DrawOperation::Text {
                text: "積".into(),
                position: Point::default(),
                params: PlotParameters {
                    text_color: Color::BLACK,
                    ..PlotParameters::default()
                },
            }],
        );
        for page in [image, arc, no_glyph] {
            let mut output = Vec::new();
            let error = write_pdf(&[Scene::new(30, 40), page], &mut output).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::Unsupported);
            assert!(output.is_empty());
        }
    }

    #[test]
    fn returns_the_bounded_sink_error() {
        struct Limit(usize);
        impl Write for Limit {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if bytes.len() > self.0 {
                    return Err(io::Error::other("PDF byte limit"));
                }
                self.0 -= bytes.len();
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let error = write_pdf(&[sample()], &mut Limit(4_096)).unwrap_err();
        assert_eq!(error.to_string(), "PDF byte limit");
    }
}
