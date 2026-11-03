use std::collections::BTreeMap;

pub const CLOCK_EXTRA_SYMS: [&str; 17] = [
    "p3", "q3", "p4", "q4", "p5", "q5", "p6", "q6", "p7", "q7", "h1", "h2", "m1", "m2", "l1",
    "l2", "deg",
];

/// Возвращает канонический список всех 45 сегментов часов в фиксированном порядке:
/// `a1..g1`, `a2..g2`, `a3..g3`, `a4..g4` (28) + `p3..deg` (17).
pub fn canonical_clock_segment_names() -> Vec<String> {
    let mut names = Vec::with_capacity(45);
    for pos in 1..=4 {
        for part in ["a", "b", "c", "d", "e", "f", "g"] {
            names.push(format!("{}{}", part, pos));
        }
    }
    for sym in CLOCK_EXTRA_SYMS {
        names.push(sym.to_string());
    }
    names
}

pub fn empty_clock_segments_map() -> BTreeMap<String, u8> {
    let mut map = BTreeMap::new();
    for name in canonical_clock_segment_names() {
        map.insert(name, 0);
    }
    map
}

pub fn is_valid_clock_segment(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    if CLOCK_EXTRA_SYMS.contains(&lower.as_str()) {
        return true;
    }
    if lower.len() == 2 {
        let bytes = lower.as_bytes();
        return (b'a'..=b'g').contains(&bytes[0]) && (b'1'..=b'4').contains(&bytes[1]);
    }
    false
}

/// Разбирает строку перечисления сегментов вида `"a1,b1,deg"` или `"a1=255, g2=128, deg:64"`.
pub fn parse_clock_segment_spec(spec: &str, default_brightness: u8) -> Option<BTreeMap<String, u8>> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.eq_ignore_ascii_case("all") {
        let mut map = empty_clock_segments_map();
        for val in map.values_mut() {
            *val = default_brightness;
        }
        return Some(map);
    }
    let tokens: Vec<&str> = trimmed
        .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .collect();
    if tokens.is_empty() {
        return None;
    }

    let mut map = empty_clock_segments_map();
    for tok in tokens {
        let (seg_raw, val_opt) = if let Some((k, v)) = tok.split_once('=') {
            (k.trim(), Some(v.trim()))
        } else if let Some((k, v)) = tok.split_once(':') {
            (k.trim(), Some(v.trim()))
        } else {
            (tok.trim(), None)
        };
        if !is_valid_clock_segment(seg_raw) {
            return None;
        }
        let br = if let Some(v_str) = val_opt {
            v_str.parse::<u16>().ok()?.min(255) as u8
        } else {
            default_brightness
        };
        map.insert(seg_raw.to_ascii_lowercase(), br);
    }
    Some(map)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedLedDisplayFrame {
    Matrix21x12(Vec<u8>),
    Segments45(BTreeMap<String, u8>),
}

/// Разбирает строку или файл формата Quasar `.led` для матрицы `21×12` (252 токена)
/// или 7-сегментного циферблата (28..45 токенов).
pub fn parse_display_led_frame(raw: &str, scale_brightness: u8) -> Option<ParsedLedDisplayFrame> {
    for line in raw.lines() {
        let clean = line.trim();
        if clean.is_empty() || clean.eq_ignore_ascii_case("loop") || clean.starts_with('#') {
            continue;
        }
        let mut parts: Vec<&str> = clean.split_whitespace().collect();
        if parts.len() < 28 {
            continue;
        }
        if parts.len() > 28 {
            if let Some(&last) = parts.last() {
                if last.len() != 6 && last.chars().all(|c| c.is_ascii_digit()) {
                    parts.pop();
                }
            }
        }
        let mut values = Vec::with_capacity(parts.len());
        let mut all_hex = true;
        for &tok in &parts {
            let hex = tok.trim_start_matches("0x").trim_start_matches('#');
            if hex.len() == 6 {
                if let Ok(rgb) = u32::from_str_radix(hex, 16) {
                    let r = ((rgb >> 16) & 0xff) as u8;
                    let g = ((rgb >> 8) & 0xff) as u8;
                    let b = (rgb & 0xff) as u8;
                    let luma = r.max(g).max(b);
                    let scaled = ((luma as u16 * scale_brightness as u16) / 255) as u8;
                    values.push(scaled);
                    continue;
                }
            } else if let Ok(v) = tok.parse::<u16>() {
                let luma = v.min(255) as u8;
                let scaled = ((luma as u16 * scale_brightness as u16) / 255) as u8;
                values.push(scaled);
                continue;
            }
            all_hex = false;
            break;
        }
        if !all_hex {
            continue;
        }
        if values.len() == 252 {
            return Some(ParsedLedDisplayFrame::Matrix21x12(values));
        } else if (28..=45).contains(&values.len()) {
            let names = canonical_clock_segment_names();
            let mut map = empty_clock_segments_map();
            for (name, val) in names.into_iter().zip(values.into_iter()) {
                map.insert(name, val);
            }
            return Some(ParsedLedDisplayFrame::Segments45(map));
        }
    }
    None
}

