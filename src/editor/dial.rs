//! Half-disc fan menu for axis dB scales (no plate, fades at rim)

use super::*;

const OUTER: f32 = 106.0;
const HUB: f32 = 34.0;
const GAP: f32 = 0.030;
/// where sector starts fading to transparent
const BODY: f32 = 0.90;
const BODY_LIT: f32 = 0.96;

/// fade angle at flat ends
const FEATHER: f32 = 0.07;

const ARRIVAL_S: f32 = 0.34;
const STAGGER_S: f32 = 0.045;
const EXIT_S: f32 = 0.15;
const HUB_FADE_S: f32 = 0.07;
const SPIN: f32 = 0.22;


/// start straight up, cover 180 deg
const START: f32 = -std::f32::consts::FRAC_PI_2;
const SPAN: f32 = std::f32::consts::FRAC_PI_2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Idle,
    Picked(usize),
    Dismissed,
}

/// which way the fan opens (always into plot)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Side {
    Right,
    Left,
}

impl Side {
    /// 1.0 for right, -1.0 for mirrored left
    fn mirror(self) -> f32 {
        match self {
            Self::Right => 1.0,
            Self::Left => -1.0,
        }
    }
}

/// state for the axis fan menu
#[derive(Clone, Copy, Debug)]
pub(super) struct Dial {
    anchor: Pos2,
    side: Side,
    opened_at: Option<f64>,
    closing_at: Option<f64>,
    /// wait for opening click release so it doesn't close immediately
    armed: bool,
}

impl Dial {
    pub(super) const SHUT: Self = Self {
        anchor: Pos2::ZERO,
        side: Side::Right,
        opened_at: None,
        closing_at: None,
        armed: false,
    };

    pub(super) fn open(&mut self, anchor: Pos2, now: f64, side: Side) {
        *self = Self {
            anchor,
            side,
            opened_at: Some(now),
            closing_at: None,
            armed: false,
        };
    }

    pub(super) fn is_shown(&self) -> bool {
        self.opened_at.is_some()
    }

    pub(super) fn is_up(&self) -> bool {
        self.opened_at.is_some() && self.closing_at.is_none()
    }

    pub(super) fn close(&mut self, now: f64) {
        if self.is_up() {
            self.closing_at = Some(now);
        }
    }

    pub(super) fn shut(&mut self) {
        *self = Self::SHUT;
    }

    /// keep fan center inside top/bottom bounds
    fn center(&self, canvas: Rect) -> Pos2 {
        let margin = OUTER + 2.0;
        let min = canvas.top() + margin;
        let max = canvas.bottom() - margin;
        let y = if min > max {
            canvas.center().y
        } else {
            self.anchor.y.clamp(min, max)
        };
        Pos2::new(self.anchor.x, y)
    }

    /// draw fan and handle clicks
    pub(super) fn show(
        &mut self,
        ui: &mut egui::Ui,
        canvas: Rect,
        labels: &[String],
        current: Option<usize>,
    ) -> Outcome {
        let Some(opened_at) = self.opened_at else {
            return Outcome::Idle;
        };
        let now = ui.input(|i| i.time);

        if self
            .closing_at
            .is_some_and(|at| now - at >= f64::from(EXIT_S))
        {
            self.shut();
            return Outcome::Idle;
        }

        let center = self.center(canvas);
        let leaving = self
            .closing_at
            .map(|at| (((now - at) as f32) / EXIT_S).clamp(0.0, 1.0));
        let mirror = self.side.mirror();
        let disc = Disc {
            center,
            mirror,
            elapsed: ((now - opened_at) as f32).max(0.0),
            leaving: leaving.unwrap_or(0.0),
            fade: 1.0 - leaving.unwrap_or(0.0),
        };
        // use full reach for hit-test even while opening
        let reach = OUTER * (1.0 - disc.leaving);

        // capture clicks inside fan bounds
        let bounds = Rect::from_min_max(
            center + Vec2::new((OUTER * mirror).min(0.0), -OUTER),
            center + Vec2::new((OUTER * mirror).max(0.0), OUTER),
        );
        let response = ui.interact(bounds, ui.scope_id().with("axis-dial"), Sense::click());

        let pointer = ui.input(|i| i.pointer.interact_pos());
        let hovered = pointer
            .filter(|_| disc.fade > 0.5)
            .and_then(|position| sector_at(position - center, labels.len(), reach, mirror));
        if hovered.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        let mut outcome = Outcome::Idle;
        if leaving.is_none() {
            if !self.armed {
                self.armed = !response.is_pointer_button_down_on();
            } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                outcome = Outcome::Dismissed;
            } else if ui.input(|i| i.pointer.any_click()) {
                outcome = match pointer
                    .and_then(|position| sector_at(position - center, labels.len(), reach, mirror))
                {
                    Some(index) => Outcome::Picked(index),
                    None => Outcome::Dismissed,
                };
            }
        }

