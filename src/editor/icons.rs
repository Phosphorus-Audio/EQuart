//! Baked 32x32 alpha icon masks (tinted at draw time)

use egui::{Color32, ColorImage, Context, Rect, TextureHandle, TextureId, TextureOptions};

pub(crate) const SIZE: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Glyph {
    Bypass,
    Scale,
    Options,
    Close,
    Delete,
    Add,
    ModifierMenu,
    ToRegion,
    ToBand,
}

impl Glyph {
    const ALL: [Self; 9] = [
        Self::Bypass,
        Self::Scale,
        Self::Options,
        Self::Close,
        Self::Delete,
        Self::Add,
        Self::ModifierMenu,
        Self::ToRegion,
        Self::ToBand,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Bypass => "equart-icon-bypass",
            Self::Scale => "equart-icon-scale",
            Self::Options => "equart-icon-options",
            Self::Close => "equart-icon-band-close",
            Self::Delete => "equart-icon-band-delete",
            Self::Add => "equart-icon-band-add",
            Self::ModifierMenu => "equart-icon-modifier-menu",
            Self::ToRegion => "equart-icon-to-region",
            Self::ToBand => "equart-icon-to-band",
        }
    }

    fn mask(self) -> &'static [u8] {
        match self {
            Self::Bypass => include_bytes!("../../assets/icons/bypass.alpha"),
            Self::Scale => include_bytes!("../../assets/icons/scale.alpha"),
            Self::Options => include_bytes!("../../assets/icons/options.alpha"),
            Self::Close => include_bytes!("../../assets/icons/x-bold.alpha"),
            Self::Delete => include_bytes!("../../assets/icons/trash-simple-bold.alpha"),
            Self::Add => include_bytes!("../../assets/icons/plus-bold.alpha"),
            Self::ModifierMenu => include_bytes!("../../assets/icons/dots-six-bold.alpha"),
            Self::ToRegion => {
                include_bytes!("../../assets/icons/align-center-horizontal-simple-bold.alpha")
            }
            Self::ToBand => include_bytes!("../../assets/icons/circle-bold.alpha"),
        }
    }
}

#[derive(Clone)]
pub(crate) struct Icons {
    handles: [TextureHandle; Glyph::ALL.len()],
}

impl Icons {
    fn load(ctx: &Context) -> Self {
        Self {
            handles: Glyph::ALL.map(|glyph| {
                let pixels = glyph
                    .mask()
                    .iter()
                    .map(|&alpha| Color32::from_white_alpha(alpha))
                    .collect();
                ctx.load_texture(
                    glyph.name(),
                    ColorImage::new([SIZE, SIZE], pixels),
                    TextureOptions::LINEAR,
                )
            }),
        }
    }

    pub(crate) fn id(&self, glyph: Glyph) -> TextureId {
        self.handles[glyph as usize].id()
    }
}

/// cache icon textures in egui context so we don't need &mut editor
pub(crate) fn textures(ctx: &Context) -> Icons {
    let id = egui::Id::unique("equart-icons");
    if let Some(icons) = ctx.data(|data| data.get_temp::<Icons>(id)) {
        return icons;
    }
    let icons = Icons::load(ctx);
    ctx.data_mut(|data| data.insert_temp(id, icons.clone()));
    icons
}

pub(crate) fn draw(
    painter: &egui::Painter,
    ctx: &Context,
    glyph: Glyph,
    rect: Rect,
    color: Color32,
) {
    let icons = textures(ctx);
    painter.image(
        icons.id(glyph),
        rect,
        Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        color,
    );
}
