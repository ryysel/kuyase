use eframe::egui::{Pos2, Rect};
pub use protocol::default_board_ring_leds;
pub use protocol::display::{
    build_segments_map_from_text as build_local_segments_from_text, render_text_to_matrix_21x12,
};
use protocol::BridgeStateSnapshot;
use std::collections::BTreeMap;
use std::f32::consts::{FRAC_PI_2, TAU};

pub fn led_ring_position(center: Pos2, radius: f32, index: usize, count: usize) -> Pos2 {
    if count <= 1 {
        return center;
    }
    let angle = -FRAC_PI_2 + TAU * (index as f32) / (count as f32);
    Pos2::new(
        center.x + radius * angle.cos(),
        center.y + radius * angle.sin(),
    )
}

pub fn doa_angle_from_pointer(center: Pos2, pointer: Pos2) -> u32 {
    let dx = pointer.x - center.x;
    let dy = pointer.y - center.y;
    let deg = dy.atan2(dx).to_degrees() + 90.0;
    (deg.rem_euclid(360.0).round() as u32) % 360
}

pub fn segment_brightness(segments: &BTreeMap<String, u8>, key: &str) -> u8 {
    segments.get(key).copied().unwrap_or(0)
}

pub fn board_has_segment_clock(slug: &str, st: &BridgeStateSnapshot) -> bool {
    st.display_kind == "segment_clock" || protocol::board_has_segment_clock(slug, st.has_clock)
}

pub fn board_has_matrix_21x12(slug: &str, st: &BridgeStateSnapshot) -> bool {
    protocol::board_has_matrix_21x12(slug, &st.display_kind)
}

fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let ap = p - a;
    let len_sq = ab.length_sq();
    if len_sq <= 1e-5 {
        return ap.length();
    }
    let t = (ap.dot(ab) / len_sq).clamp(0.0, 1.0);
    let proj = a + ab * t;
    p.distance(proj)
}

pub fn clock_segment_lines(rect: Rect) -> Vec<(String, Pos2, Pos2)> {
    let digit_w = 24.0;
    let digit_h = 40.0;
    let start_x = rect.center().x - 66.0;
    let top_y = rect.center().y - digit_h * 0.5;
    let ym = top_y + digit_h * 0.5;

    let mut out = Vec::with_capacity(30);
    let plus_cx = start_x - 16.0;
    out.push((
        "h1".to_string(),
        Pos2::new(plus_cx, ym - 7.0),
        Pos2::new(plus_cx, ym + 7.0),
    ));
    out.push((
        "m1".to_string(),
        Pos2::new(plus_cx - 7.0, ym),
        Pos2::new(plus_cx + 7.0, ym),
    ));

    for idx in 0..4 {
        let pos = idx + 1;
        let x0 = start_x + (idx as f32) * 34.0 + if idx >= 2 { 12.0 } else { 0.0 };
        let x1 = x0 + digit_w;
        let y1 = top_y + digit_h;
        let coords = [
            ("a", Pos2::new(x0 + 2.5, top_y), Pos2::new(x1 - 2.5, top_y)),
            ("b", Pos2::new(x1, top_y + 2.5), Pos2::new(x1, ym - 1.5)),
            ("c", Pos2::new(x1, ym + 1.5), Pos2::new(x1, y1 - 2.5)),
            ("d", Pos2::new(x0 + 2.5, y1), Pos2::new(x1 - 2.5, y1)),
            ("e", Pos2::new(x0, ym + 1.5), Pos2::new(x0, y1 - 2.5)),
            ("f", Pos2::new(x0, top_y + 2.5), Pos2::new(x0, ym - 1.5)),
            ("g", Pos2::new(x0 + 2.5, ym), Pos2::new(x1 - 2.5, ym)),
        ];
        for (part, p0, p1) in coords {
            out.push((format!("{}{}", part, pos), p0, p1));
        }
    }
    out
}

pub fn hit_test_clock_segment(rect: Rect, pointer: Pos2) -> Option<String> {
    if !rect.contains(pointer) {
        return None;
    }
    let digit_h = 40.0;
    let start_x = rect.center().x - 66.0;
    let top_y = rect.center().y - digit_h * 0.5;
    let colon_x = start_x + 2.0 * 34.0 - 1.0;

    let p3_pos = Pos2::new(colon_x, top_y + 13.0);
    if pointer.distance(p3_pos) <= 6.5 {
        return Some("p3".to_string());
    }
    let q3_pos = Pos2::new(colon_x, top_y + 27.0);
    if pointer.distance(q3_pos) <= 6.5 {
        return Some("q3".to_string());
    }
    let deg_pos = Pos2::new(start_x + 4.0 * 34.0 + 14.0, top_y + 6.0);
    if pointer.distance(deg_pos) <= 7.5 {
        return Some("deg".to_string());
    }

    let mut best: Option<(String, f32)> = None;
    for (name, p0, p1) in clock_segment_lines(rect) {
        let d = dist_to_segment(pointer, p0, p1);
        if d <= 6.0 && best.as_ref().map(|(_, b)| d < *b).unwrap_or(true) {
            best = Some((name, d));
        }
    }
    best.map(|(n, _)| n)
}

pub fn hit_test_matrix_pixel(rect: Rect, cell: f32, pointer: Pos2) -> Option<usize> {
    if !rect.contains(pointer) {
        return None;
    }
    let col = ((pointer.x - rect.left() - 5.0) / cell).floor() as isize;
    let row = ((pointer.y - rect.top() - 5.0) / cell).floor() as isize;
    if (0..21).contains(&col) && (0..12).contains(&row) {
        Some((row as usize) * 21 + (col as usize))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Vec2;

    #[test]
    fn test_led_ring_geometry_and_doa_azimuth() {
        let c = Pos2::new(100.0, 100.0);
        let top = led_ring_position(c, 50.0, 0, 4);
        let right = led_ring_position(c, 50.0, 1, 4);
        assert!((top.x - 100.0).abs() < 1e-3 && (top.y - 50.0).abs() < 1e-3);
        assert!((right.x - 150.0).abs() < 1e-3 && (right.y - 100.0).abs() < 1e-3);
        assert_eq!(doa_angle_from_pointer(c, top), 0);
        assert_eq!(doa_angle_from_pointer(c, right), 90);
        assert_eq!(doa_angle_from_pointer(c, Pos2::new(100.0, 150.0)), 180);
        assert_eq!(doa_angle_from_pointer(c, Pos2::new(50.0, 100.0)), 270);
    }

    #[test]
    fn test_clock_and_matrix_hit_testing() {
        let rect = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(260.0, 64.0));
        let lines = clock_segment_lines(rect);
        let a1_line = lines.iter().find(|(k, _, _)| k == "a1").unwrap();
        let mid_a1 = Pos2::new(
            (a1_line.1.x + a1_line.2.x) * 0.5,
            (a1_line.1.y + a1_line.2.y) * 0.5,
        );
        assert_eq!(
            hit_test_clock_segment(rect, mid_a1),
            Some("a1".to_string())
        );

        let mrect = Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::new(283.0, 166.0));
        assert_eq!(
            hit_test_matrix_pixel(mrect, 13.0, Pos2::new(17.0, 17.0)),
            Some(0)
        );
        assert_eq!(
            hit_test_matrix_pixel(
                mrect,
                13.0,
                Pos2::new(15.0 + 20.5 * 13.0, 15.0 + 11.5 * 13.0)
            ),
            Some(251)
        );
    }
}
