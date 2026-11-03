use super::{find_framebuffer_by_name, read_sysfs_u8};
use crate::leds::parse_led_pattern_str;
pub use protocol::display::{
    build_segments_map_from_text, canonical_clock_segment_names, empty_clock_segments_map,
    is_valid_clock_segment, parse_clock_segment_spec, parse_display_led_frame,
    render_text_to_matrix_21x12, ParsedLedDisplayFrame,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

pub struct FrontDisplayReadout {
    pub display_kind: &'static str,
    pub clock_segments: BTreeMap<String, u8>,
    pub screen_width: usize,
    pub screen_height: usize,
    pub screen_pixels: Vec<u8>,
}

pub trait FrontDisplayDevice: Send + Sync {
    fn display_kind(&self) -> &'static str;
    fn init(&self);
    fn read_display(&self) -> FrontDisplayReadout;
    fn write_display_text(&self, text: &str, brightness: u8);
    fn write_display_segments(&self, segments: &BTreeMap<String, u8>, replace_all: bool);
    fn write_display_pixels(&self, pixels: &[u8]);
    fn write_display_led_frame(&self, raw_led: &str, brightness: u8) -> bool;
    fn scale_active_brightness(&self, brightness: u8);
    fn render_volume_overlay(&self, level: u32, active: bool);
}

pub struct NullFrontDisplay;

impl FrontDisplayDevice for NullFrontDisplay {
    fn display_kind(&self) -> &'static str {
        "none"
    }
    fn init(&self) {}
    fn read_display(&self) -> FrontDisplayReadout {
        FrontDisplayReadout {
            display_kind: "none",
            clock_segments: BTreeMap::new(),
            screen_width: OrionMatrix21x12Display::WIDTH,
            screen_height: OrionMatrix21x12Display::HEIGHT,
            screen_pixels: Vec::new(),
        }
    }
    fn write_display_text(&self, _text: &str, _brightness: u8) {}
    fn write_display_segments(&self, _segments: &BTreeMap<String, u8>, _replace_all: bool) {}
    fn write_display_pixels(&self, _pixels: &[u8]) {}
    fn write_display_led_frame(&self, _raw_led: &str, _brightness: u8) -> bool {
        false
    }
    fn scale_active_brightness(&self, _brightness: u8) {}
    fn render_volume_overlay(&self, _level: u32, _active: bool) {}
}

pub struct SegmentClockSysfsDisplay {
    pub sysfs_prefix: String,
    pub saved_segments: Mutex<BTreeMap<String, u8>>,
    pub active_mask: Mutex<BTreeMap<String, bool>>,
}

impl SegmentClockSysfsDisplay {
    pub fn new(sysfs_prefix: String) -> Self {
        Self {
            sysfs_prefix,
            saved_segments: Mutex::new(empty_clock_segments_map()),
            active_mask: Mutex::new(BTreeMap::new()),
        }
    }
}

