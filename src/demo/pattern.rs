use crate::media::PooledPlanes;

const PANEL_LUMA: u8 = 22;
const INK_LUMA: u8 = 235;
const NEUTRAL: (u8, u8) = (128, 128);
const BANDS: usize = 9;
const BAND_HUES: [(f32, f32, f32); BANDS] = [
    (0.95, 0.35, 0.20),
    (0.98, 0.58, 0.18),
    (0.95, 0.82, 0.25),
    (0.45, 0.80, 0.35),
    (0.20, 0.75, 0.65),
    (0.20, 0.55, 0.92),
    (0.38, 0.38, 0.95),
    (0.65, 0.35, 0.90),
    (0.90, 0.30, 0.60),
];
const SEGMENTS: [u8; 10] = [
    0b0111111, 0b0000110, 0b1011011, 0b1001111, 0b1100110, 0b1101101, 0b1111101, 0b0000111,
    0b1111111, 0b1101111,
];

pub struct FramePosition {
    pub index: u64,
    pub frame_rate: u32,
    pub silent: bool,
}

struct Canvas<'a> {
    luma: &'a mut [u8],
    chroma: &'a mut [u8],
    width: usize,
    height: usize,
}

fn chroma_of(red: f32, green: f32, blue: f32) -> (u8, u8) {
    let cb = -0.1146 * red - 0.3854 * green + 0.5 * blue;
    let cr = 0.5 * red - 0.4542 * green - 0.0458 * blue;
    (
        (128.0 + cb * 224.0 * 0.6).round() as u8,
        (128.0 + cr * 224.0 * 0.6).round() as u8,
    )
}

impl Canvas<'_> {
    fn fill(&mut self, x: usize, y: usize, w: usize, h: usize, luma: u8, chroma: (u8, u8)) {
        let x_end = (x + w).min(self.width);
        let y_end = (y + h).min(self.height);
        if x >= x_end || y >= y_end {
            return;
        }
        for row in y..y_end {
            self.luma[row * self.width + x..row * self.width + x_end].fill(luma);
        }
        for row in y / 2..y_end.div_ceil(2) {
            let start = row * self.width + (x / 2) * 2;
            let end = row * self.width + x_end.div_ceil(2) * 2;
            for pair in self.chroma[start..end].as_chunks_mut::<2>().0 {
                pair[0] = chroma.0;
                pair[1] = chroma.1;
            }
        }
    }

    fn bands(&mut self, frame: u64) {
        let band_rows = self.height.div_ceil(BANDS);
        let mut luma_row = vec![0u8; self.width];
        let mut chroma_row = vec![0u8; self.width];
        for (band, hue) in BAND_HUES.iter().enumerate() {
            let speed = 6 + band as u64 * 5;
            let period = 384 + band * 96;
            let offset = (frame * speed) as usize;
            for (x, value) in luma_row.iter_mut().enumerate() {
                let phase = (x + offset) % period;
                let ramp = if phase * 2 < period {
                    phase * 2
                } else {
                    (period - phase) * 2
                };
                *value = (52 + ramp * 150 / period) as u8;
            }
            let (cb, cr) = chroma_of(hue.0, hue.1, hue.2);
            for pair in chroma_row.as_chunks_mut::<2>().0 {
                pair[0] = cb;
                pair[1] = cr;
            }
            let first = band * band_rows;
            let last = ((band + 1) * band_rows).min(self.height);
            for row in first..last {
                self.luma[row * self.width..(row + 1) * self.width].copy_from_slice(&luma_row);
            }
            for row in first / 2..last.div_ceil(2).min(self.height / 2) {
                self.chroma[row * self.width..(row + 1) * self.width].copy_from_slice(&chroma_row);
            }
        }
    }

    fn digit(&mut self, value: usize, x: usize, y: usize, scale: usize) {
        let (long, thick) = (scale * 4, scale);
        let mask = SEGMENTS[value % 10];
        let strokes = [
            (thick, 0, long, thick),
            (thick + long, thick, thick, long),
            (thick + long, 2 * thick + long, thick, long),
            (thick, 2 * (thick + long), long, thick),
            (0, 2 * thick + long, thick, long),
            (0, thick, thick, long),
            (thick, thick + long, long, thick),
        ];
        for (bit, (dx, dy, w, h)) in strokes.into_iter().enumerate() {
            if mask & (1 << bit) != 0 {
                self.fill(x + dx, y + dy, w, h, INK_LUMA, NEUTRAL);
            }
        }
    }

    fn number(&mut self, value: u64, digits: usize, x: usize, y: usize, scale: usize) -> usize {
        let advance = scale * 8;
        for place in 0..digits {
            let divisor = 10u64.pow((digits - 1 - place) as u32);
            self.digit(
                ((value / divisor) % 10) as usize,
                x + place * advance,
                y,
                scale,
            );
        }
        x + digits * advance
    }

    fn colon(&mut self, x: usize, y: usize, scale: usize) -> usize {
        self.fill(x, y + scale * 3, scale, scale, INK_LUMA, NEUTRAL);
        self.fill(x, y + scale * 8, scale, scale, INK_LUMA, NEUTRAL);
        x + scale * 3
    }
}

