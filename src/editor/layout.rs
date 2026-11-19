use super::*;

pub(crate) struct Layout;

impl Layout {
    pub(crate) const HEADER: f32 = 44.0;
    pub(crate) const FOOTER: f32 = 28.0;
    /// right-side output meter width
    pub(crate) const METER_W: f32 = 24.0;
    /// spectrum area (starts at top behind translucent header)
    pub(crate) const SPECTRUM: [f32; 4] =
        [0.0, 0.0, CANVAS_W - Self::METER_W, CANVAS_H - Self::FOOTER];

    /// spectrum rect without meter and footer
    pub(crate) fn plot_outer(canvas: Rect) -> Rect {
        Rect::from_min_max(
            canvas.min + Vec2::new(Self::SPECTRUM[0], Self::SPECTRUM[1]),
            Pos2::new(canvas.max.x - Self::METER_W, canvas.max.y - Self::FOOTER),
        )
    }
    pub(crate) const PANEL_W: f32 = 468.0;
    pub(crate) const PANEL_H: f32 = 124.0;
    /// gap between panel bottom and spectrum bottom
    const PANEL_BOTTOM_GAP: f32 = 70.0;

    /// clamp focus_x so panel stays on screen (target for smooth glide)
    pub(crate) fn panel_focus_x(canvas: Rect, focus_x: f32) -> f32 {
        Self::panel_left(canvas, focus_x, Self::PANEL_W) + Self::PANEL_W * 0.5
    }

    fn panel_left(canvas: Rect, focus_x: f32, width: f32) -> f32 {
        let margin = 12.0;
        let min_x = canvas.left() + margin;
        let max_x = (canvas.right() - width - margin).max(min_x);
        (focus_x - width * 0.5).clamp(min_x, max_x)
    }

    /// floating panel rect, clamped inside canvas
    pub(crate) fn panel(canvas: Rect, focus_x: f32) -> Rect {
        Self::panel_with_width(canvas, focus_x, Self::PANEL_W)
    }

    pub(super) fn panel_with_width(canvas: Rect, focus_x: f32, width: f32) -> Rect {
        let x = Self::panel_left(canvas, focus_x, width);
        let bottom = canvas.bottom() - Self::FOOTER - Self::PANEL_BOTTOM_GAP;
        Rect::from_min_size(
            Pos2::new(x, bottom - Self::PANEL_H),
            Vec2::new(width, Self::PANEL_H),
        )
    }
}
