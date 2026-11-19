use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2, pos2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Play,
    Pause,
    Previous,
    Next,
    Volume,
    Muted,
    Subtitles,
    AudioTrack,
    Sliders,
    Playlist,
    Info,
    Fullscreen,
    Minus,
    Plus,
    Close,
    Shuffle,
    Repeat,
    RepeatOne,
    Loop,
    More,
}

struct Pen<'a> {
    painter: &'a Painter,
    origin: Pos2,
    unit: f32,
    color: Color32,
}

impl Pen<'_> {
    fn at(&self, x: f32, y: f32) -> Pos2 {
        self.origin + vec2(x, y) * self.unit
    }

    fn stroke(&self) -> Stroke {
        Stroke::new((self.unit * 1.5).max(1.2), self.color)
    }

    fn line(&self, from: (f32, f32), to: (f32, f32)) {
        self.painter.line_segment(
            [self.at(from.0, from.1), self.at(to.0, to.1)],
            self.stroke(),
        );
    }

    fn path(&self, points: &[(f32, f32)]) {
        for pair in points.windows(2) {
            self.line(pair[0], pair[1]);
        }
    }

    fn fill(&self, points: &[(f32, f32)]) {
        let points = points.iter().map(|(x, y)| self.at(*x, *y)).collect();
        self.painter
            .add(Shape::convex_polygon(points, self.color, Stroke::NONE));
    }

    fn bar(&self, x: f32, y: f32, width: f32, height: f32) {
        let rect = Rect::from_min_size(self.at(x, y), Vec2::new(width, height) * self.unit);
        self.painter.rect_filled(rect, self.unit * 0.6, self.color);
    }

    fn frame(&self, x: f32, y: f32, width: f32, height: f32) {
        let rect = Rect::from_min_size(self.at(x, y), Vec2::new(width, height) * self.unit);
        self.painter
            .rect_stroke(rect, self.unit * 1.6, self.stroke(), StrokeKind::Inside);
    }

    fn ring(&self, x: f32, y: f32, radius: f32) {
        self.painter
            .circle_stroke(self.at(x, y), radius * self.unit, self.stroke());
    }

    fn dot(&self, x: f32, y: f32, radius: f32) {
        self.painter
            .circle_filled(self.at(x, y), radius * self.unit, self.color);
    }
}