pub fn paint(planes: &mut PooledPlanes, width: u32, height: u32, position: &FramePosition) {
    let (width, height) = (width as usize, height as usize);
    let mut buffers = planes.planes_mut();
    let (Some((luma, _)), Some((chroma, _))) = (buffers.next(), buffers.next()) else {
        return;
    };
    let mut canvas = Canvas {
        luma,
        chroma,
        width,
        height,
    };
    let unit = (width / 240).max(2);
    let rate = u64::from(position.frame_rate);
    let frame_in_second = position.index % rate;
    let seconds = position.index / rate;

    canvas.bands(position.index);

    let sweep = (frame_in_second as usize * width) / rate as usize;
    canvas.fill(sweep, 0, unit, height, INK_LUMA, NEUTRAL);

    let scale = unit * 2;
    let panel_w = scale * 8 * 6 + scale * 3 * 2 + unit * 8;
    let panel_h = scale * 11 + unit * 16;
    let panel_x = (width - panel_w) / 2;
    let panel_y = (height - panel_h) / 2;
    canvas.fill(panel_x, panel_y, panel_w, panel_h, PANEL_LUMA, NEUTRAL);
    let text_y = panel_y + unit * 4;
    let mut cursor = panel_x + unit * 4;
    cursor = canvas.number(seconds / 60, 2, cursor, text_y, scale);
    cursor = canvas.colon(cursor, text_y, scale);
    cursor = canvas.number(seconds % 60, 2, cursor, text_y, scale);
    cursor = canvas.colon(cursor, text_y, scale);
    canvas.number(frame_in_second, 2, cursor, text_y, scale);
    canvas.number(
        position.index,
        7,
        panel_x + unit * 4,
        text_y + scale * 11 + unit * 3,
        unit / 2 + 1,
    );

    let cell = panel_w / rate as usize;
    let ruler_y = panel_y + panel_h + unit * 2;
    canvas.fill(
        panel_x,
        ruler_y,
        cell * rate as usize,
        unit * 3,
        PANEL_LUMA,
        NEUTRAL,
    );
    canvas.fill(
        panel_x + cell * frame_in_second as usize,
        ruler_y,
        cell,
        unit * 3,
        INK_LUMA,
        NEUTRAL,
    );

    let badge = unit * 10;
    let badge_x = width - badge - unit * 6;
    let flash = frame_in_second < 3 && !position.silent;
    let badge_luma = if flash { INK_LUMA } else { PANEL_LUMA };
    canvas.fill(badge_x, unit * 6, badge, badge, badge_luma, NEUTRAL);
    if position.silent {
        let muted = chroma_of(0.95, 0.25, 0.2);
        canvas.fill(
            badge_x - badge - unit * 2,
            unit * 6,
            badge,
            badge,
            120,
            muted,
        );
    }
}
