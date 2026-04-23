

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const BLACK: Color = Color { r: 0, g: 0, b: 0, a: 255 };
    pub const WHITE: Color = Color { r: 255, g: 255, b: 255, a: 255 };
    pub const RED: Color = Color { r: 220, g: 50, b: 50, a: 255 };
    pub const GREEN: Color = Color { r: 50, g: 180, b: 50, a: 255 };
    pub const BLUE: Color = Color { r: 50, g: 100, b: 220, a: 255 };
    pub const GRAY: Color = Color { r: 180, g: 180, b: 180, a: 255 };
    pub const LIGHT_GRAY: Color = Color { r: 230, g: 230, b: 230, a: 255 };
    pub const DARK_GRAY: Color = Color { r: 100, g: 100, b: 100, a: 255 };
    pub const ORANGE: Color = Color { r: 255, g: 140, b: 0, a: 255 };
    pub const PURPLE: Color = Color { r: 150, g: 50, b: 200, a: 255 };
    pub const CYAN: Color = Color { r: 0, g: 180, b: 220, a: 255 };

    pub const PALETTE: [Color; 7] = [
        Color::BLUE,
        Color::RED,
        Color::GREEN,
        Color::ORANGE,
        Color::PURPLE,
        Color::CYAN,
        Color::DARK_GRAY,
    ];
}



pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Bitmap {
    pub fn new(width: u32, height: u32) -> Self {
        let size = (width * height * 4) as usize;
        Bitmap {
            width,
            height,
            pixels: vec![255; size],
        }
    }

    #[inline]
    fn idx(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return None;
        }
    }

    pub fn set_pixel(&mut self, x: i32, y: i32, color: Color) {
        if let Some(i) = self.idx(x, y) {
            self.pixels[i] = color.r;
            self.pixels[i + 1] = color.g;
            self.pixels[i + 2] = color.b;
            self.pixels[i + 3] = color.a;
        }
    }

    pub fn fill(&mut self, color: Color) {
        for y in 0..self.height as i32 {
            for x in 0..self.width as i32 {
                self.set_pixel(x, y, color);
            }
        }
    }

    pub fn draw_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: Color) {
        let dx = (x1 - x0).abs();
        let dy = (y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx - dy;
        let mut x = x0;
        let mut y = y0;

        loop {
            self.set_pixel(x, y, color);
            if x == x1 && y == y1 {
                break;
            }
        }
    }

    pub fn draw_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color) {
        for dy in 0..h {
            for dx in 0..w {
                self.set_pixel(x + dx, y + dy, color);
            }
        }
    }

    pub fn draw_circle(&mut self, cx: i32, cy: i32, r: i32, color: Color) {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy <= r * r {
                    self.set_pixel(cx + dx, cy + dy, color);
                }
            }
        }
    }

    pub fn draw_hollow_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color) {
        for dx in 0..w {
            self.set_pixel(x + dx, y, color);
            self.set_pixel(x + dx, y + h - 1, color);
        }
    }
}

/// Margin sizes in pixels.
const MARGIN_LEFT: i32 = 60;
const MARGIN_RIGHT: i32 = 20;
const MARGIN_TOP: i32 = 40;
const MARGIN_BOTTOM: i32 = 50;

fn map_x(x: f64, xmin: f64, xmax: f64, plot_w: i32) -> i32 {
    let t = if xmax == xmin { 0.5 } else { (x - xmin) / (xmax - xmin) };
    MARGIN_LEFT + (t * plot_w as f64) as i32
}

fn map_y(y: f64, ymin: f64, ymax: f64, plot_h: i32) -> i32 {
    let t = if ymax == ymin { 0.5 } else { (y - ymin) / (ymax - ymin) };
    MARGIN_TOP + plot_h - (t * plot_h as f64) as i32
}

fn nice_range(min: f64, max: f64) -> (f64, f64, f64) {
    if min == max {
        return (min - 1.0, max + 1.0, 1.0);
    }
    let span = max - min;
    let step = 10f64.powf(span.log10().floor());
    let nice_min = (min / step).floor() * step;
    let nice_max = (max / step).ceil() * step;
    let count = ((nice_max - nice_min) / step).max(1.0);
    let nice_step = if count > 10.0 { step * 2.0 } else if count < 3.0 { step / 2.0 } else { step };
    (nice_min, nice_max, nice_step)
}



fn draw_axes(bm: &mut Bitmap, xmin: f64, xmax: f64, ymin: f64, ymax: f64) {
    let plot_w = bm.width as i32 - MARGIN_LEFT - MARGIN_RIGHT;
    let plot_h = bm.height as i32 - MARGIN_TOP - MARGIN_BOTTOM;

    bm.draw_rect(MARGIN_LEFT, MARGIN_TOP, plot_w, plot_h, Color::WHITE);

    let (x0, x1, xs) = nice_range(xmin, xmax);
    let (y0, y1, ys) = nice_range(ymin, ymax);

    let mut x = x0;
    while x <= x1 + xs * 0.5 {
        let px = map_x(x, xmin, xmax, plot_w);
        bm.draw_line(px, MARGIN_TOP, px, MARGIN_TOP + plot_h, Color::LIGHT_GRAY);
        x += xs;
    }
    let mut y = y0;
    while y <= y1 + ys * 0.5 {
        let py = map_y(y, ymin, ymax, plot_h);
        bm.draw_line(MARGIN_LEFT, py, MARGIN_LEFT + plot_w, py, Color::LIGHT_GRAY);
        y += ys;
    }

    bm.draw_line(MARGIN_LEFT, MARGIN_TOP + plot_h, MARGIN_LEFT + plot_w, MARGIN_TOP + plot_h, Color::BLACK);
    bm.draw_line(MARGIN_LEFT, MARGIN_TOP, MARGIN_LEFT, MARGIN_TOP + plot_h, Color::BLACK);
}