impl Icon {
    pub fn paint(self, painter: &Painter, rect: Rect, color: Color32) {
        let side = rect.width().min(rect.height());
        let pen = Pen {
            painter,
            origin: rect.center() - Vec2::splat(side / 2.0),
            unit: side / 16.0,
            color,
        };
        match self {
            Self::Play => pen.fill(&[(4.5, 3.0), (13.0, 8.0), (4.5, 13.0)]),
            Self::Pause => {
                pen.bar(4.0, 3.0, 3.0, 10.0);
                pen.bar(9.0, 3.0, 3.0, 10.0);
            }
            Self::Previous => {
                pen.bar(3.0, 4.0, 1.8, 8.0);
                pen.fill(&[(12.5, 4.0), (5.5, 8.0), (12.5, 12.0)]);
            }
            Self::Next => {
                pen.bar(11.2, 4.0, 1.8, 8.0);
                pen.fill(&[(3.5, 4.0), (10.5, 8.0), (3.5, 12.0)]);
            }
            Self::Volume | Self::Muted => {
                pen.fill(&[(2.0, 6.0), (5.0, 6.0), (5.0, 10.0), (2.0, 10.0)]);
                pen.fill(&[(5.0, 6.0), (8.5, 3.0), (8.5, 13.0), (5.0, 10.0)]);
                if self == Self::Muted {
                    pen.line((10.5, 6.0), (14.0, 10.0));
                    pen.line((14.0, 6.0), (10.5, 10.0));
                } else {
                    pen.path(&[(10.5, 5.5), (11.8, 8.0), (10.5, 10.5)]);
                    pen.path(&[(12.5, 3.5), (14.5, 8.0), (12.5, 12.5)]);
                }
            }
            Self::Subtitles => {
                pen.frame(1.5, 3.5, 13.0, 9.0);
                pen.line((4.0, 7.0), (8.0, 7.0));
                pen.line((10.0, 7.0), (12.0, 7.0));
                pen.line((4.0, 9.8), (6.0, 9.8));
                pen.line((8.0, 9.8), (12.0, 9.8));
            }
            Self::AudioTrack => {
                for (x, top) in [(3.0, 6.0), (6.3, 3.0), (9.6, 5.0), (12.9, 7.0)] {
                    pen.line((x, top), (x, 16.0 - top));
                }
            }
            Self::Sliders => {
                for (y, knob) in [(4.0, 10.5), (8.0, 5.5), (12.0, 9.0)] {
                    pen.line((2.0, y), (14.0, y));
                    pen.dot(knob, y, 1.9);
                }
            }
            Self::Playlist => {
                for y in [4.0, 8.0, 12.0] {
                    pen.dot(2.8, y, 1.0);
                    pen.line((5.5, y), (14.0, y));
                }
            }
            Self::Info => {
                pen.ring(8.0, 8.0, 6.0);
                pen.dot(8.0, 5.0, 0.9);
                pen.line((8.0, 7.3), (8.0, 11.3));
            }
            Self::Fullscreen => {
                pen.path(&[(2.5, 6.0), (2.5, 2.5), (6.0, 2.5)]);
                pen.path(&[(10.0, 2.5), (13.5, 2.5), (13.5, 6.0)]);
                pen.path(&[(13.5, 10.0), (13.5, 13.5), (10.0, 13.5)]);
                pen.path(&[(6.0, 13.5), (2.5, 13.5), (2.5, 10.0)]);
            }
            Self::More => {
                for x in [3.5, 8.0, 12.5] {
                    pen.dot(x, 8.0, 1.3);
                }
            }
            Self::Minus => pen.line((4.0, 8.0), (12.0, 8.0)),
            Self::Plus => {
                pen.line((4.0, 8.0), (12.0, 8.0));
                pen.line((8.0, 4.0), (8.0, 12.0));
            }
            Self::Close => {
                pen.line((4.5, 4.5), (11.5, 11.5));
                pen.line((11.5, 4.5), (4.5, 11.5));
            }
            Self::Shuffle => {
                pen.path(&[(2.0, 4.5), (5.0, 4.5), (10.0, 11.5), (13.5, 11.5)]);
                pen.path(&[(2.0, 11.5), (5.0, 11.5), (10.0, 4.5), (13.5, 4.5)]);
                pen.path(&[(12.0, 3.0), (13.8, 4.5), (12.0, 6.0)]);
                pen.path(&[(12.0, 10.0), (13.8, 11.5), (12.0, 13.0)]);
            }
            Self::Repeat | Self::RepeatOne | Self::Loop => {
                pen.path(&[(4.0, 11.5), (2.5, 11.5), (2.5, 4.5), (11.5, 4.5)]);
                pen.path(&[(12.0, 4.5), (13.5, 4.5), (13.5, 11.5), (4.5, 11.5)]);
                pen.path(&[(10.0, 3.0), (11.8, 4.5), (10.0, 6.0)]);
                pen.path(&[(6.0, 10.0), (4.2, 11.5), (6.0, 13.0)]);
                match self {
                    Self::RepeatOne => pen.line((8.0, 6.6), (8.0, 9.6)),
                    Self::Loop => {
                        pen.dot(6.3, 8.0, 0.9);
                        pen.dot(9.7, 8.0, 0.9);
                    }
                    _ => {}
                }
            }
        }
    }
}

pub fn center_square(rect: Rect, side: f32) -> Rect {
    Rect::from_center_size(pos2(rect.center().x, rect.center().y), Vec2::splat(side))
}
