use super::*;

/// How long the card takes to unfold, and how long the white label plates take to fade
pub(super) const SETTINGS_ARRIVAL_S: f32 = 0.75;
const SETTINGS_FLASH_S: f32 = 0.9;
/// The scale never reaches exactly zero，else egui will crash
const SETTINGS_MIN_SCALE: f32 = 0.02;
/// How long the card takes to drop away once it is dismissed
const SETTINGS_FALL_S: f32 = 0.28;
const SETTINGS_FALL_DROP: f32 = 108.0;
const SETTINGS_FALL_SLIDE: f32 = 12.0;
/// How far the card tips on the way down, in radians
const SETTINGS_FALL_TIP_MIN: f32 = 0.035;
const SETTINGS_FALL_TIP_MAX: f32 = 0.13;

// why?
const _: () = assert!(SETTINGS_FALL_S <= 0.3);

impl EquartEditor {
    pub(in crate::editor) fn animated_card(
        popup: egui::Popup<'_>,
        life: &mut CardLife,
        now: f64,
        pivot: Option<Pos2>,
        draw: impl FnOnce(&mut egui::Ui, f32, f32, Option<Fall>) -> bool,
    ) -> bool {
        let speed = animation::speed(popup.ctx());

        if life.landed(now, speed) {
            life.shut();
        }

        if !life.is_shown() {
            return false;
        }

        let (progress, flash) = life.arrival(now, speed);
        let fall = life.falling(now, speed);
        let pose = card_pose(progress, fall);
        let mut asked_to_close = false;
        let response = popup.interactable(fall.is_none()).show(|ui| {
            asked_to_close = draw(ui, progress, flash, fall);
        });
        if let (Some(pose), Some(response)) = (pose, &response) {
            // The footer cards
            pose_card(
                &response.response.ctx,
                response.response.layer_id,
                pivot.unwrap_or_else(|| response.response.rect.left_bottom()),
                pose,
            );
            response.response.ctx.request_repaint();
        }
        if let Some(shown) = &response {
            if fall.is_some() || flash < 1.0 {
                shown.response.ctx.request_repaint();
            }
        }
        let dismissed = response.as_ref().is_some_and(|r| r.response.should_close());
        dismissed || asked_to_close
    }
}


fn unfold_scale(progress: f32) -> f32 {
    SETTINGS_MIN_SCALE + (1.0 - SETTINGS_MIN_SCALE) * progress.clamp(0.0, 1.0)
}

#[derive(Clone, Copy, Debug)]
pub(in crate::editor) struct Fall {
    offset: Vec2,
    tip: f32,
    fade: f32,
}

#[derive(Clone, Copy, Debug)]
struct CardFall {
    released_at: f64,
    tip: f32,
}

