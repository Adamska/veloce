#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoAdjustments {
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub gamma: f32,
    pub hue_degrees: f32,
    pub sharpen: f32,
    pub deinterlace: bool,
    pub frame_blending: bool,
}

impl Default for VideoAdjustments {
    fn default() -> Self {
        Self {
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            gamma: 1.0,
            hue_degrees: 0.0,
            sharpen: 0.0,
            deinterlace: false,
            frame_blending: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AspectMode {
    #[default]
    Auto,
    Wide,
    UltraWide,
    Classic,
    Square,
}

impl AspectMode {
    pub const ALL: [Self; 5] = [
        Self::Auto,
        Self::Wide,
        Self::UltraWide,
        Self::Classic,
        Self::Square,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Wide => "16:9",
            Self::UltraWide => "21:9",
            Self::Classic => "4:3",
            Self::Square => "1:1",
        }
    }

    fn ratio(self) -> Option<f32> {
        match self {
            Self::Auto => None,
            Self::Wide => Some(16.0 / 9.0),
            Self::UltraWide => Some(21.0 / 9.0),
            Self::Classic => Some(4.0 / 3.0),
            Self::Square => Some(1.0),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CropPreset {
    #[default]
    None,
    Wide,
    Flat,
    Scope,
    Classic,
    Square,
}

impl CropPreset {
    pub const ALL: [Self; 6] = [
        Self::None,
        Self::Wide,
        Self::Flat,
        Self::Scope,
        Self::Classic,
        Self::Square,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Wide => "16:9",
            Self::Flat => "1.85:1",
            Self::Scope => "2.39:1",
            Self::Classic => "4:3",
            Self::Square => "1:1",
        }
    }

    fn ratio(self) -> Option<f32> {
        match self {
            Self::None => None,
            Self::Wide => Some(16.0 / 9.0),
            Self::Flat => Some(1.85),
            Self::Scope => Some(2.39),
            Self::Classic => Some(4.0 / 3.0),
            Self::Square => Some(1.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub aspect: AspectMode,
    pub crop: CropPreset,
    pub zoom: f32,
    pub pan: [f32; 2],
    pub quarter_turns: u8,
    pub mirror: bool,
}

impl Default for Geometry {
    fn default() -> Self {
        Self {
            aspect: AspectMode::Auto,
            crop: CropPreset::None,
            zoom: 1.0,
            pan: [0.0, 0.0],
            quarter_turns: 0,
            mirror: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub scale: [f32; 2],
    pub offset: [f32; 2],
    pub uv_rows: [[f32; 3]; 2],
}

impl Placement {
    pub const IDENTITY: Self = Self {
        scale: [1.0, 1.0],
        offset: [0.0, 0.0],
        uv_rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
    };

    pub fn source_uv(&self, corner: [f32; 2]) -> [f32; 2] {
        let row = |row: [f32; 3]| row[0] * corner[0] + row[1] * corner[1] + row[2];
        [row(self.uv_rows[0]), row(self.uv_rows[1])]
    }
}

impl Geometry {
    pub fn rotate_clockwise(&mut self) {
        self.quarter_turns = (self.quarter_turns + 1) % 4;
    }

    pub fn placement(&self, source_aspect: f32, viewport: [f32; 2]) -> Placement {
        let source_aspect = source_aspect.max(1e-3);
        let (crop_size, visible_aspect) = match self.crop.ratio() {
            Some(ratio) if ratio > source_aspect => ([1.0, source_aspect / ratio], ratio),
            Some(ratio) => ([ratio / source_aspect, 1.0], ratio),
            None => ([1.0, 1.0], source_aspect),
        };
        let crop_origin = [(1.0 - crop_size[0]) / 2.0, (1.0 - crop_size[1]) / 2.0];

        let mut display_aspect = self.aspect.ratio().unwrap_or(visible_aspect);
        if self.quarter_turns % 2 == 1 {
            display_aspect = 1.0 / display_aspect;
        }
        let viewport_aspect = (viewport[0] / viewport[1].max(1.0)).max(1e-3);
        let fit = if display_aspect > viewport_aspect {
            [1.0, viewport_aspect / display_aspect]
        } else {
            [display_aspect / viewport_aspect, 1.0]
        };
        let zoom = self.zoom.max(0.1);

        let turned: [[f32; 3]; 2] = match self.quarter_turns % 4 {
            0 => [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            1 => [[0.0, 1.0, 0.0], [-1.0, 0.0, 1.0]],
            2 => [[-1.0, 0.0, 1.0], [0.0, -1.0, 1.0]],
            _ => [[0.0, -1.0, 1.0], [1.0, 0.0, 0.0]],
        };
        let mirrored = turned.map(|row| {
            if self.mirror {
                [-row[0], row[1], row[2] + row[0]]
            } else {
                row
            }
        });
        let uv_rows = [0, 1].map(|axis| {
            let row = mirrored[axis];
            [
                row[0] * crop_size[axis],
                row[1] * crop_size[axis],
                row[2] * crop_size[axis] + crop_origin[axis],
            ]
        });

        Placement {
            scale: [fit[0] * zoom, fit[1] * zoom],
            offset: [self.pan[0] * zoom, self.pan[1] * zoom],
            uv_rows,
        }
    }
}