impl FrontDisplayDevice for SegmentClockSysfsDisplay {
    fn display_kind(&self) -> &'static str {
        "segment_clock"
    }

    fn init(&self) {}

    fn read_display(&self) -> FrontDisplayReadout {
        let mut clock_map = BTreeMap::new();
        let mut any_sysfs = false;
        for seg in canonical_clock_segment_names() {
            let path = format!(
                "/sys/class/leds/{}:white:clock-{}/brightness",
                self.sysfs_prefix, seg
            );
            if Path::new(&path).exists() {
                any_sysfs = true;
            }
            let v = read_sysfs_u8(&path);
            clock_map.insert(seg, v);
        }
        if !any_sysfs {
            clock_map = self.saved_segments.lock().unwrap().clone();
        }
        FrontDisplayReadout {
            display_kind: "segment_clock",
            clock_segments: clock_map,
            screen_width: OrionMatrix21x12Display::WIDTH,
            screen_height: OrionMatrix21x12Display::HEIGHT,
            screen_pixels: Vec::new(),
        }
    }

    fn write_display_text(&self, text: &str, brightness: u8) {
        if self.write_display_led_frame(text, brightness) {
            return;
        }
        if let Some(seg_map) = parse_clock_segment_spec(text, brightness) {
            self.write_display_segments(&seg_map, true);
            return;
        }
        let segs = build_segments_map_from_text(text, brightness);
        self.write_display_segments(&segs, true);
    }

    fn write_display_segments(&self, segments: &BTreeMap<String, u8>, replace_all: bool) {
        let mut cur = self.saved_segments.lock().unwrap();
        let mut mask = self.active_mask.lock().unwrap();
        if replace_all {
            *cur = empty_clock_segments_map();
            mask.clear();
            for (k, &v) in segments {
                if is_valid_clock_segment(k) {
                    let loc = k.to_ascii_lowercase();
                    cur.insert(loc.clone(), v);
                    if v > 0 {
                        mask.insert(loc, true);
                    }
                }
            }
            for (loc, &val) in cur.iter() {
                let p = format!(
                    "/sys/class/leds/{}:white:clock-{}/brightness",
                    self.sysfs_prefix, loc
                );
                let _ = fs::write(p, val.to_string());
            }
        } else {
            for (k, &val) in segments {
                if is_valid_clock_segment(k) {
                    let loc = k.to_ascii_lowercase();
                    cur.insert(loc.clone(), val);
                    if val > 0 {
                        mask.insert(loc.clone(), true);
                    } else {
                        mask.remove(&loc);
                    }
                    let p = format!(
                        "/sys/class/leds/{}:white:clock-{}/brightness",
                        self.sysfs_prefix, loc
                    );
                    let _ = fs::write(p, val.to_string());
                }
            }
        }
    }

    fn write_display_pixels(&self, _pixels: &[u8]) {}

    fn write_display_led_frame(&self, raw_led: &str, brightness: u8) -> bool {
        if let Some(ParsedLedDisplayFrame::Segments45(segs)) =
            parse_display_led_frame(raw_led, brightness)
        {
            self.write_display_segments(&segs, true);
            true
        } else {
            false
        }
    }

    fn scale_active_brightness(&self, brightness: u8) {
        let current = self.read_display().clock_segments;
        {
            let mut mask = self.active_mask.lock().unwrap();
            for (k, &v) in &current {
                if v > 0 {
                    mask.insert(k.clone(), true);
                }
            }
        }
        let mask_keys: Vec<String> = self.active_mask.lock().unwrap().keys().cloned().collect();
        let mut cur = self.saved_segments.lock().unwrap();
        for k in &mask_keys {
            cur.insert(k.clone(), brightness);
            let p = format!(
                "/sys/class/leds/{}:white:clock-{}/brightness",
                self.sysfs_prefix, k
            );
            let _ = fs::write(p, brightness.to_string());
        }
    }

    fn render_volume_overlay(&self, _level: u32, _active: bool) {}
}

pub struct OrionMatrix21x12Display {
    pub saved_pixels: Mutex<Vec<u8>>,
    pub active_mask: Mutex<Vec<bool>>,
    pub volume_overlay_active: AtomicBool,
    pub last_volume_level: AtomicU32,
}

impl OrionMatrix21x12Display {
    pub const WIDTH: usize = 21;
    pub const HEIGHT: usize = 12;
    pub const PIXELS: usize = Self::WIDTH * Self::HEIGHT;

    pub fn new() -> Self {
        let initial_px = Self::render_text_to_matrix("12:00", 255);
        let mask = initial_px.iter().map(|&v| v > 0).collect();
        Self {
            saved_pixels: Mutex::new(initial_px),
            active_mask: Mutex::new(mask),
            volume_overlay_active: AtomicBool::new(false),
            last_volume_level: AtomicU32::new(u32::MAX),
        }
    }

    fn screen_fb_path(&self) -> String {
        find_framebuffer_by_name("YNDX LED screen", "/dev/fb1")
    }

    pub fn render_text_to_matrix(text: &str, brightness: u8) -> Vec<u8> {
        let trimmed = text.trim();
        if trimmed.is_empty() || brightness == 0 {
            return vec![0u8; Self::PIXELS];
        }
        if let Some(pat_name) = trimmed.strip_prefix("pattern:") {
            if let Some(frame) = Self::load_screen_pattern_frame(pat_name, 0) {
                let mut grid = vec![0u8; Self::PIXELS];
                for (i, v) in frame.into_iter().enumerate().take(Self::PIXELS) {
                    grid[i] = ((v as u16 * brightness as u16) / 255) as u8;
                }
                return grid;
            }
        }
        render_text_to_matrix_21x12(trimmed, brightness)
    }

    pub fn load_screen_pattern_frame(name: &str, frame_index: usize) -> Option<Vec<u8>> {
        let candidates = [
            format!("/system/vendor/quasar/ledpatterns/screen/{}.led", name),
            format!(
                "/system/vendor/quasar/ledpatterns/screen/emotion/{}.led",
                name
            ),
            format!(
                "/system/vendor/quasar/ledpatterns/screen/media/{}.led",
                name
            ),
            format!("/usr/share/animation/screen/{}.led", name),
        ];
        for path in &candidates {
            if let Ok(raw) = fs::read_to_string(path) {
                let frames = parse_led_pattern_str(&raw, Self::PIXELS);
                if !frames.is_empty() {
                    let idx = frame_index.min(frames.len() - 1);
                    let mono: Vec<u8> = frames[idx]
                        .0
                        .iter()
                        .map(|px| px[0].max(px[1]).max(px[2]))
                        .collect();
                    return Some(mono);
                }
            }
        }
        None
    }

