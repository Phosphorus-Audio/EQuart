use super::*;
use controls::card_plate;

const GITHUB_URL: &str = "https://github.com/Phosphorus-Audio/EQuart";

const CARD_W: f32 = 312.0;
const CARD_H: f32 = 166.0;
const CARD_IN_S: f32 = 0.95;
const CARD_OUT_S: f32 = 0.24;
const ITEM_IN_S: f32 = 1.05;
const STAGGER_S: f32 = 0.10;
const SLIDE_PX: f32 = 46.0;

#[derive(Default)]
pub(super) struct About {
    opened_at: Option<f64>,
    closed_at: Option<f64>,
}

impl About {
    pub(super) fn open(&mut self, now: f64) {
        self.opened_at = Some(now);
        self.closed_at = None;
    }

    pub(super) fn close(&mut self, now: f64) {
        if self.opened_at.is_some() && self.closed_at.is_none() {
            self.closed_at = Some(now);
        }
    }

    pub(super) fn shut(&mut self) {
        self.opened_at = None;
        self.closed_at = None;
    }

    pub(super) fn is_shown(&self) -> bool {
        self.opened_at.is_some()
    }

    pub(super) fn is_animating(&self, now: f64) -> bool {
        let Some(opened_at) = self.opened_at else {
            return false;
        };

        if self.closed_at.is_some() {
            return true;
        }

        let total_in = f64::from(STAGGER_S * 3.0 + ITEM_IN_S);
        now - opened_at < total_in
    }
}

fn item_motion(elapsed: f32, index: usize) -> (f32, f32, f32) {
    let delay = index as f32 * STAGGER_S;
    let local = ((elapsed - delay) / ITEM_IN_S).clamp(0.0, 1.0);
    let ease = egui::emath::easing::exponential_out(local);
    let offset_x = -(1.0 - ease) * SLIDE_PX;
    let alpha = (local * 3.2).clamp(0.0, 1.0);

    (ease, offset_x, alpha)
}

fn open_url(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("rundll32");
        command.args(["url.dll,FileProtocolHandler", url]);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg(url);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(url);
        command
    };

    command.spawn().map(drop)
}

