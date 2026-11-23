use super::*;
use crate::display_preferences::{HeatmapMode, HeatmapPreferences, theme::HeatmapPalette};

#[cfg(test)]
mod tests;

const COLUMNS: usize = 384;
const ROWS: usize = 3;

#[derive(Default)]
pub(in crate::editor) struct Heatmap {
    mapping: Option<Mapping>,
    frame: Option<Frame>,
    mesh: Arc<egui::Mesh>,
    visible: bool,
}

#[derive(Clone, Copy, PartialEq)]
struct Frame {
    revision: u64,
    rect: Rect,
    floor: f32,
    settings: HeatmapPreferences,
    palette: HeatmapPalette,
    held: bool,
}

#[derive(Clone, Copy, PartialEq)]
struct MappingKey {
    max_hz: f32,
    bin_hz: f32,
    bins: usize,
}

struct Mapping {
    key: MappingKey,
    spans: [BinSpan; COLUMNS + 1],
}

#[derive(Clone, Copy)]
struct BinSpan {
    center: f32,
    first: usize,
    end: usize,
}

impl Mapping {
    fn new(key: MappingKey, plot: &Plot) -> Self {
        let bin_at = |vertex: f32| {
            let x = egui::lerp(plot.rect.x_range(), vertex / COLUMNS as f32);
            plot.hz(x) / key.bin_hz
        };
        let spans = std::array::from_fn(|i| {
            let i = i as f32;
            let last = (key.bins - 1) as f32;
            BinSpan {
                center: bin_at(i).min(last),
                first: (bin_at(i - 0.5).ceil() as usize).min(key.bins),
                end: ((bin_at(i + 0.5).floor() as usize) + 1).min(key.bins),
            }
        });
        Self { key, spans }
    }
}

impl BinSpan {
    fn level(self, levels: &[f32], floor: f32) -> f32 {
        let valid = |db: f32| {
            if db.is_finite() {
                db.max(-floor)
            } else {
                -floor
            }
        };
        let a = self.center as usize;
        let b = (a + 1).min(levels.len() - 1);
        let interpolated = egui::lerp(valid(levels[a])..=valid(levels[b]), self.center.fract());

        // retain peaks when a display column spans several high-frequency bins
        (self.first..self.end).fold(interpolated, |peak, i| peak.max(valid(levels[i])))
    }
}

fn palette_color(stops: [Color32; 5], energy: f32) -> Color32 {
    let stop = energy * (stops.len() - 1) as f32;
    let first = (stop as usize).min(stops.len() - 2);
    stops[first].lerp_to_gamma(stops[first + 1], stop - first as f32)
}

fn geometry(mesh: &mut egui::Mesh, plot: Rect, mode: HeatmapMode) {
    let (top, bottom) = match mode {
        HeatmapMode::Full | HeatmapMode::Off => (plot.top(), plot.bottom()),
        HeatmapMode::Top => (plot.top(), plot.center().y),
        HeatmapMode::Bottom => (plot.center().y, plot.bottom()),
    };
    mesh.vertices.clear();
    mesh.vertices.reserve((COLUMNS + 1) * ROWS);

    for i in 0..=COLUMNS {
        let x = egui::lerp(plot.x_range(), i as f32 / COLUMNS as f32);

        for y in [top, (top + bottom) * 0.5, bottom] {
            mesh.colored_vertex(Pos2::new(x, y), Color32::TRANSPARENT);
        }
    }

    if mesh.indices.is_empty() {
        mesh.indices.reserve(COLUMNS * (ROWS - 1) * 6);

        for a in (0..(COLUMNS * ROWS) as u32).step_by(ROWS) {
            for a in [a, a + 1] {
                let b = a + ROWS as u32;
                mesh.add_triangle(a, b, a + 1);
                mesh.add_triangle(a + 1, b, b + 1);
            }
        }
    }
}

impl Heatmap {
    pub(super) fn draw(
        &mut self,
        painter: &egui::Painter,
        plot: &Plot,
        spectrum: &SpectrumFrame<'_>,
        hold: Hold,
        settings: HeatmapPreferences,
        theme: &Theme,
    ) {
        if settings.mode == HeatmapMode::Off || settings.opacity == 0 {
            return;
        }

        let held = matches!(hold, Hold::Smooth);
        let levels = if held {
            spectrum.output_hold_levels
        } else {
            spectrum.output_levels
        };
        if levels.is_empty() || !spectrum.bin_hz.is_finite() || spectrum.bin_hz <= 0.0 {
            return;
        }

        let palette = theme.heatmap_palette();
        let key = MappingKey {
            max_hz: plot.max_hz,
            bin_hz: spectrum.bin_hz,
            bins: levels.len(),
        };
        let remap = self
            .mapping
            .as_ref()
            .is_none_or(|mapping| mapping.key != key);
        let next = Frame {
            revision: spectrum.revision,
            rect: plot.rect,
            floor: plot.spectrum_floor(),
            settings,
            palette,
            held,
        };

        if remap || self.frame != Some(next) {
            #[cfg(feature = "analyzer-profile")]
            let _timer = crate::analyzer::profile::Timer::new(crate::analyzer::profile::HEATMAP);

            if remap {
                self.mapping = Some(Mapping::new(key, plot));
            }

            let mesh = Arc::make_mut(&mut self.mesh);
            if self.frame.is_none_or(|frame| {
                frame.rect != next.rect || frame.settings.mode != next.settings.mode
            }) {
                geometry(mesh, next.rect, next.settings.mode);
            }

            let mapping = self.mapping.as_ref().unwrap();
            let intensity = f32::from(settings.intensity.min(100)) * 0.01;
            let opacity = f32::from(settings.opacity.min(100)) * 0.01;
            let power = 2.2 - 1.55 * intensity;
            let stops = palette.stops();
            let heights = match settings.mode {
                HeatmapMode::Full | HeatmapMode::Off => [0.75, 1.0, 0.75],
                HeatmapMode::Top => [1.0, 0.72, 0.0],
                HeatmapMode::Bottom => [0.0, 0.72, 1.0],
            };
            self.visible = false;

            for (span, vertices) in mapping
                .spans
                .iter()
                .zip(mesh.vertices.chunks_exact_mut(ROWS))
            {
                let level = span.level(levels, next.floor);
                let energy = ((level + next.floor) / next.floor)
                    .clamp(0.0, 1.0)
                    .powf(power);
                let color = palette_color(stops, energy);
                self.visible |= energy > 0.0;

                for (vertex, height) in vertices.iter_mut().zip(heights) {
                    vertex.color = color.gamma_multiply(energy * opacity * height);
                }
            }

            self.frame = Some(next);
        }

        if self.visible {
            painter.add(egui::Shape::Mesh(Arc::clone(&self.mesh)));
        }
    }
}
