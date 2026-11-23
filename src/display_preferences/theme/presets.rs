use super::*;
use std::sync::OnceLock;

pub(crate) fn builtins() -> &'static [(&'static str, Theme)] {
    static PRESETS: OnceLock<[(&str, Theme); 6]> = OnceLock::new();

    PRESETS.get_or_init(|| {
        [
            ("default", Theme::default()),
            ("im_disperser_light", im_disperser_light()),
            ("im_disperser_dark", im_disperser_dark()),
            ("Dark Pink", dark_pink()),
            ("f0lock", f0lock()),
            ("InWarm", in_warm()),
        ]
    })
}

pub(crate) fn is_builtin_name(name: &str) -> bool {
    builtins()
        .iter()
        .any(|(builtin, _)| builtin.eq_ignore_ascii_case(name))
}

const fn hex(value: u32) -> Color32 {
    rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

fn solid(value: u32) -> SpectrumPaint {
    let color = hex(value);
    SpectrumPaint {
        mode: ColorMode::Solid,
        solid: color,
        stops: [color; 5],
    }
}

fn metal(colors: &mut KnobColors, face: [u32; 2], trim: [u32; 2], pointer: u32) {
    [colors.face_top, colors.face_bottom] = face.map(hex);
    [colors.rim_top, colors.rim_bottom] = trim.map(hex);
    colors.bezel_top = colors.rim_bottom;
    colors.bezel_bottom = colors.face_top;
    colors.shoulder_top = colors.face_top;
    colors.shoulder_bottom = colors.face_bottom;
    colors.pointer = hex(pointer);
}

fn im_disperser_light() -> Theme {
    let cream = hex(0xf2fbf4);
    let mint = hex(0xe0f0e6);
    let fern = hex(0x28664d);
    let ink = hex(0x183d31);
    let muted = hex(0x456254);
    let edge = hex(0xa3c0ae);
    let mut theme = Theme {
        background: Background {
            top: cream,
            bottom: hex(0xdceee2),
            header_top: hex(0xf8fff9),
            header_bottom: mint,
            footer_top: mint,
            footer_bottom: hex(0xcde5d6),
            footer_border: edge,
        },
        grid: Grid {
            major: hex(0xb6cebf),
            minor: hex(0xd0e3d6),
            zero: hex(0x8cae9d),
        },
        spectrum_fill: solid(0x6ebf93),
        spectrum_trace: solid(0x317c59),
        curves: Curves {
            response: hex(0x164e45),
            phase: hex(0x456d80),
            peak_hold: fern,
        },
        ..Theme::default()
    };
    theme.text = Text {
        primary: ink,
        muted,
        heading: muted,
        body: ink,
        value: fern,
        spectrum_scale: muted,
    };
    theme.panel = Panel {
        parameter: cream,
        label: ink,
        muted_label: muted,
        menu: cream,
        top: cream,
        bottom: mint,
        border: edge,
        option_border: edge,
        option: hex(0xd7ebdf),
        hover: hex(0xcce6d6),
        selected: hex(0xb5d9c4),
        marker: cream,
        accent: fern,
    };
    metal(
        &mut theme.knob.colors,
        [0xf9fff9, 0xd8ede0],
        [0xe3f5e8, 0x93b5a0],
        0x183d31,
    );
    theme.knob.follow_accent = false;
    theme.knob.colors.arc = hex(0x408e66);
    theme.knob.colors.caption = muted;
    theme.knob.colors.value = ink;
    theme.knob.colors.ticks = hex(0x739883);
    theme.knob.colors.gain_arc = fern;

    theme.meter = Meter {
        well: mint,
        track: hex(0xd0e5d8),
        edge,
        low: hex(0x38845e),
        medium: fern,
        high: hex(0xa05422),
        red: hex(0xa64044),
        peak: ink,
        clip: hex(0x7d2734),
        silent: muted,
        compact_track: hex(0xd0e5d8),
        compact_level: fern,
    };
    theme.scrollbar = Scrollbar {
        track: hex(0xd5e9dc),
        thumb: hex(0x83ad95),
        hover: hex(0x669b80),
        active: fern,
    };
    theme.piano = Piano {
        well: hex(0xd1e6d8),
        white_top: cream,
        white_bottom: mint,
        black: hex(0x36584a),
    };
    theme.band = theme.band.map(|color| color.lerp_to_gamma(ink, 0.25));
    theme.band[0] = fern;
    theme.region.fill(hex(0x45836d));

    theme
}

fn im_disperser_dark() -> Theme {
    let black = hex(0x090c0a);
    let surface = hex(0x121713);
    let raised = hex(0x1a231d);
    let edge = hex(0x344a3b);
    let mint = hex(0xb1ffc0);
    let cream = hex(0xf2fbf4);
    let muted = hex(0x9aba9f);
    let mut theme = Theme {
        text: Text {
            primary: mint,
            muted,
            heading: muted,
            body: cream,
            value: mint,
            spectrum_scale: hex(0x829a84),
        },
        spectrum_fill: solid(0x79c783),
        spectrum_trace: solid(0x98fb98),
        curves: Curves {
            response: mint,
            phase: hex(0x72c8b1),
            peak_hold: cream,
        },
        ..Theme::default()
    };

    theme.background = Background {
        top: black,
        bottom: surface,
        header_top: raised,
        header_bottom: black,
        footer_top: surface,
        footer_bottom: black,
        footer_border: edge,
    };
    theme.grid = Grid {
        major: hex(0x26352b),
        minor: hex(0x18231b),
        zero: hex(0x48614e),
    };
    theme.panel = Panel {
        parameter: surface,
        label: mint,
        muted_label: theme.text.muted,
        menu: surface,
        top: raised,
        bottom: black,
        border: edge,
        option_border: edge,
        option: raised,
        hover: hex(0x23342a),
        selected: hex(0x294534),
        marker: black,
        accent: mint,
    };

    theme.knob.follow_accent = false;
    let knob = &mut theme.knob.colors;
    metal(knob, [0x1a231d, 0x0b110d], [0x2c4031, 0x080b09], 0xb1ffc0);
    knob.caption = theme.text.muted;
    knob.value = mint;
    knob.ticks = hex(0x54735f);
    knob.arc = hex(0x83db9a);
    knob.gain_arc = hex(0x90dcb1);

    theme.meter.well = black;
    theme.meter.track = surface;
    theme.meter.edge = edge;
    theme.meter.compact_track = surface;
    theme.meter.low = hex(0x6dc288);
    theme.meter.medium = mint;
    theme.meter.compact_level = mint;
    theme.scrollbar.track = black;
    theme.scrollbar.thumb = edge;
    theme.scrollbar.hover = hex(0x4b6854);
    theme.scrollbar.active = hex(0x659176);
    theme.piano = Piano {
        well: black,
        white_top: hex(0x36483b),
        white_bottom: hex(0x233128),
        black,
    };
    theme.band[0] = hex(0x67c67d);
    theme.region.fill(hex(0x69bd91));

    theme
}

fn dark_pink() -> Theme {
    let ink = hex(0xffbddf);
    let mut theme = Theme {
        background: Background {
            top: hex(0x0e0d0f),
            bottom: hex(0x161416),
            header_top: gray(30),
            header_bottom: gray(17),
            footer_top: gray(30),
            footer_bottom: gray(15),
            footer_border: hex(0x513b48),
        },
        grid: Grid {
            major: hex(0x44343e),
            minor: hex(0x282026),
            zero: hex(0x6d5162),
        },
        spectrum_fill: solid(0xc592af),
        spectrum_trace: solid(0xf8c8e5),
        curves: Curves {
            response: hex(0xfff9fd),
            phase: hex(0xae8db5),
            peak_hold: ink,
        },
        ..Theme::default()
    };
    theme.text = Text {
        primary: hex(0xfff4fb),
        muted: hex(0xbbaab4),
        body: hex(0xf1d7e8),
        value: ink,
        heading: hex(0xc68cae),
        spectrum_scale: hex(0xad93a3),
    };
    theme.panel = Panel {
        parameter: hex(0x211b20),
        label: theme.text.primary,
        muted_label: theme.text.muted,
        top: hex(0x261e24),
        bottom: hex(0x171416),
        menu: hex(0x211a20),
        border: hex(0x513b48),
        option_border: hex(0x664757),
        option: hex(0x332630),
        hover: hex(0x453140),
        selected: hex(0x583a4f),
        marker: hex(0x171416),
        accent: ink,
    };
    metal(
        &mut theme.knob.colors,
        [0x353035, 0x1d1b1e],
        [0x1b171b, 0x090809],
        0xffe5f4,
    );
    theme.knob.colors.caption = hex(0xe0c3d5);
    theme.knob.colors.value = ink;
    theme.meter.low = hex(0xad698f);
    theme.meter.medium = ink;
    theme.meter.compact_level = ink;
    theme.scrollbar.thumb = hex(0x69475b);
    theme.scrollbar.hover = hex(0x8c607a);
    theme.scrollbar.active = hex(0xae7696);
    theme.band = std::array::from_fn(|index| {
        [
            0xffbddf, 0xde91ab, 0xd896ce, 0xb6a2df, 0xffc2bb, 0xee8cbc, 0xe9add7, 0xbb8fad,
        ]
        .map(hex)[index % 8]
    });
    theme.region.fill(hex(0xe69ac5));
    theme
}

fn f0lock() -> Theme {
    let red = hex(0xc93741);
    let mut theme = Theme {
        background: Background {
            top: gray(24),
            bottom: gray(32),
            header_top: gray(66),
            header_bottom: gray(43),
            footer_top: gray(48),
            footer_bottom: gray(37),
            footer_border: gray(67),
        },
        grid: Grid {
            major: hex(0x36393b),
            minor: gray(35),
            zero: gray(75),
        },
        spectrum_fill: solid(0x767676),
        spectrum_trace: solid(0x808080),
        curves: Curves {
            response: red,
            phase: hex(0x85a3b0),
            peak_hold: gray(195),
        },
        ..Theme::default()
    };
    theme.panel.parameter = gray(213);
    theme.panel.label = hex(0x3a3b3c);
    theme.panel.muted_label = gray(88);
    theme.panel.accent = hex(0x676c71);
    metal(
        &mut theme.knob.colors,
        [0xe2e2e2, 0xc9c9c9],
        [0xf6f6f6, 0x7c7c7c],
        0x3a3b3c,
    );
    theme.knob.colors.caption = gray(115);
    theme.knob.colors.value = gray(112);
    theme.knob.colors.ticks = gray(139);
    theme.meter.low = gray(142);
    theme.meter.medium = gray(172);
    theme.meter.high = gray(205);
    theme.region.fill(hex(0x5f9eb8));
    theme
}

fn in_warm() -> Theme {
    let sand = hex(0xe6d9c8);
    let ink = hex(0x2b2621);
    let blue = hex(0x24637d);
    let light_blue = hex(0x7bc8ee);
    let mut theme = Theme {
        background: Background {
            top: hex(0xece1d3),
            bottom: hex(0xd8c9b8),
            header_top: hex(0xf8efe3),
            header_bottom: sand,
            footer_top: sand,
            footer_bottom: hex(0xcdbba6),
            footer_border: hex(0xa19381),
        },
        grid: Grid {
            major: hex(0xb1a392),
            minor: hex(0xccc0b0),
            zero: hex(0x8c7d6c),
        },
        spectrum_fill: solid(0x77b7d4),
        spectrum_trace: solid(0x326d89),
        curves: Curves {
            response: blue,
            phase: hex(0x765637),
            peak_hold: hex(0x34748f),
        },
        ..Theme::default()
    };
    theme.text = Text {
        primary: ink,
        muted: hex(0x66594c),
        body: ink,
        value: hex(0x514234),
        heading: hex(0x544739),
        spectrum_scale: hex(0x66594c),
    };
    theme.panel = Panel {
        parameter: sand,
        label: ink,
        muted_label: theme.text.muted,
        top: hex(0xeee3d4),
        bottom: sand,
        menu: hex(0xf1e7d7),
        border: hex(0xa19381),
        option_border: hex(0xa19381),
        option: hex(0xe0d2c0),
        hover: hex(0xd5c4ae),
        selected: hex(0xccb99f),
        marker: hex(0xf3eadc),
        accent: blue,
    };
    metal(
        &mut theme.knob.colors,
        [0xfcf3e6, 0xd6c8b5],
        [0xe8dbc9, 0x827263],
        0x29241f,
    );
    theme.knob.colors.caption = ink;
    theme.knob.colors.value = theme.text.value;
    theme.knob.colors.ticks = hex(0x6d6154);
    theme.knob.colors.gain_arc = blue;
    theme.meter.well = hex(0xeee0d1);
    theme.meter.track = hex(0xe2dbd4);
    theme.meter.edge = hex(0x8a776b);
    theme.meter.low = hex(0x4d91b1);
    theme.meter.medium = hex(0x71b2c6);
    theme.meter.high = hex(0x89dadb);
    theme.meter.compact_level = light_blue;
    theme.piano = Piano {
        well: hex(0xd7cabb),
        white_top: hex(0xfff1e0),
        white_bottom: hex(0xcabeab),
        black: hex(0x6d6156),
    };
    theme.scrollbar.track = hex(0xcfbeab);
    theme.scrollbar.thumb = hex(0x8c7e70);
    theme.scrollbar.hover = hex(0x776a5e);
    theme.scrollbar.active = hex(0x62564a);
    theme.band = theme.band.map(|color| color.lerp_to_gamma(ink, 0.32));
    theme.band[0] = blue;
    theme.region.fill(hex(0x42748a));
    theme
}