pub fn clock_segs_for_char(c: char) -> &'static [&'static str] {
    match c.to_ascii_uppercase() {
        '0' | 'O' => &["a", "b", "c", "d", "e", "f"],
        '1' | 'I' => &["b", "c"],
        '2' => &["a", "b", "d", "e", "g"],
        '3' => &["a", "b", "c", "d", "g"],
        '4' => &["b", "c", "f", "g"],
        '5' | 'S' => &["a", "c", "d", "f", "g"],
        '6' => &["a", "c", "d", "e", "f", "g"],
        '7' => &["a", "b", "c"],
        '8' => &["a", "b", "c", "d", "e", "f", "g"],
        '9' => &["a", "b", "c", "d", "f", "g"],
        '-' => &["g"],
        'E' => &["a", "d", "e", "f", "g"],
        'R' => &["e", "g"],
        'C' => &["a", "d", "e", "f"],
        'P' => &["a", "b", "e", "f", "g"],
        _ => &[],
    }
}

pub fn build_segments_map_from_text(text: &str, brightness: u8) -> BTreeMap<String, u8> {
    let mut map = empty_clock_segments_map();
    let trimmed = text.trim();
    if trimmed.is_empty() || brightness == 0 {
        return map;
    }

    let mut chars = Vec::new();
    let mut colon = false;
    let mut degree = false;
    let mut plus = false;

    for ch in trimmed.chars() {
        match ch {
            ':' => colon = true,
            '°' | '*' => degree = true,
            '+' => plus = true,
            _ => {
                if chars.len() < 4 {
                    chars.push(ch);
                }
            }
        }
    }

    for (idx, &ch) in chars.iter().take(4).enumerate() {
        let pos = idx + 1;
        for &part in clock_segs_for_char(ch) {
            map.insert(format!("{}{}", part, pos), brightness);
        }
    }
    if colon {
        map.insert("p3".to_string(), brightness);
        map.insert("q3".to_string(), brightness);
    }
    if degree {
        map.insert("deg".to_string(), brightness);
    }
    if plus {
        map.insert("h1".to_string(), brightness);
        map.insert("m1".to_string(), brightness);
    }
    map
}

/// 4×7 растровый шрифт цифр и символов (`OrionFrontPanelLedFrameBuilder`) для матрицы 21×12.
pub fn glyph_4x7(ch: char) -> [u8; 7] {
    match ch.to_ascii_uppercase() {
        '0' | 'O' => [0b0110, 0b1001, 0b1001, 0b1001, 0b1001, 0b1001, 0b0110],
        '1' | 'I' => [0b0010, 0b0110, 0b0010, 0b0010, 0b0010, 0b0010, 0b0111],
        '2' => [0b0110, 0b1001, 0b0001, 0b0010, 0b0100, 0b1000, 0b1111],
        '3' => [0b1110, 0b0001, 0b0001, 0b0110, 0b0001, 0b0001, 0b1110],
        '4' => [0b1001, 0b1001, 0b1001, 0b1111, 0b0001, 0b0001, 0b0001],
        '5' | 'S' => [0b1111, 0b1000, 0b1000, 0b1110, 0b0001, 0b0001, 0b1110],
        '6' => [0b0111, 0b1000, 0b1000, 0b1110, 0b1001, 0b1001, 0b0110],
        '7' => [0b1111, 0b0001, 0b0010, 0b0010, 0b0100, 0b0100, 0b0100],
        '8' => [0b0110, 0b1001, 0b1001, 0b0110, 0b1001, 0b1001, 0b0110],
        '9' => [0b0110, 0b1001, 0b1001, 0b0111, 0b0001, 0b0001, 0b1110],
        '-' => [0b0000, 0b0000, 0b0000, 0b1111, 0b0000, 0b0000, 0b0000],
        '+' => [0b0000, 0b0100, 0b0100, 0b1110, 0b0100, 0b0100, 0b0000],
        _ => [0; 7],
    }
}