        disc.paint(ui.painter_at(canvas), response.id, labels, current, hovered);
        if !matches!(outcome, Outcome::Idle) {
            self.close(now);
        }
        // editor already repaints while dial is shown
        outcome
    }
}

struct Disc {
    center: Pos2,
    mirror: f32,
    elapsed: f32,
    leaving: f32,
    fade: f32,
}

impl Disc {
    /// polar to screen pos (mirrors if opening left)
    fn place(&self, angle: f32, radius: f32) -> Pos2 {
        let offset = Vec2::angled(angle) * radius;
        self.center + Vec2::new(offset.x * self.mirror, offset.y)
    }

    fn paint(
        &self,
        p: egui::Painter,
        id: egui::Id,
        labels: &[String],
        current: Option<usize>,
        hovered: Option<usize>,
    ) {
        let palette = theme::current(p.ctx());
        let count = labels.len().max(1);
        // twist fan slightly while opening
        let total = ARRIVAL_S + STAGGER_S * count.saturating_sub(1) as f32;
        let spin = SPIN * (1.0 - arrival((self.elapsed / total).clamp(0.0, 1.0)));

        for (index, label) in labels.iter().enumerate() {
            let hover = hover_progress(p.ctx(), id.with(index), Some(index) == hovered);
            let (raw, clock) = sector_clock(self.elapsed, index);
            let reach = OUTER * clock * (1.0 - self.leaving) * 0.98;
            if reach <= HUB {
                continue;
            }
            let middle = sector_angle(index, count) + spin;
            // half angle of one sector
            let half = (std::f32::consts::PI / count as f32 * 0.5 - GAP) * clock;
            if half <= 0.0 {
                continue;
            }
            // wait till slice is wide enough for label
            let label_fade = ((raw - 0.40) / 0.35).clamp(0.0, 1.0) * self.fade;
            let chosen = Some(index) == current;
            let (color, text, body) = if chosen {
                (palette.panel.selected, palette.text.primary, BODY_LIT)
            } else {
                (
                    palette.panel.option.lerp_to_gamma(palette.panel.hover, hover),
                    palette.text.body.lerp_to_gamma(palette.text.primary, hover),
                    egui::lerp(BODY..=BODY_LIT, hover),
                )
            };
            p.add(egui::Shape::mesh(
                self.sector(middle, half, reach, color, body),
            ));
            if label_fade > 0.01 {
                let position = self.place(middle, reach * 0.70);
                p.text(
                    position,
                    egui::Align2::CENTER_CENTER,
                    label,
                    FontId::proportional(12.5),
                    text.gamma_multiply(label_fade),
                );
            }
        }

        self.paint_hub(&p, labels, current);
    }

