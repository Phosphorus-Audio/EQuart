use super::*;

/// Wait this long after dragging or editing before card starts moving.
const PANEL_HOLD_SECONDS: f64 = 0.25;
/// Smooth-damp time for panel glide.
const PANEL_SMOOTH_TIME: f32 = 0.16;
/// Cap frame delta so lag spikes won't make the card teleport.
const PANEL_MAX_FRAME_DELTA: f64 = 0.05;
/// Snap to target and stop moving when close enough.
const PANEL_SNAP_DISTANCE: f32 = 0.25;
const PANEL_SNAP_SPEED: f32 = 0.6;
#[derive(Clone, Copy)]
struct PanelMotion {
    /// Current x and speed (already clamped on screen).
    current: f32,
    velocity: f32,
    target: f32,
    /// Last frame node x, helps tell edit apart from window resize.
    raw: f32,
    /// Last time target moved or mouse was down.
    last_change: f64,
    last_active: f64,
    /// Last frame timestamp for dt.
    last_frame: f64,
    /// Was mouse down last frame?
    was_down: bool,
    pending: bool,
    /// Snap directly if band or width changed.
    band: Option<usize>,
    width: f32,
}

fn panel_motion_id(ui: &egui::Ui) -> egui::Id {
    ui.make_persistent_id("panel-anchor-motion")
}

/// Smooth-damp spring towards target, no overshoot.
fn smooth_damp(current: f32, target: f32, velocity: &mut f32, smooth_time: f32, dt: f32) -> f32 {
    if dt <= 0.0 {
        return current;
    }
    let omega = 2.0 / smooth_time;
    let x = omega * dt;
    let decay = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
    let change = current - target;
    let temp = (*velocity + omega * change) * dt;
    *velocity = (*velocity - omega * temp) * decay;
    let output = target + (change + temp) * decay;
    // Don't overshoot target even on big dt.
    if (target - current) * (target - output) < 0.0 {
        *velocity = 0.0;
        target
    } else {
        output
    }
}

impl EquartEditor {
    /// Get raw x of selected band, or canvas center if nothing to follow.
    fn panel_target_x(&self, canvas: Rect) -> f32 {
        let Some(index) = self.selected else {
            return canvas.center().x;
        };

        let control = self.control(index);
        if !control.enabled && !self.placed(index) {
            return canvas.center().x;
        }

        let outer = Layout::plot_outer(canvas, self.meter_visible());
        let plot = Plot::new(outer, self.telemetry.sample_rate());
        plot.x(control.frequency)
    }

    /// Where the card actually draws. Waits 1s after editing/mouse up, then smoothly glides to the band.
    /// Using smooth-damp spring with clamped target so it won't get stuck at screen edge.
    pub(crate) fn panel_anchor_x(&self, ui: &egui::Ui, canvas: Rect) -> f32 {
        let now = ui.input(|i| i.time);
        let pointer_down = ui.input(|i| i.pointer.primary_down());
        let raw = self.panel_target_x(canvas);
        let desired = Layout::panel_focus_x(canvas, raw);
        let id = panel_motion_id(ui);
        let smooth_time = animation::speed(ui.ctx()).duration(PANEL_SMOOTH_TIME);

        ui.data_mut(|data| {
            let mut m = data.get_temp::<PanelMotion>(id).unwrap_or(PanelMotion {
                current: desired,
                velocity: 0.0,
                target: desired,
                raw,
                last_change: now,
                last_active: now,
                last_frame: now,
                was_down: pointer_down,
                pending: false,
                band: self.selected,
                width: canvas.width(),
            });

            // Window resize moves node x too, don't treat it as an edit.
            let resized = (m.width - canvas.width()).abs() > 0.5;
            m.width = canvas.width();

            // Snap immediately when switching bands.
            if m.band != self.selected {
                m.current = desired;
                m.target = desired;
                m.velocity = 0.0;
                m.last_change = now;
                m.pending = false;
                m.band = self.selected;
            }

            // Keep pushing hold timer while mouse is down (and on release).
            if pointer_down || m.was_down {
                m.last_active = now;
            }
            m.was_down = pointer_down;

            let settle = m.last_change.max(m.last_active);
            let gliding = m.pending && now - settle >= PANEL_HOLD_SECONDS;

            // Restart wait timer if node moved, unless we are already gliding.
            if !resized && !gliding && (raw - m.raw).abs() > 0.25 {
                m.last_change = now;
                m.velocity = 0.0;
            }
            m.raw = raw;

            if (desired - m.target).abs() > 0.25 {
                m.target = desired;
                m.pending = true;
            }

            // Keep current pos inside screen bounds after resize.
            m.current = Layout::panel_focus_x(canvas, m.current);

            let dt = (now - m.last_frame).clamp(0.0, PANEL_MAX_FRAME_DELTA) as f32;
            m.last_frame = now;

            let held = now - m.last_change.max(m.last_active) < PANEL_HOLD_SECONDS;
            if m.pending && (!held || (smooth_time == 0.0 && !pointer_down)) {
                let remaining = m.target - m.current;
                if smooth_time == 0.0
                    || (remaining.abs() <= PANEL_SNAP_DISTANCE
                        && m.velocity.abs() <= PANEL_SNAP_SPEED)
                {
                    m.current = m.target;
                    m.velocity = 0.0;
                    m.pending = false;
                } else {
                    m.current = smooth_damp(m.current, m.target, &mut m.velocity, smooth_time, dt);
                }
            }

            data.insert_temp(id, m);
            m.current
        })
    }
}
