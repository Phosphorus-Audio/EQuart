use crate::{band::BAND_COUNT, region::REGION_COUNT};
use egui::Color32;
use serde::{Deserialize, Serialize};

pub(crate) mod presets;

macro_rules! palette {
    ($name:ident { $($field:ident: $label:literal = $color:expr),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(default)]
        pub(crate) struct $name {
            $(#[serde(with = "color")] pub(crate) $field: Color32),+
        }

        impl Default for $name {
            fn default() -> Self {
                Self { $($field: $color),+ }
            }
        }

        impl $name {
            pub(crate) fn edit(&mut self, mut edit_color: impl FnMut(&str, &mut Color32)) {
                $(edit_color($label, &mut self.$field);)+
            }

            fn merge_edits(&mut self, original: &Self, edited: &Self) {
                $(merge_value(&mut self.$field, original.$field, edited.$field);)+
            }
        }
    };
}

const fn gray(value: u8) -> Color32 {
    Color32::from_gray(value)
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

palette!(Background {
    top: "Canvas top" = gray(24),
    bottom: "Canvas bottom" = gray(32),
    footer_top: "Footer top" = gray(46),
    footer_bottom: "Footer bottom" = gray(27),
    footer_border: "Footer border" = gray(76),
});

palette!(Grid {
    major: "Major lines" = rgb(54, 57, 59),
    minor: "Minor lines" = gray(34),
    zero: "Zero dB line" = gray(72),
});

palette!(Text {
    primary: "Primary" = rgb(213, 214, 214),
    muted: "Secondary and axis labels" = rgb(145, 148, 150),
    spectrum_scale: "Spectrum scale" = gray(112),
    heading: "Section headings" = gray(134),
    body: "Menu labels" = gray(200),
    value: "Menu values" = gray(142),
});

palette!(Panel {
    parameter: "Band and region panels" = gray(38),
    label: "Band and region text" = rgb(213, 214, 214),
    muted_label: "Band and region secondary text" = rgb(145, 148, 150),
    top: "Settings top" = gray(38),
    bottom: "Settings bottom" = gray(30),
    menu: "Popup background" = gray(33),
    border: "Borders" = gray(64),
    option_border: "Option borders" = gray(86),
    option: "Option background" = gray(42),
    hover: "Option hover" = gray(50),
    selected: "Selected option" = gray(58),
    marker: "Band marker center" = gray(27),
    accent: "Default accent" = rgb(150, 157, 164),
});

palette!(KnobColors {
    face_top: "Face top" = gray(25),
    face_bottom: "Face bottom" = gray(13),
    shoulder_top: "Shoulder top" = gray(26),
    shoulder_bottom: "Shoulder bottom" = gray(11),
    bezel_top: "Bezel top" = gray(8),
    bezel_bottom: "Bezel bottom" = gray(26),
    rim_top: "Rim top" = gray(30),
    rim_bottom: "Rim bottom" = gray(6),
    pointer: "Pointer" = gray(238),
    ticks: "Ticks" = gray(112),
    caption: "Caption" = gray(158),
    value: "Value" = gray(128),
    arc: "Value arc" = rgb(196, 130, 45),
    gain_arc: "Dynamic gain arc" = gray(215),
    shadow: "Shadow" = Color32::BLACK,
});

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Knob {
    pub(crate) colors: KnobColors,
    pub(crate) follow_accent: bool,
}

impl Default for Knob {
    fn default() -> Self {
        Self {
            colors: KnobColors::default(),
            follow_accent: true,
        }
    }
}

palette!(Meter {
    well: "Well" = gray(14),
    track: "Segment track" = gray(22),
    edge: "Column divider" = gray(44),
    low: "Normal level" = rgb(104, 192, 108),
    medium: "Above −18 dBFS" = rgb(224, 198, 72),
    high: "Above −6 dBFS" = rgb(232, 142, 52),
    red: "Above −1 dBFS" = rgb(201, 55, 65),
    peak: "Peak marker" = Color32::WHITE,
    clip: "Clipping marker" = rgb(255, 214, 214),
    silent: "Silent readout" = gray(96),
    compact_track: "Compact meter track" = gray(26),
    compact_level: "Compact meter level" = gray(155),
});

palette!(Scrollbar {
    track: "Track" = gray(10),
    thumb: "Thumb" = gray(60),
    hover: "Hover" = gray(70),
    active: "Dragging" = gray(55),
});

palette!(Curves {
    response: "Combined EQ response" = rgb(245, 208, 76),
    phase: "Phase" = rgb(142, 190, 203),
    peak_hold: "Peak hold" = rgb(238, 243, 245),
});

palette!(DropdownColors {
    background: "Background" = gray(42),
    hover: "Hover" = gray(50),
    selected: "Selected / open" = gray(58),
    border: "Border" = gray(86),
    text: "Text" = rgb(213, 214, 214),
    arrow: "Arrow" = gray(142),
    popup: "Popup background" = gray(42),
});

palette!(SwitchColors {
    track_off: "Track off" = gray(64),
    track_on: "Track on" = rgb(128, 134, 140),
    thumb: "Thumb" = rgb(213, 214, 214),
});

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Switch {
    pub(crate) colors: SwitchColors,
    pub(crate) follow_accent: bool,
}

impl Default for Switch {
    fn default() -> Self {
        Self {
            colors: SwitchColors::default(),
            follow_accent: true,
        }
    }
}

palette!(Piano {
    well: "Background" = gray(15),
    white_top: "White key top" = gray(198),
    white_bottom: "White key bottom" = gray(138),
    black: "Black keys" = gray(18),
    tip_background: "Tooltip background" = rgb(22, 26, 24),
    tip_text: "Tooltip text" = rgb(213, 214, 214),
});

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ColorMode {
    Solid,
    #[default]
    Gradient,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct SpectrumPaint {
    pub(crate) mode: ColorMode,
    #[serde(with = "color")]
    pub(crate) solid: Color32,
    #[serde(with = "color_array")]
    pub(crate) stops: [Color32; 5],
}

impl Default for SpectrumPaint {
    fn default() -> Self {
        Self {
            mode: ColorMode::Gradient,
            solid: rgb(104, 190, 190),
            stops: [
                rgb(186, 214, 120),
                rgb(126, 205, 148),
                rgb(104, 190, 190),
                rgb(100, 146, 186),
                rgb(120, 112, 178),
            ],
        }
    }
}

impl SpectrumPaint {
    fn merge_edits(&mut self, original: &Self, edited: &Self) {
        merge_value(&mut self.mode, original.mode, edited.mode);
        merge_value(&mut self.solid, original.solid, edited.solid);
        merge_colors(&mut self.stops, original.stops, edited.stops);
    }

    pub(crate) fn color(self, t: f32) -> Color32 {
        match self.mode {
            ColorMode::Solid => self.solid,
            ColorMode::Gradient => {
                let stop = t.clamp(0.0, 1.0) * (self.stops.len() - 1) as f32;
                let first = (stop as usize).min(self.stops.len() - 2);
                self.stops[first].lerp_to_gamma(self.stops[first + 1], stop - first as f32)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Theme {
    pub(crate) background: Background,
    pub(crate) grid: Grid,
    pub(crate) text: Text,
    pub(crate) panel: Panel,
    pub(crate) dropdown: DropdownColors,
    pub(crate) panel_dropdown: DropdownColors,
    pub(crate) switch: Switch,
    pub(crate) knob: Knob,
    pub(crate) meter: Meter,
    pub(crate) scrollbar: Scrollbar,
    pub(crate) curves: Curves,
    pub(crate) piano: Piano,
    pub(crate) spectrum_fill: SpectrumPaint,
    pub(crate) spectrum_trace: SpectrumPaint,
    #[serde(with = "color_array")]
    pub(crate) band: [Color32; BAND_COUNT],
    #[serde(with = "color_array")]
    pub(crate) region: [Color32; REGION_COUNT],
}

impl Theme {
    pub(crate) fn merge_edits(&mut self, original: &Self, edited: &Self) {
        self.background
            .merge_edits(&original.background, &edited.background);
        self.grid.merge_edits(&original.grid, &edited.grid);
        self.text.merge_edits(&original.text, &edited.text);
        self.panel.merge_edits(&original.panel, &edited.panel);
        self.dropdown
            .merge_edits(&original.dropdown, &edited.dropdown);
        self.panel_dropdown
            .merge_edits(&original.panel_dropdown, &edited.panel_dropdown);
        self.switch
            .colors
            .merge_edits(&original.switch.colors, &edited.switch.colors);
        merge_value(
            &mut self.switch.follow_accent,
            original.switch.follow_accent,
            edited.switch.follow_accent,
        );
        self.knob
            .colors
            .merge_edits(&original.knob.colors, &edited.knob.colors);
        merge_value(
            &mut self.knob.follow_accent,
            original.knob.follow_accent,
            edited.knob.follow_accent,
        );
        self.meter.merge_edits(&original.meter, &edited.meter);
        self.scrollbar
            .merge_edits(&original.scrollbar, &edited.scrollbar);
        self.curves.merge_edits(&original.curves, &edited.curves);
        self.piano.merge_edits(&original.piano, &edited.piano);
        self.spectrum_fill
            .merge_edits(&original.spectrum_fill, &edited.spectrum_fill);
        self.spectrum_trace
            .merge_edits(&original.spectrum_trace, &edited.spectrum_trace);
        merge_colors(&mut self.band, original.band, edited.band);
        merge_colors(&mut self.region, original.region, edited.region);
    }
}

fn merge_value<T: PartialEq>(current: &mut T, original: T, edited: T) {
    if original != edited {
        *current = edited;
    }
}

fn merge_colors<const N: usize>(
    current: &mut [Color32; N],
    original: [Color32; N],
    edited: [Color32; N],
) {
    for ((color, original), edited) in current.iter_mut().zip(original).zip(edited) {
        merge_value(color, original, edited);
    }
}

impl Default for Theme {
    fn default() -> Self {
        let spectrum_fill = SpectrumPaint::default();
        let brighten = |ink: Color32| ink.lerp_to_gamma(Color32::WHITE, 0.42);

        Self {
            background: Background::default(),
            grid: Grid::default(),
            text: Text::default(),
            panel: Panel::default(),
            dropdown: DropdownColors::default(),
            panel_dropdown: DropdownColors::default(),
            switch: Switch::default(),
            knob: Knob::default(),
            meter: Meter::default(),
            scrollbar: Scrollbar::default(),
            curves: Curves::default(),
            piano: Piano::default(),
            spectrum_fill,
            spectrum_trace: SpectrumPaint {
                solid: brighten(spectrum_fill.solid),
                stops: spectrum_fill.stops.map(brighten),
                ..spectrum_fill
            },
            band: std::array::from_fn(band_color),
            region: [rgb(113, 161, 225); REGION_COUNT],
        }
    }
}

fn band_color(index: usize) -> Color32 {
    const FIRST: [Color32; 8] = [
        rgb(201, 55, 65),
        rgb(196, 130, 45),
        rgb(49, 153, 120),
        rgb(80, 131, 200),
        rgb(151, 108, 192),
        rgb(203, 105, 151),
        rgb(140, 159, 61),
        rgb(50, 164, 182),
    ];

    if index < FIRST.len() {
        return FIRST[index];
    }

    let hue = (index as f32 * 0.618_034) % 1.0 * 6.0;
    let channel = |offset| {
        let k: f32 = (hue + offset) % 6.0;
        ((0.82 - 0.82 * 0.62 * k.min(4.0 - k).clamp(0.0, 1.0)) * 255.0) as u8
    };
    rgb(channel(5.0), channel(3.0), channel(1.0))
}

mod color {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(
        color: &Color32,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        [color.r(), color.g(), color.b()].serialize(serializer)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Color32, D::Error> {
        let [r, g, b] = <[u8; 3]>::deserialize(deserializer)?;
        Ok(rgb(r, g, b))
    }
}

mod color_array {
    use super::*;
    use serde::{Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer, const N: usize>(
        colors: &[Color32; N],
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        [[u8; 3]; N]: Serialize,
    {
        colors
            .map(|color| [color.r(), color.g(), color.b()])
            .serialize(serializer)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>, const N: usize>(
        deserializer: D,
    ) -> Result<[Color32; N], D::Error>
    where
        [[u8; 3]; N]: Deserialize<'de>,
    {
        let colors = <[[u8; 3]; N]>::deserialize(deserializer)?;
        Ok(colors.map(|[r, g, b]| rgb(r, g, b)))
    }
}
