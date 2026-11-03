use protocol::{AmplifierSlotState, UltrasoundState};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

pub struct AudioHwReadout {
    pub amplifiers: Vec<AmplifierSlotState>,
    pub ultrasound: Option<UltrasoundState>,
}

pub trait AudioSubsystemDevice: Send + Sync {
    fn init(&self);
    fn on_volume_or_mute_changed(&self, volume_level: u32, muted: bool);
    fn read_audio_hw(&self, tof_mm: u32) -> AudioHwReadout;
}

pub struct GenericAlsaAudio;

impl AudioSubsystemDevice for GenericAlsaAudio {
    fn init(&self) {}
    fn on_volume_or_mute_changed(&self, _volume_level: u32, _muted: bool) {}
    fn read_audio_hw(&self, _tof_mm: u32) -> AudioHwReadout {
        AudioHwReadout {
            amplifiers: Vec::new(),
            ultrasound: None,
        }
    }
}

pub struct OrionQuadAcm8625Audio {
    pub volume_level: AtomicU32,
    pub spk_muted: AtomicBool,
}

impl OrionQuadAcm8625Audio {
    pub fn new() -> Self {
        Self {
            volume_level: AtomicU32::new(7),
            spk_muted: AtomicBool::new(false),
        }
    }

    const AMP_SLOTS: [(&'static str, &'static str, &'static str, &'static str); 4] = [
        (
            "3-002d",
            "woofer",
            "wf Acm8625",
            "orion-acm8625-woofer.cfg",
        ),
        (
            "3-002e",
            "lr",
            "fr1 Acm8625",
            "orion-acm8625-lr.cfg",
        ),
        (
            "3-002c",
            "center",
            "fr2 Acm8625",
            "orion-acm8625-center.cfg",
        ),
        (
            "2-002c",
            "us",
            "us Acm8625",
            "orion-acm8625-us.cfg",
        ),
    ];

    /// Кривая громкости `hardvold` (`/etc/hardvold.config`): `0 -> -66.6 dB`, `255 -> 0.0 dB`.
    pub fn hardvold_gain_db(volume_level: u32) -> f32 {
        let clamped = volume_level.min(10) as f32;
        ((-66.6 + (clamped / 10.0) * 66.6) * 10.0).round() / 10.0
    }
}

impl AudioSubsystemDevice for OrionQuadAcm8625Audio {
    fn init(&self) {
        for (i2c_id, _role, _alsa, default_cfg) in Self::AMP_SLOTS {
            let dir = format!("/sys/bus/i2c/devices/{}", i2c_id);
            let _ = fs::create_dir_all(&dir);
            let _ = fs::write(format!("{}/name", dir), "acm8625s\n");
            let settings_path = format!("{}/settings_file", dir);
            if !Path::new(&settings_path).exists() {
                let _ = fs::write(&settings_path, format!("{}\n", default_cfg));
            }
        }
    }

    fn on_volume_or_mute_changed(&self, volume_level: u32, muted: bool) {
        self.volume_level
            .store(volume_level.min(10), Ordering::SeqCst);
        self.spk_muted
            .store(muted || volume_level == 0, Ordering::SeqCst);
    }

    fn read_audio_hw(&self, tof_mm: u32) -> AudioHwReadout {
        let vol = self.volume_level.load(Ordering::Relaxed);
        let gain_db = Self::hardvold_gain_db(vol);
        let spk_muted = self.spk_muted.load(Ordering::Relaxed) || vol == 0;

        let mut amplifiers = Vec::with_capacity(4);
        for (i2c_id, role, alsa_prefix, default_cfg) in Self::AMP_SLOTS {
            let sysfs_path = format!("/sys/bus/i2c/devices/{}/settings_file", i2c_id);
            let cfg_name = fs::read_to_string(&sysfs_path)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| default_cfg.to_string());

            let (slot_gain, slot_muted) = if role == "us" {
                (-6.0, false)
            } else {
                (gain_db, spk_muted)
            };

            amplifiers.push(AmplifierSlotState {
                i2c_device: i2c_id.to_string(),
                role: role.to_string(),
                alsa_prefix: alsa_prefix.to_string(),
                settings_file: cfg_name,
                gain_db: slot_gain,
                ramp_db: -6.0,
                muted: slot_muted,
            });
        }

        let echo_boost = if tof_mm < 1500 {
            ((1500 - tof_mm) / 2).min(1200)
        } else {
            0
        };

        AudioHwReadout {
            amplifiers,
            ultrasound: Some(UltrasoundState {
                enabled: true,
                tdm_device: "hw:0,0 (TDM-A-acm8625-ultrasonic / pcm.ultrasound)".to_string(),
                sample_rate_hz: 96000,
                carrier_freq_hz: 38400,
                bandwidth_hz: 600,
                channels: 4,
                active: true,
                rms: 420 + echo_boost,
            }),
        }
    }
}