/// Render a scatter plot.
pub fn scatter_plot(x: &[f64], y: &[f64], width: u32, height: u32) -> Bitmap {
    let mut bm = Bitmap::new(width, height);
    bm.fill(Color::WHITE);

    let n = x.len().min(y.len());
    if n == 0 {
        return bm;
    }

    let xmin = x.iter().cloned().fold(f64::INFINITY, f64::min);
    let xmax = x.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let ymin = y.iter().cloned().fold(f64::INFINITY, f64::min);
    let ymax = y.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    let plot_w = width as i32 - MARGIN_LEFT - MARGIN_RIGHT;
    let plot_h = height as i32 - MARGIN_TOP - MARGIN_BOTTOM;

    draw_axes(&mut bm, xmin, xmax, ymin, ymax);

    for i in 0..n {
        let px = map_x(x[i], xmin, xmax, plot_w);
        let py = map_y(y[i], ymin, ymax, plot_h);
        bm.draw_circle(px, py, 3, Color::BLUE);
    }

    bm
}

/// Render a line plot.
pub fn line_plot(x: &[f64], y: &[f64], width: u32, height: u32) -> Bitmap {
    let mut bm = Bitmap::new(width, height);
    bm.fill(Color::WHITE);

    let n = x.len().min(y.len());
    if n == 0 {
        return bm;
    }

    let xmin = x.iter().cloned().fold(f64::INFINITY, f64::min);
    let xmax = x.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let ymin = y.iter().cloned().fold(f64::INFINITY, f64::min);
    let ymax = y.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    let plot_w = width as i32 - MARGIN_LEFT - MARGIN_RIGHT;
    let plot_h = height as i32 - MARGIN_TOP - MARGIN_BOTTOM;

    draw_axes(&mut bm, xmin, xmax, ymin, ymax);

    for i in 1..n {
        let x0 = map_x(x[i - 1], xmin, xmax, plot_w);
        let y0 = map_y(y[i - 1], ymin, ymax, plot_h);
        let x1 = map_x(x[i], xmin, xmax, plot_w);
        let y1 = map_y(y[i], ymin, ymax, plot_h);
        bm.draw_line(x0, y0, x1, y1, Color::BLUE);
    }

    bm
}

/// Render a histogram.
pub fn histogram(data: &[f64], breaks: usize, width: u32, height: u32) -> Bitmap {
    let mut bm = Bitmap::new(width, height);
    bm.fill(Color::WHITE);

    if data.is_empty() || breaks == 0 {
        return bm;
    }

    let min = data.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = data.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let span = max - min;
    let bin_width = if span == 0.0 { 1.0 } else { span / breaks as f64 };

    let mut counts = vec![0usize; breaks];
    for &v in data {
        let idx = ((v - min) / bin_width).min(breaks as f64 - 1.0).max(0.0) as usize;
        counts[idx] += 1;
    }

    let ymax = counts.iter().cloned().max().unwrap_or(1) as f64;
    let plot_w = width as i32 - MARGIN_LEFT - MARGIN_RIGHT;
    let plot_h = height as i32 - MARGIN_TOP - MARGIN_BOTTOM;

    draw_axes(&mut bm, min, max, 0.0, ymax);

    let bar_w = plot_w as f64 / breaks as f64;
    for i in 0..breaks {
        let x0 = MARGIN_LEFT + (i as f64 * bar_w) as i32;
        let x1 = MARGIN_LEFT + ((i + 1) as f64 * bar_w) as i32;
        let bw = (x1 - x0).max(1);
        let bh = ((counts[i] as f64 / ymax) * plot_h as f64) as i32;
        let y0 = MARGIN_TOP + plot_h - bh;
        bm.draw_rect(x0, y0, bw, bh, Color::BLUE);
        bm.draw_hollow_rect(x0, y0, bw, bh, Color::BLACK);
    }

    bm
}

/// Render a bar chart.
pub fn bar_chart(values: &[f64], width: u32, height: u32) -> Bitmap {
    let mut bm = Bitmap::new(width, height);
    bm.fill(Color::WHITE);

    if values.is_empty() {
        return bm;
    }

    let n = values.len();
    let ymax = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max).max(0.0);
    let plot_w = width as i32 - MARGIN_LEFT - MARGIN_RIGHT;
    let plot_h = height as i32 - MARGIN_TOP - MARGIN_BOTTOM;

    draw_axes(&mut bm, 0.0, n as f64, 0.0, ymax);

    let bar_w = plot_w as f64 / n as f64;
    for i in 0..n {
        let x0 = MARGIN_LEFT + (i as f64 * bar_w) as i32;
        let bw = (bar_w as i32).max(1) - 1;
        let bh = ((values[i] / ymax.max(1e-10)) * plot_h as f64) as i32;
        let y0 = MARGIN_TOP + plot_h - bh;
        let color = Color::PALETTE[i % Color::PALETTE.len()];
        bm.draw_rect(x0 + 1, y0, bw, bh, color);
        bm.draw_hollow_rect(x0 + 1, y0, bw, bh, Color::BLACK);
    }

    bm
}
