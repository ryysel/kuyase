pub mod audio_hw;
pub mod aux_light;
pub mod front_display;
pub mod led_ring;

pub use audio_hw::{AudioSubsystemDevice, GenericAlsaAudio, OrionQuadAcm8625Audio};
pub use aux_light::{AuxLightDevice, LogoSysfsFbDevice, NullAuxLight, OrionAmbilightDevice};
pub use front_display::{
    FrontDisplayDevice, NullFrontDisplay, OrionMatrix21x12Display, SegmentClockSysfsDisplay,
};
pub use led_ring::{LedRingDevice, Sn3236I2cRing, SysfsFbLedRing};

use crate::state::BoardProfile;
use std::fs;
use std::path::Path;

pub fn read_sysfs_u8(path: &str) -> u8 {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<u8>().ok())
        .unwrap_or(0)
}

pub fn read_active_trigger(path: &str) -> String {
    let Ok(raw) = fs::read_to_string(path) else {
        return "none".to_string();
    };
    if let Some(start) = raw.find('[') {
        if let Some(end) = raw[start + 1..].find(']') {
            return raw[start + 1..start + 1 + end].to_string();
        }
    }
    raw.trim().to_string()
}

/// Определяет устройство `/dev/fb*` по содержимому `/sys/class/graphics/fb*/name`.
/// Если sysfs-узлы графики не смонтированы, возвращает `fallback_dev`.
pub fn find_framebuffer_by_name(expected_name: &str, fallback_dev: &str) -> String {
    for idx in 0..8 {
        let name_path = format!("/sys/class/graphics/fb{}/name", idx);
        if let Ok(raw) = fs::read_to_string(&name_path) {
            if raw.trim().eq_ignore_ascii_case(expected_name) {
                let dev_path = format!("/dev/fb{}", idx);
                if Path::new(&dev_path).exists() {
                    return dev_path;
                }
            }
        }
    }
    fallback_dev.to_string()
}

pub struct BoardPeripherals {
    pub ring: Box<dyn LedRingDevice>,
    pub aux_light: Box<dyn AuxLightDevice>,
    pub front_display: Box<dyn FrontDisplayDevice>,
    pub audio_hw: Box<dyn AudioSubsystemDevice>,
}

impl BoardPeripherals {
    pub fn from_profile(profile: &BoardProfile) -> Self {
        let slug = profile.board_slug.as_str();
        let desc = protocol::find_board_descriptor(slug);

        // 1. Световое кольцо (`LedRingDevice`)
        let ring_driver = desc.map(|d| d.ring_driver).unwrap_or("sysfs");
        let ring: Box<dyn LedRingDevice> = if ring_driver == "i2c_sn3236" {
            Box::new(Sn3236I2cRing)
        } else {
            let fallback_fb = desc
                .map(|d| d.ring_fb_dev.to_string())
                .unwrap_or_else(|| "/dev/fb0".to_string());
            Box::new(SysfsFbLedRing {
                sysfs_prefix: profile.sysfs_prefix.clone(),
                leds: profile.ring_leds,
                fallback_fb_dev: fallback_fb,
                driver_label: ring_driver,
            })
        };

        // 2. Дополнительная подсветка (`AuxLightDevice`: Ambilight 12×2 RGBW или Logo)
        let aux_light: Box<dyn AuxLightDevice> = if profile.ambilight_leds > 0 {
            Box::new(OrionAmbilightDevice {
                zones: profile.ambilight_leds.max(12),
            })
        } else if profile.logo_leds > 0 {
            Box::new(LogoSysfsFbDevice {
                sysfs_prefix: profile.sysfs_prefix.clone(),
                count: profile.logo_leds,
            })
        } else {
            Box::new(NullAuxLight)
        };

        // 3. Фронтальный дисплей (`FrontDisplayDevice`: монохромная матрица 21×12 или 7-seg часы)
        let has_matrix = desc.map(|d| d.has_matrix_21x12).unwrap_or(false);
        let front_display: Box<dyn FrontDisplayDevice> = if has_matrix {
            Box::new(OrionMatrix21x12Display::new())
        } else if profile.has_clock {
            Box::new(SegmentClockSysfsDisplay::new(profile.sysfs_prefix.clone()))
        } else {
            Box::new(NullFrontDisplay)
        };

        // 4. Аудиоподсистема (`AudioSubsystemDevice`: 4× ACM8625S + Ultrasound TDM-A)
        let has_ultrasound = desc.map(|d| d.has_ultrasound).unwrap_or(false);
        let audio_hw: Box<dyn AudioSubsystemDevice> = if has_ultrasound {
            Box::new(OrionQuadAcm8625Audio::new())
        } else {
            Box::new(GenericAlsaAudio)
        };

        let peripherals = Self {
            ring,
            aux_light,
            front_display,
            audio_hw,
        };
        peripherals.ring.init();
        peripherals.aux_light.init();
        peripherals.front_display.init();
        peripherals.audio_hw.init();
        peripherals
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hardvold_gain_curve() {
        assert_eq!(OrionQuadAcm8625Audio::hardvold_gain_db(0), -66.6);
        assert_eq!(OrionQuadAcm8625Audio::hardvold_gain_db(10), 0.0);
        let mid = OrionQuadAcm8625Audio::hardvold_gain_db(5);
        assert!(mid > -34.0 && mid < -32.0);
    }
}