/// Рендерит текстовую строку (например, `"19:30"`, `"+21°"`, `"8"`) в буфер `21×12` (252 байта).
pub fn render_text_to_matrix_21x12(text: &str, brightness: u8) -> Vec<u8> {
    let mut grid = vec![0u8; 252];
    let trimmed = text.trim();
    if trimmed.is_empty() || brightness == 0 {
        return grid;
    }

    let mut chars = Vec::new();
    let mut has_colon = false;
    let mut has_degree = false;
    for ch in trimmed.chars() {
        match ch {
            ':' => has_colon = true,
            '°' | '*' => has_degree = true,
            _ => {
                if chars.len() < 4 {
                    chars.push(ch);
                }
            }
        }
    }

    let y0 = 2usize;
    let put_px = |grid: &mut [u8], x: usize, y: usize, val: u8| {
        if x < 21 && y < 12 {
            grid[y * 21 + x] = val;
        }
    };

    let x_positions: &[usize] = match (chars.len(), has_colon) {
        (4, true) => &[0, 5, 12, 17],
        (4, false) => &[0, 5, 10, 15],
        (3, _) => &[1, 6, 11],
        (2, _) => &[4, 10],
        (1, _) => &[8],
        _ => &[],
    };

    for (idx, &ch) in chars.iter().enumerate() {
        let Some(&x0) = x_positions.get(idx) else {
            break;
        };
        let rows = glyph_4x7(ch);
        for (ry, &bits) in rows.iter().enumerate() {
            for rx in 0..4 {
                if (bits & (1 << (3 - rx))) != 0 {
                    put_px(&mut grid, x0 + rx, y0 + ry, brightness);
                }
            }
        }
    }

    if has_colon {
        put_px(&mut grid, 10, y0 + 2, brightness);
        put_px(&mut grid, 10, y0 + 4, brightness);
    }

    if has_degree {
        if chars.len() >= 4 {
            put_px(&mut grid, 20, 0, brightness);
        } else {
            let deg_x = x_positions
                .last()
                .map(|&x| (x + 5).min(18))
                .unwrap_or(16);
            put_px(&mut grid, deg_x + 1, y0, brightness);
            put_px(&mut grid, deg_x, y0 + 1, brightness);
            put_px(&mut grid, deg_x + 2, y0 + 1, brightness);
            put_px(&mut grid, deg_x + 1, y0 + 2, brightness);
        }
    }

    grid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_segments_matrix_and_led_frame_parsing() {
        let names = canonical_clock_segment_names();
        assert_eq!(names.len(), 45);

        let spec = parse_clock_segment_spec("a1, b2=128, deg:64", 200).unwrap();
        assert_eq!(spec.get("a1"), Some(&200));
        assert_eq!(spec.get("b2"), Some(&128));
        assert_eq!(spec.get("deg"), Some(&64));
        assert_eq!(spec.get("c1"), Some(&0));

        let map_time = build_segments_map_from_text("19:30°", 255);
        assert_eq!(map_time.get("b1"), Some(&255));
        assert_eq!(map_time.get("p3"), Some(&255));
        assert_eq!(map_time.get("deg"), Some(&255));
        assert_eq!(map_time.get("h1"), Some(&0));

        let map_temp = build_segments_map_from_text("+24°", 200);
        assert_eq!(map_temp.get("h1"), Some(&200));
        assert_eq!(map_temp.get("m1"), Some(&200));
        assert_eq!(map_temp.get("deg"), Some(&200));

        let grid = render_text_to_matrix_21x12("19:30°", 255);
        assert_eq!(grid.len(), 252);
        assert!(grid.iter().filter(|&&px| px > 0).count() > 25);
        assert_eq!(grid[4 * 21 + 10], 255);
        assert_eq!(grid[6 * 21 + 10], 255);
        assert_eq!(grid[20], 255);

        let mut tokens45 = vec!["000000"; 45];
        tokens45[0] = "ffffff";
        tokens45[44] = "808080";
        let raw_seg_led = format!("loop\n{} 40\n", tokens45.join(" "));
        match parse_display_led_frame(&raw_seg_led, 255) {
            Some(ParsedLedDisplayFrame::Segments45(map)) => {
                assert_eq!(map.get("a1"), Some(&255));
                assert_eq!(map.get("deg"), Some(&128));
                assert_eq!(map.get("b1"), Some(&0));
            }
            _ => panic!("Expected Segments45 frame"),
        }

        let mut tokens252 = vec!["000000"; 252];
        tokens252[0] = "ffffff";
        tokens252[251] = "404040";
        let raw_mat_led = format!("loop\n{} 35\n", tokens252.join(" "));
        match parse_display_led_frame(&raw_mat_led, 255) {
            Some(ParsedLedDisplayFrame::Matrix21x12(px)) => {
                assert_eq!(px.len(), 252);
                assert_eq!(px[0], 255);
                assert_eq!(px[251], 64);
            }
            _ => panic!("Expected Matrix21x12 frame"),
        }
    }
}
