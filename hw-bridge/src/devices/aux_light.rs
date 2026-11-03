use super::{find_framebuffer_by_name, read_sysfs_u8};
use std::fs;
use std::path::Path;

pub trait AuxLightDevice: Send + Sync {
    fn aux_kind(&self) -> &'static str;
    fn zone_count(&self) -> usize;
    fn init(&self);
    fn read_zones(&self, native_frame: &[[u8; 3]]) -> Vec<[u8; 3]>;
    fn write_native_frame(&self, px: &[[u8; 3]]);
}

pub struct NullAuxLight;

impl AuxLightDevice for NullAuxLight {
    fn aux_kind(&self) -> &'static str {
        "none"
    }
    fn zone_count(&self) -> usize {
        0
    }
    fn init(&self) {}
    fn read_zones(&self, _native_frame: &[[u8; 3]]) -> Vec<[u8; 3]> {
        Vec::new()
    }
    fn write_native_frame(&self, _px: &[[u8; 3]]) {}
}

pub struct LogoSysfsFbDevice {
    pub sysfs_prefix: String,
    pub count: usize,
}

impl AuxLightDevice for LogoSysfsFbDevice {
    fn aux_kind(&self) -> &'static str {
        "logo"
    }

    fn zone_count(&self) -> usize {
        self.count
    }

    fn init(&self) {
        for i in 0..self.count {
            for c in ["red", "green", "blue"] {
                let dir = format!("/sys/class/leds/{}:{}:logo{}", self.sysfs_prefix, c, i);
                if !Path::new(&dir).exists() {
                    let _ = fs::create_dir_all(&dir);
                    let _ = fs::write(format!("{}/brightness", dir), "0\n");
                    let _ = fs::write(format!("{}/max_brightness", dir), "255\n");
                    let _ = fs::write(format!("{}/trigger", dir), "[ledfb-logo] none\n");
                }
            }
        }
        if !Path::new("/dev/fb1").exists() && self.count > 0 {
            let _ = fs::write("/dev/fb1", vec![0u8; self.count * 3]);
        }
    }

    fn read_zones(&self, native_frame: &[[u8; 3]]) -> Vec<[u8; 3]> {
        let mut out = vec![[0u8; 3]; self.count];
        let mut any_sysfs = false;
        for (i, slot) in out.iter_mut().enumerate().take(self.count) {
            let r = read_sysfs_u8(&format!(
                "/sys/class/leds/{}:red:logo{}/brightness",
                self.sysfs_prefix, i
            ));
            let g = read_sysfs_u8(&format!(
                "/sys/class/leds/{}:green:logo{}/brightness",
                self.sysfs_prefix, i
            ));
            let b = read_sysfs_u8(&format!(
                "/sys/class/leds/{}:blue:logo{}/brightness",
                self.sysfs_prefix, i
            ));
            if r > 0 || g > 0 || b > 0 {
                any_sysfs = true;
            }
            *slot = [r, g, b];
        }
        if any_sysfs {
            return out;
        }
        if let Ok(raw) = fs::read("/dev/fb1") {
            if raw.len() == self.count * 4 && raw.iter().any(|&b| b > 0) {
                for (i, slot) in out.iter_mut().enumerate().take(self.count) {
                    let base = i * 4;
                    let w = raw[base + 3] / 2;
                    *slot = [
                        raw[base].saturating_add(w),
                        raw[base + 1].saturating_add(w),
                        raw[base + 2].saturating_add(w),
                    ];
                }
                return out;
            } else if raw.len() >= self.count * 3 && raw[..self.count * 3].iter().any(|&b| b > 0) {
                for (i, slot) in out.iter_mut().enumerate().take(self.count) {
                    let base = i * 3;
                    *slot = [raw[base], raw[base + 1], raw[base + 2]];
                }
                return out;
            }
        }
        if native_frame.len() >= self.count {
            let any_native = native_frame
                .iter()
                .take(self.count)
                .any(|p| p[0] > 0 || p[1] > 0 || p[2] > 0);
            if any_native {
                out[..self.count].copy_from_slice(&native_frame[..self.count]);
            }
        }
        out
    }

    fn write_native_frame(&self, px: &[[u8; 3]]) {
        if self.count == 0 {
            return;
        }
        let mut raw = Vec::with_capacity(self.count * 3);
        for i in 0..self.count {
            let c = px.get(i % px.len().max(1)).copied().unwrap_or([0, 0, 0]);
            raw.extend_from_slice(&c);
        }
        let _ = fs::write("/dev/fb1", raw);
    }
}