impl EquartEditor {
    pub(super) fn about_dialog(&mut self, ctx: &egui::Context, canvas: Rect) {
        let Some(opened_at) = self.about.opened_at else {
            return;
        };

        let now = ctx.input(|input| input.time);
        let elapsed = (now - opened_at).max(0.0) as f32;

        let out_t = self
            .about
            .closed_at
            .map_or(0.0, |closed| ((now - closed) as f32 / CARD_OUT_S).clamp(0.0, 1.0));

        if out_t >= 1.0 {
            self.about.shut();
            return;
        }

        let out_alpha = 1.0 - egui::emath::easing::cubic_in(out_t);
        let card_progress = egui::emath::easing::exponential_out((elapsed / CARD_IN_S).clamp(0.0, 1.0));
        let card_scale = (0.92 + 0.08 * card_progress) * (1.0 - 0.05 * out_t);

        let palette = theme::current(ctx);
        let accent = self.accent();
        let mut should_close = self.about.closed_at.is_none()
            && ctx.input(|input| input.key_pressed(egui::Key::Escape));
        let mut open_github = false;

        egui::Area::new(egui::Id::unique("equart-about"))
            .order(egui::Order::Foreground)
            .fixed_pos(canvas.min)
            .show(ctx, |ui| {
                ui.set_min_size(canvas.size());

                let scrim = ui.interact(
                    canvas,
                    ui.make_persistent_id("equart-about-scrim"),
                    Sense::click(),
                );
                let scrim_alpha = (118.0 * card_progress * out_alpha) as u8;
                ui.painter()
                    .rect_filled(canvas, 0.0, Color32::from_black_alpha(scrim_alpha));

                let card = Rect::from_center_size(
                    canvas.center(),
                    Vec2::new(CARD_W, CARD_H) * card_scale,
                );
                let _ = ui.interact(
                    card,
                    ui.make_persistent_id("equart-about-card"),
                    Sense::click(),
                );

                if scrim.clicked() && self.about.closed_at.is_none() {
                    should_close = true;
                }

                card_plate(ui, card, card_progress * out_alpha, accent);

                let close_rect = Rect::from_center_size(
                    Pos2::new(card.right() - 20.0, card.top() + 20.0),
                    Vec2::splat(22.0),
                );
                let close_resp = ui
                    .interact(
                        close_rect,
                        ui.make_persistent_id("equart-about-close"),
                        Sense::click(),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                let close_hover = hover_amount(&close_resp);
                let close_ink = palette
                    .text
                    .muted
                    .lerp_to_gamma(palette.text.primary, close_hover)
                    .gamma_multiply(card_progress * out_alpha);

                icons::draw(
                    ui.painter(),
                    ctx,
                    icons::Glyph::Close,
                    Rect::from_center_size(close_rect.center(), Vec2::splat(11.0)),
                    close_ink,
                );

                if close_resp.clicked() && self.about.closed_at.is_none() {
                    should_close = true;
                }

                let painter = ui.painter().with_clip_rect(card.shrink(1.0));
                let pad_x = 26.0;
                let left = card.left() + pad_x;
                let right = card.right() - pad_x;

                let (logo_ease, logo_dx, logo_alpha) = item_motion(elapsed, 0);
                let logo_a = logo_alpha * out_alpha;
                let logo_size = 40.0;
                let logo_pos = Pos2::new(left + logo_dx, card.top() + 26.0);
                let logo_angle = std::f32::consts::TAU * logo_ease;
                let title_x = left + logo_size + 14.0;

                logo::draw_logo_rotated(
                    &painter,
                    logo_pos,
                    logo_size,
                    logo_angle,
                    palette.text.primary.gamma_multiply(logo_a),
                );
                painter.text(
                    Pos2::new(title_x + logo_dx, logo_pos.y + 12.0),
                    egui::Align2::LEFT_CENTER,
                    crate::NAME,
                    FontId::new(21.0, egui::FontFamily::Name(GLOBAL_FONT.into())),
                    palette.text.primary.gamma_multiply(logo_a),
                );

                let (_, ver_dx, ver_alpha) = item_motion(elapsed, 1);
                let ver_a = ver_alpha * out_alpha;
                painter.text(
                    Pos2::new(title_x + ver_dx, logo_pos.y + 32.0),
                    egui::Align2::LEFT_CENTER,
                    format!("v{}", env!("CARGO_PKG_VERSION")),
                    FontId::proportional(11.5),
                    palette.text.muted.gamma_multiply(ver_a),
                );

                let divider_y = card.top() + 86.0;
                painter.hline(
                    left..=right,
                    divider_y,
                    Stroke::new(
                        1.0,
                        palette
                            .panel
                            .border
                            .gamma_multiply(0.65 * card_progress * out_alpha),
                    ),
                );

                let bottom_y = (divider_y + card.bottom()) * 0.5;

                let (_, dev_dx, dev_alpha) = item_motion(elapsed, 2);
                let dev_a = dev_alpha * out_alpha;
                painter.text(
                    Pos2::new(left + dev_dx, bottom_y),
                    egui::Align2::LEFT_CENTER,
                    "Dev by sout & UnD3ath",
                    FontId::proportional(13.0),
                    palette.text.body.gamma_multiply(dev_a),
                );

                let (_, btn_dx, btn_alpha) = item_motion(elapsed, 3);
                let btn_a = btn_alpha * out_alpha;
                let btn_size = 32.0;
                let btn_rect = Rect::from_center_size(
                    Pos2::new(right - btn_size * 0.5 + btn_dx, bottom_y),
                    Vec2::splat(btn_size),
                );
                let btn_resp = ui
                    .interact(
                        btn_rect,
                        ui.make_persistent_id("equart-about-github"),
                        Sense::click(),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(GITHUB_URL);
                let btn_hover = hover_amount(&btn_resp);
                let btn_ink = palette
                    .text
                    .muted
                    .lerp_to_gamma(palette.text.primary, btn_hover)
                    .gamma_multiply(btn_a);

                if btn_hover > 0.0 {
                    painter.rect_filled(
                        btn_rect,
                        6.0,
                        menu::fade(palette.panel.hover, btn_hover * btn_a),
                    );
                }

                icons::draw(
                    &painter,
                    ctx,
                    icons::Glyph::GitHub,
                    Rect::from_center_size(btn_rect.center(), Vec2::splat(20.0)),
                    btn_ink,
                );

                if btn_resp.clicked() && self.about.closed_at.is_none() {
                    open_github = true;
                }
            });

        if open_github && let Err(error) = open_url(GITHUB_URL) {
            self.notify(format!("Could not open GitHub: {error}"), now);
        }

        if should_close {
            self.about.close(now);
        }

        if self.about.is_animating(now) {
            ctx.request_repaint();
        }
    }
}
