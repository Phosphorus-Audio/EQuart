use super::*;

pub(super) fn draw(
    p: &egui::Painter,
    outer: Rect,
    markers: &[(usize, Pos2)],
    controls: &[BandControl; BAND_COUNT],
    priority: [Option<usize>; 3],
) {
    let palette = theme::current(p.ctx());
    let mut badges: Vec<_> = markers
        .iter()
        .copied()
        .filter(|(index, _)| controls[*index].channel != ChannelMode::Stereo)
        .collect();
    badges.sort_unstable_by_key(|(index, _)| {
        (
            priority
                .iter()
                .position(|&band| band == Some(*index))
                .unwrap_or(priority.len()),
            *index,
        )
    });

    let mut occupied: Vec<Rect> = Vec::with_capacity(badges.len());
    let bounds = outer.shrink(1.0);
    let size = Vec2::new(11.0, 11.0);

    for (index, node) in badges {
        let control = controls[index];
        let candidates = (0..6).flat_map(|ring| {
            [0.0, -1.0, 1.0, -2.0, 2.0, -3.0, 3.0]
                .into_iter()
                .flat_map(move |y| {
                    [-1.0, 1.0].map(move |side| {
                        let x = side * (16.0 + ring as f32 * 13.0);
                        Rect::from_center_size(node + Vec2::new(-x, y * 13.0), size)
                    })
                })
        });
        let Some(rect) = candidates.into_iter().find(|rect| {
            let margin = rect.expand(1.0);
            bounds.contains_rect(*rect)
                && !occupied.iter().any(|other| other.intersects(margin))
                && !markers.iter().any(|(_, point)| {
                    Rect::from_center_size(*point, Vec2::splat(20.0)).intersects(margin)
                })
        }) else {
            continue;
        };

        let color = palette.curves.channel(control.channel);
        let ink = color.gamma_multiply(if control.enabled { 1.0 } else { 0.65 });
        p.rect_filled(rect, 3.0, palette.panel.marker);
        p.rect_stroke(rect, 3.0, Stroke::new(0.8, ink), egui::StrokeKind::Inside);
        p.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            control.channel.tag(),
            FontId::monospace(8.5),
            ink,
        );
        occupied.push(rect);
    }
}