    fn write_matrix_to_fb(&self, mono_252: &[u8]) {
        let fb_path = self.screen_fb_path();
        let mut raw_rgb24 = Vec::with_capacity(Self::PIXELS * 3);
        for i in 0..Self::PIXELS {
            let v = mono_252.get(i).copied().unwrap_or(0);
            raw_rgb24.extend_from_slice(&[v, v, v]);
        }
        let _ = fs::write(fb_path, raw_rgb24);
    }
}

impl FrontDisplayDevice for OrionMatrix21x12Display {
    fn display_kind(&self) -> &'static str {
        "matrix_21x12"
    }

    fn init(&self) {
        let grid = self.saved_pixels.lock().unwrap().clone();
        self.write_matrix_to_fb(&grid);
    }

    fn read_display(&self) -> FrontDisplayReadout {
        let fb_path = self.screen_fb_path();
        let mut pixels = vec![0u8; Self::PIXELS];
        let mut read_ok = false;
        if let Ok(raw) = fs::read(&fb_path) {
            if raw.len() >= Self::PIXELS * 3 {
                for i in 0..Self::PIXELS {
                    let r = raw[i * 3];
                    let g = raw[i * 3 + 1];
                    let b = raw[i * 3 + 2];
                    pixels[i] = r.max(g).max(b);
                }
                read_ok = true;
            }
        }
        if !read_ok {
            pixels = self.saved_pixels.lock().unwrap().clone();
        }

        FrontDisplayReadout {
            display_kind: "matrix_21x12",
            clock_segments: BTreeMap::new(),
            screen_width: Self::WIDTH,
            screen_height: Self::HEIGHT,
            screen_pixels: pixels,
        }
    }

    fn write_display_text(&self, text: &str, brightness: u8) {
        if self.write_display_led_frame(text, brightness) {
            return;
        }
        let mask_grid = Self::render_text_to_matrix(text, 255);
        *self.active_mask.lock().unwrap() = mask_grid.iter().map(|&v| v > 0).collect();
        let grid = Self::render_text_to_matrix(text, brightness);
        *self.saved_pixels.lock().unwrap() = grid.clone();
        self.write_matrix_to_fb(&grid);
    }

    fn write_display_segments(&self, _segments: &BTreeMap<String, u8>, _replace_all: bool) {}

    fn write_display_pixels(&self, pixels: &[u8]) {
        let mut grid = vec![0u8; Self::PIXELS];
        for (i, &v) in pixels.iter().enumerate().take(Self::PIXELS) {
            grid[i] = v;
        }
        if grid.iter().any(|&v| v > 0) || pixels.is_empty() {
            *self.active_mask.lock().unwrap() = grid.iter().map(|&v| v > 0).collect();
        }
        *self.saved_pixels.lock().unwrap() = grid.clone();
        self.write_matrix_to_fb(&grid);
    }

    fn write_display_led_frame(&self, raw_led: &str, brightness: u8) -> bool {
        if let Some(ParsedLedDisplayFrame::Matrix21x12(px)) =
            parse_display_led_frame(raw_led, brightness)
        {
            self.write_display_pixels(&px);
            true
        } else {
            false
        }
    }

    fn scale_active_brightness(&self, brightness: u8) {
        let cur_px = self.read_display().screen_pixels;
        {
            let mut mask = self.active_mask.lock().unwrap();
            if cur_px.iter().any(|&v| v > 0) {
                *mask = cur_px.iter().map(|&v| v > 0).collect();
            }
        }
        let mask = self.active_mask.lock().unwrap().clone();
        let mut px = vec![0u8; Self::PIXELS];
        for (i, slot) in px.iter_mut().enumerate().take(Self::PIXELS) {
            if *mask.get(i).unwrap_or(&false) {
                *slot = brightness;
            }
        }
        *self.saved_pixels.lock().unwrap() = px.clone();
        self.write_matrix_to_fb(&px);
    }

    fn render_volume_overlay(&self, level: u32, active: bool) {
        let was_active = self.volume_overlay_active.swap(active, Ordering::SeqCst);
        if active {
            let clamped = level.min(10);
            let prev_lvl = self.last_volume_level.swap(clamped, Ordering::SeqCst);
            if !was_active || prev_lvl != clamped {
                let grid = Self::render_text_to_matrix(&clamped.to_string(), 255);
                self.write_matrix_to_fb(&grid);
            }
        } else if was_active {
            self.last_volume_level.store(u32::MAX, Ordering::SeqCst);
            let grid = self.saved_pixels.lock().unwrap().clone();
            self.write_matrix_to_fb(&grid);
        }
    }
}