    /// center hub showing current dB range
    fn paint_hub(&self, p: &egui::Painter, labels: &[String], current: Option<usize>) {
        let values = (self.elapsed / HUB_FADE_S).clamp(0.0, 1.0) * self.fade;
        if values <= 0.01 {
            return;
        }
        let palette = theme::current(p.ctx());
        let mesh = self.fan(
            0.0,
            SPAN,
            &[(0.0, 1.0), (HUB * 0.72, 0.95), (HUB, 0.30)],
            palette.panel.menu,
            values,
            0.0,
        );
        p.add(egui::Shape::mesh(mesh));

        let Some(current) = current else { return };
        let Some(label) = labels.get(current) else {
            return;
        };
        let middle = self.center + Vec2::new(HUB * 0.46 * self.mirror, 0.0);
        p.text(
            middle - Vec2::new(0.0, 4.0),
            egui::Align2::CENTER_CENTER,
            label,
            FontId::proportional(14.5),
            palette.text.primary.gamma_multiply(values),
        );
        p.text(
            middle + Vec2::new(0.0, 11.0),
            egui::Align2::CENTER_CENTER,
            "dB",
            FontId::new(7.0, egui::FontFamily::Name(GLOBAL_FONT.into())),
            palette.text.muted.gamma_multiply(values),
        );
    }

    /// sector mesh with faded rim & edges
    fn sector(&self, middle: f32, half: f32, reach: f32, color: Color32, body: f32) -> egui::Mesh {
        let rings = [(HUB * 0.9, 1.0), (reach * body, 1.0), (reach, 0.0)];
        self.fan(middle, half, &rings, color, self.fade, FEATHER)
    }

    /// build fan/hub mesh with vertex alpha fade
    fn fan(
        &self,
        middle: f32,
        half: f32,
        rings: &[(f32, f32)],
        color: Color32,
        fade: f32,
        feather: f32,
    ) -> egui::Mesh {
        let steps = ((half * 2.0 / 0.06).ceil() as usize).max(2);
        let angle_at = |step: usize| middle - half + 2.0 * half * step as f32 / steps as f32;

        // fade alpha near flat ends
        let ends = |angle: f32| {
            if feather <= 0.0 {
                return 1.0;
            }
            let inside = (SPAN - angle.abs()).max(0.0);
            (inside / feather).clamp(0.0, 1.0)
        };

        let mut mesh = egui::Mesh::default();
        let mut profiles: Vec<Vec<(Pos2, Color32)>> = Vec::with_capacity(rings.len());
        for &(radius, lit) in rings {
            let mut points = Vec::with_capacity(steps + 1);
            for step in 0..=steps {
                let angle = angle_at(step);
                let alpha = lit * ends(angle) * fade;
                points.push((self.place(angle, radius), color.gamma_multiply(alpha)));
            }
            profiles.push(points);
        }

        let vertex = |(pos, color): (Pos2, Color32)| egui::epaint::Vertex {
            pos,
            uv: egui::epaint::WHITE_UV,
            color,
        };
        for pair in profiles.windows(2) {
            let (inner, outer) = (&pair[0], &pair[1]);
            let base = mesh.vertices.len() as u32;
            for step in 0..=steps {
                mesh.vertices.push(vertex(inner[step]));
                mesh.vertices.push(vertex(outer[step]));
            }
            for step in 0..steps {
                let near = base + step as u32 * 2;
                mesh.indices
                    .extend([near, near + 2, near + 3, near, near + 3, near + 1]);
            }
        }
        mesh
    }
}

fn arrival(t: f32) -> f32 {
    egui::emath::easing::exponential_out(t.clamp(0.0, 1.0))
}

/// (raw, eased) arrival progress for sector i
fn sector_clock(elapsed: f32, index: usize) -> (f32, f32) {
    let raw = ((elapsed - STAGGER_S * index as f32) / ARRIVAL_S).clamp(0.0, 1.0);
    (raw, arrival(raw))
}

fn sector_angle(index: usize, count: usize) -> f32 {
    START + (index as f32 + 0.5) * (std::f32::consts::PI / count as f32)
}

/// hit-test sector under pointer (None in hub or outside)
fn sector_at(offset: Vec2, count: usize, radius: f32, mirror: f32) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let distance = offset.length();
    if distance < HUB || distance > radius {
        return None;
    }
    // mirror x into rightwards frame
    let angle = Vec2::new(offset.x * mirror, offset.y).angle();
    if !angle.is_nan() && !(START..=-START).contains(&angle) {
        return None;
    }
    let index = ((angle - START) / (std::f32::consts::PI / count as f32)).floor() as usize;
    Some(index.min(count - 1))
}
