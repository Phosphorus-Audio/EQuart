use super::*;


pub(super) fn gradient_fill(
    p: &egui::Painter,
    points: &[Pos2],
    reach: impl Fn(usize, f32) -> f32,
    stops: &[f32],
    colors: impl Fn(usize, usize) -> Color32,
) {
    let n = stops.len() as u32;
    let mut mesh = egui::Mesh::default();

    for (i, point) in points.iter().enumerate() {
        let span = reach(i, point.y);
        for (s, &t) in stops.iter().enumerate() {
            mesh.colored_vertex(Pos2::new(point.x, point.y + span * t), colors(i, s));
        }
    }

    for i in 0..points.len().saturating_sub(1) as u32 {
        for s in 0..n.saturating_sub(1) {
            let a = i * n + s;
            let b = a + n;
            mesh.add_triangle(a, a + 1, b);
            mesh.add_triangle(a + 1, b + 1, b);
        }
    }

    p.add(egui::Shape::mesh(mesh));
}