impl CardFall {
    /// How far through the fall we are, `0..=1`.
    fn progress(&self, now: f64, speed: AnimationSpeed) -> f32 {
        (speed.elapsed(now - self.released_at) as f32 / SETTINGS_FALL_S).clamp(0.0, 1.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Pose {
    scale: f32,
    rotation: f32,
    offset: Vec2,
    fade: f32,
}

impl Pose {
    const REST: Self = Self {
        scale: 1.0,
        rotation: 0.0,
        offset: Vec2::ZERO,
        fade: 1.0,
    };

    fn is_rest(&self) -> bool {
        *self == Self::REST
    }

    fn apply(&self, pivot: Pos2, point: Pos2) -> Pos2 {
        pivot
            + egui::emath::Rot2::from_angle(self.rotation) * ((point - pivot) * self.scale)
            + self.offset
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(in crate::editor) struct CardLife(CardState);

#[derive(Clone, Copy, Debug, Default)]
enum CardState {
    #[default]
    Closed,
    Open {
        opened_at: f64,
    },
    Falling {
        fall: CardFall,
    },
}

impl CardLife {
    /// Nothing on screen, and nothing left over from last time
    pub(in crate::editor) const SHUT: Self = Self(CardState::Closed);

    pub(in crate::editor) fn is_up(&self) -> bool {
        matches!(self.0, CardState::Open { .. })
    }

    /// Whether there is anything left to draw
    pub(in crate::editor) fn is_shown(&self) -> bool {
        !matches!(self.0, CardState::Closed)
    }

    pub(in crate::editor) fn open(&mut self, now: f64) {
        self.0 = CardState::Open { opened_at: now };
    }

    pub(in crate::editor) fn release(&mut self, now: f64) {
        if !self.is_up() {
            return;
        }

        self.0 = CardState::Falling {
            fall: CardFall {
                released_at: now,
                tip: tip_at(now),
            },
        };
    }

    /// Shut for good, once the fall has played out.
    pub(in crate::editor) fn shut(&mut self) {
        *self = Self::SHUT;
    }

    fn arrival(&self, now: f64, speed: AnimationSpeed) -> (f32, f32) {
        let CardState::Open { opened_at } = self.0 else {
            return (1.0, 1.0);
        };

        let elapsed = speed.elapsed(now - opened_at) as f32;
        let eased =
            |seconds: f32, curve: fn(f32) -> f32| curve((elapsed / seconds).clamp(0.0, 1.0));
        (
            eased(SETTINGS_ARRIVAL_S, egui::emath::easing::exponential_out),
            eased(SETTINGS_FLASH_S, egui::emath::easing::cubic_out),
        )
    }

    fn falling(&self, now: f64, speed: AnimationSpeed) -> Option<Fall> {
        match self.0 {
            CardState::Falling { fall } => Some(fall_at(fall.progress(now, speed), fall.tip)),
            CardState::Closed | CardState::Open { .. } => None,
        }
    }

    fn landed(&self, now: f64, speed: AnimationSpeed) -> bool {
        matches!(self.0, CardState::Falling { fall } if fall.progress(now, speed) >= 1.0)
    }
}

fn card_pose(progress: f32, fall: Option<Fall>) -> Option<Pose> {
    let pose = match fall {
        Some(fall) => Pose {
            scale: 1.0,
            rotation: fall.tip,
            offset: fall.offset,
            fade: fall.fade,
        },
        None => Pose {
            scale: unfold_scale(progress),
            ..Pose::REST
        },
    };
    (!pose.is_rest()).then_some(pose)
}

fn fall_at(t: f32, tip: f32) -> Fall {
    let t = t.clamp(0.0, 1.0);
    let drop = t * t;
    Fall {
        offset: Vec2::new(
            SETTINGS_FALL_SLIDE * drop * tip / SETTINGS_FALL_TIP_MAX,
            SETTINGS_FALL_DROP * drop,
        ),
        tip: tip * t,
        fade: 1.0 - t,
    }
}

fn tip_at(released_at: f64) -> f32 {
    let hash = mix((released_at * 1_000.0) as u64);
    let size = (hash >> 40) as f32 / (1u64 << 24) as f32;
    let magnitude = SETTINGS_FALL_TIP_MIN + (SETTINGS_FALL_TIP_MAX - SETTINGS_FALL_TIP_MIN) * size;
    if hash & 1 == 0 { -magnitude } else { magnitude }
}

/// a freaking splitmix64's finalizer wrote by gpt: a millisecond clock differs in a few low bits from
/// one dismissal to the next, and this is enough to spread those over the
/// whole word.
fn mix(x: u64) -> u64 {
    let mut x = x;
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 33;
    x = x.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    x ^= x >> 33;
    x
}


fn pose_card(ctx: &egui::Context, layer: egui::LayerId, pivot: Pos2, pose: Pose) {
    let viewport = ctx.viewport_rect();
    let options = ctx.tessellation_options(|options| *options);
    let font_size = ctx.fonts(|fonts| fonts.font_image_size());

    let mut batches: Vec<(Rect, egui::epaint::Mesh)> = Vec::new();
    let mut tessellator =
        egui::epaint::Tessellator::new(ctx.pixels_per_point(), options, font_size, Vec::new());

    ctx.graphics_mut(|graphics| {
        let Some(list) = graphics.get_mut(layer) else {
            return;
        };

        for index in 0..list.next_idx().0 {
            list.mutate_shape(egui::layers::ShapeIdx(index), |clipped| {
                let clip_rect = clipped.clip_rect;
                let shape = std::mem::replace(&mut clipped.shape, egui::Shape::Noop);
                let mut mesh = egui::epaint::Mesh::default();
                tessellator.tessellate_shape(shape, &mut mesh);

                if mesh.vertices.is_empty() {
                    return;
                }

                match batches.last_mut() {
                    Some((last_clip, last_mesh))
                        if *last_clip == clip_rect && last_mesh.texture_id == mesh.texture_id =>
                    {
                        last_mesh.append(mesh);
                    }
                    _ => batches.push((clip_rect, mesh)),
                }
            });
        }

        for (clip_rect, mut mesh) in batches {
            for vertex in &mut mesh.vertices {
                vertex.pos = pose.apply(pivot, vertex.pos);
                vertex.color = vertex.color.gamma_multiply(pose.fade);
            }

            let clip = if clip_rect.is_finite() {
                let corners = [
                    pose.apply(pivot, clip_rect.left_top()),
                    pose.apply(pivot, clip_rect.right_top()),
                    pose.apply(pivot, clip_rect.right_bottom()),
                    pose.apply(pivot, clip_rect.left_bottom()),
                ];
                Rect::from_points(&corners).intersect(viewport)
            } else {
                viewport
            };

            if clip.is_positive() {
                list.add(clip, egui::Shape::Mesh(mesh.into()));
            }
        }
    });
}