pub struct OrionAmbilightDevice {
    pub zones: usize,
}

impl OrionAmbilightDevice {
    fn ambilight_fb_path(&self) -> String {
        find_framebuffer_by_name("ambilight", "/dev/fb0")
    }

    fn read_sysfs_zone(i: usize) -> Option<[u8; 3]> {
        let ra = read_sysfs_u8(&format!("/sys/class/leds/orion:red:ambilight{}a/brightness", i));
        let ga = read_sysfs_u8(&format!(
            "/sys/class/leds/orion:green:ambilight{}a/brightness",
            i
        ));
        let ba = read_sysfs_u8(&format!(
            "/sys/class/leds/orion:blue:ambilight{}a/brightness",
            i
        ));
        let wa = read_sysfs_u8(&format!(
            "/sys/class/leds/orion:white:ambilight{}a/brightness",
            i
        ));

        let rb = read_sysfs_u8(&format!("/sys/class/leds/orion:red:ambilight{}b/brightness", i));
        let gb = read_sysfs_u8(&format!(
            "/sys/class/leds/orion:green:ambilight{}b/brightness",
            i
        ));
        let bb = read_sysfs_u8(&format!(
            "/sys/class/leds/orion:blue:ambilight{}b/brightness",
            i
        ));
        let wb = read_sysfs_u8(&format!(
            "/sys/class/leds/orion:white:ambilight{}b/brightness",
            i
        ));

        let r = ra.max(rb).saturating_add(wa.max(wb) / 2);
        let g = ga.max(gb).saturating_add(wa.max(wb) / 2);
        let b = ba.max(bb).saturating_add(wa.max(wb) / 2);
        if r > 0 || g > 0 || b > 0 {
            Some([r, g, b])
        } else {
            None
        }
    }
}

impl AuxLightDevice for OrionAmbilightDevice {
    fn aux_kind(&self) -> &'static str {
        "ambilight"
    }

    fn zone_count(&self) -> usize {
        self.zones
    }

    fn init(&self) {
        let fb_path = self.ambilight_fb_path();
        if !Path::new(&fb_path).exists() {
            let _ = fs::write(&fb_path, vec![0u8; self.zones * 6]);
        }
    }

    fn read_zones(&self, _native_frame: &[[u8; 3]]) -> Vec<[u8; 3]> {
        let mut zones_px = vec![[0u8; 3]; self.zones];
        let mut any_sysfs = false;
        for (i, slot) in zones_px.iter_mut().enumerate().take(self.zones) {
            if let Some(rgb) = Self::read_sysfs_zone(i) {
                *slot = rgb;
                any_sysfs = true;
            }
        }
        if any_sysfs {
            return zones_px;
        }

        let fb_path = self.ambilight_fb_path();
        if let Ok(raw) = fs::read(&fb_path) {
            if raw.len() >= self.zones * 6 && raw.iter().any(|&b| b > 0) {
                for (i, slot) in zones_px.iter_mut().enumerate().take(self.zones) {
                    let base = i * 6;
                    let r = raw[base].max(raw[base + 3]);
                    let g = raw[base + 1].max(raw[base + 4]);
                    let b = raw[base + 2].max(raw[base + 5]);
                    *slot = [r, g, b];
                }
            }
        }
        zones_px
    }

    fn write_native_frame(&self, px: &[[u8; 3]]) {
        let fb_path = self.ambilight_fb_path();
        let mut raw = Vec::with_capacity(self.zones * 6);
        for i in 0..self.zones {
            let [r, g, b] = px.get(i % px.len().max(1)).copied().unwrap_or([0, 0, 0]);
            raw.extend_from_slice(&[r, g, b, r, g, b]);
        }
        let _ = fs::write(fb_path, raw);
    }
}
