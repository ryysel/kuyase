use crate::devices::BoardPeripherals;
use serde_json::Value;
use std::collections::VecDeque;
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const MAIND_LOG_PATH: &str = "/tmp/daemons_logs/maind.log";
pub const VOLUME_FILES: [&str; 3] = [
    "/bkupgrade/data/quasar/last_vendor_volume",
    "/data/quasar/data/last_vendor_volume",
    "/data/quasar/last_vendor_volume",
];

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AliceState {
    Idle = 0,
    Listening = 1,
    Speaking = 2,
}

impl AliceState {
    pub fn from_u32(v: u32) -> Self {
        match v {
            1 => Self::Listening,
            2 => Self::Speaking,
            _ => Self::Idle,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "IDLE",
            Self::Listening => "LISTENING",
            Self::Speaking => "SPEAKING",
        }
    }
}

pub struct BoardProfile {
    pub board_slug: String,
    pub board_name: String,
    pub sysfs_prefix: String,
    pub ring_leds: usize,
    pub logo_leds: usize,
    pub ambilight_leds: usize,
    pub has_clock: bool,
}

impl BoardProfile {
    pub fn detect() -> Self {
        let raw_slug = env::var("QUASAR_BOARD").unwrap_or_else(|_| {
            if let Ok(raw) = fs::read_to_string("/tmp/quasar-emu-hw.json") {
                if let Ok(v) = serde_json::from_str::<Value>(&raw) {
                    if let Some(s) = v.get("board_slug").and_then(|x| x.as_str()) {
                        return s.to_string();
                    }
                }
            }
            if let Ok(raw) = fs::read_to_string("/etc/quasar-board") {
                let s = raw.trim();
                if !s.is_empty() {
                    return s.to_string();
                }
            }
            if Path::new("/sys/class/leds/cucumber:red:ring0").exists()
                || Path::new("/boot/yandex-cucumber-i2ctop-overlay.dtb").exists()
            {
                "cucumber".to_string()
            } else if Path::new("/sys/class/leds/orion:red:ring0").exists()
                || Path::new("/boot/yandex-orion-i2c_top-overlay.dtb").exists()
            {
                "orion".to_string()
            } else if Path::new("/sys/class/leds/plum:red:ring1").exists()
                || Path::new("/sys/class/leds/plum:red:ring0").exists()
            {
                if Path::new("/sys/class/leds/plum:white:clock-a1").exists() {
                    "plum".to_string()
                } else {
                    "plumnc".to_string()
                }
            } else if Path::new("/sys/class/leds/station2:red:ring0").exists() {
                "station2".to_string()
            } else if Path::new("/sys/class/leds/bergamot:red:ring0").exists()
                || Path::new("/sys/class/leds/bergamot:white:clock-a1").exists()
                || Path::new("/lib/firmware/bergamot-acm8625.cfg").exists()
            {
                "bergamot".to_string()
            } else if Path::new("/sys/class/leds/mango:red:ring0").exists()
                || Path::new("/sys/class/leds/mango:red:ring1").exists()
                || Path::new("/lib/firmware/mango-acm8625.cfg").exists()
            {
                "mango".to_string()
            } else if Path::new("/sys/class/leds/mini2:red:ring0").exists()
                || Path::new("/sys/class/leds/mini2:red:ring1").exists()
                || Path::new("/etc/init.d/S61leds_brightness").exists()
            {
                if Path::new("/sys/class/leds/mini2:white:clock-a1").exists() {
                    "yandexmini2".to_string()
                } else {
                    "yandexmini2nc".to_string()
                }
            } else if Path::new("/sys/class/leds/micro:red:ring0").exists()
                || Path::new("/sys/class/leds/micro:red:ring1").exists()
            {
                "yandexmicro".to_string()
            } else {
                "yandexmini".to_string()
            }
        });

        let (board_slug, board_name, sysfs_prefix, ring_leds, logo_leds, ambilight_leds, has_clock) =
            if let Some(desc) = protocol::find_board_descriptor(&raw_slug) {
                (
                    desc.slug.to_string(),
                    desc.board_name.to_string(),
                    desc.sysfs_prefix.to_string(),
                    desc.ring_leds,
                    desc.logo_leds,
                    desc.ambilight_leds,
                    desc.has_clock,
                )
            } else if let Some(v) = fs::read_to_string("/tmp/quasar-emu-hw.json")
                .ok()
                .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            {
                let other = raw_slug.as_str();
                (
                    other.to_string(),
                    v.get("board_name")
                        .and_then(|x| x.as_str())
                        .unwrap_or(other)
                        .to_string(),
                    v.get("sysfs_prefix")
                        .and_then(|x| x.as_str())
                        .unwrap_or(other)
                        .to_string(),
                    v.get("ring_leds").and_then(|x| x.as_u64()).unwrap_or(6) as usize,
                    v.get("logo_leds").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
                    v.get("ambilight_leds")
                        .and_then(|x| x.as_u64())
                        .unwrap_or(0) as usize,
                    v.get("has_clock").and_then(|x| x.as_bool()).unwrap_or(false),
                )
            } else {
                let other = raw_slug.as_str();
                (
                    other.to_string(),
                    format!("Quasar Speaker ({})", other),
                    other.to_string(),
                    6,
                    0,
                    0,
                    false,
                )
            };

        Self {
            board_slug,
            board_name,
            sysfs_prefix,
            ring_leds,
            logo_leds,
            ambilight_leds,
            has_clock,
        }
    }
}

pub struct BridgeState {
    pub board_slug: String,
    pub board_name: String,
    pub ring_leds: usize,
    pub logo_leds: usize,
    pub ambilight_leds: usize,
    pub has_clock: bool,
    pub full_mode: bool,
    pub doa_angle: AtomicU32,
    pub mic_rms: AtomicU32,
    pub mics_muted: AtomicBool,
    pub volume_level: AtomicU32,
    pub volume_overlay_until_ms: AtomicU64,
    pub forced_led_pattern: Mutex<String>,
    pub tof_mm: AtomicU32,
    pub als_lux: AtomicU32,
    pub temp_milli_c: AtomicU32,
    pub speaker_rms: AtomicU32,
    pub mic_burst_active: AtomicBool,
    pub tof_uinput: Mutex<Option<File>>,
    #[allow(dead_code)]
    pub jack_uinput: Mutex<Option<File>>,
    pub key_uinput: Mutex<Option<File>>,
    pub captured_pcm: Mutex<Vec<u8>>,
    pub recent_logs: Mutex<VecDeque<String>>,
    pub aliced_hijack_count: AtomicU32,
    pub aliced_say_count: AtomicU32,
    pub ipc_error_count: AtomicU32,
    pub led_trigger_conflict_count: AtomicU32,
    alice_state: AtomicU32,
    pub dialog_seq: AtomicU32,
    pub hijacked_seq: AtomicU32,
    pub native_led_frame: Mutex<Vec<[u8; 3]>>,
    pub native_aux_frame: Mutex<Vec<[u8; 3]>>,
    pub last_spoken_text: Mutex<String>,
    pub bridge_clients: Mutex<Vec<std::sync::mpsc::SyncSender<protocol::GuestPacket>>>,
    pub peripherals: BoardPeripherals,
}

impl BridgeState {
    pub fn new(
        profile: BoardProfile,
        tof_uinput: Option<File>,
        jack_uinput: Option<File>,
        key_uinput: Option<File>,
    ) -> Self {
        let peripherals = BoardPeripherals::from_profile(&profile);
        let ring_leds = profile.ring_leds;
        let aux_zones = peripherals.aux_light.zone_count();
        let full_mode = fs::read_to_string("/proc/cmdline")
            .map(|c| c.split_whitespace().any(|t| t == "quasar.mode=full"))
            .unwrap_or(false);
        Self {
            board_slug: profile.board_slug,
            board_name: profile.board_name,
            ring_leds,
            logo_leds: profile.logo_leds,
            ambilight_leds: profile.ambilight_leds,
            has_clock: profile.has_clock,
            full_mode,
            doa_angle: AtomicU32::new(0),
            mic_rms: AtomicU32::new(60),
            mics_muted: AtomicBool::new(false),
            volume_level: AtomicU32::new(7),
            volume_overlay_until_ms: AtomicU64::new(0),
            forced_led_pattern: Mutex::new(String::new()),
            tof_mm: AtomicU32::new(8190),
            als_lux: AtomicU32::new(240),
            temp_milli_c: AtomicU32::new(40000),
            speaker_rms: AtomicU32::new(0),
            mic_burst_active: AtomicBool::new(false),
            tof_uinput: Mutex::new(tof_uinput),
            jack_uinput: Mutex::new(jack_uinput),
            key_uinput: Mutex::new(key_uinput),
            captured_pcm: Mutex::new(Vec::new()),
            recent_logs: Mutex::new(VecDeque::new()),
            aliced_hijack_count: AtomicU32::new(0),
            aliced_say_count: AtomicU32::new(0),
            ipc_error_count: AtomicU32::new(0),
            led_trigger_conflict_count: AtomicU32::new(0),
            alice_state: AtomicU32::new(AliceState::Idle as u32),
            dialog_seq: AtomicU32::new(0),
            hijacked_seq: AtomicU32::new(0),
            native_led_frame: Mutex::new(vec![[0u8; 3]; ring_leds]),
            native_aux_frame: Mutex::new(vec![[0u8; 3]; aux_zones]),
            last_spoken_text: Mutex::new(String::new()),
            bridge_clients: Mutex::new(Vec::new()),
            peripherals,
        }
    }

    pub fn broadcast_audio_chunk(&self, pcm_chunk: Vec<u8>) {
        let mut clients = self.bridge_clients.lock().unwrap();
        clients.retain(|tx| {
            match tx.try_send(protocol::GuestPacket::PcmAudioChunk(pcm_chunk.clone())) {
                Ok(()) => true,
                Err(std::sync::mpsc::TrySendError::Full(_)) => true,
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => false,
            }
        });
    }

    pub fn alice_state(&self) -> AliceState {
        AliceState::from_u32(self.alice_state.load(Ordering::Relaxed))
    }

    pub fn set_alice_state(&self, st: AliceState) {
        self.alice_state.store(st as u32, Ordering::SeqCst);
    }

    pub fn set_volume_level(&self, level: u32) {
        let clamped = level.min(10);
        self.volume_level.store(clamped, Ordering::SeqCst);
        self.volume_overlay_until_ms
            .store(now_millis() + 1500, Ordering::SeqCst);
        self.peripherals
            .audio_hw
            .on_volume_or_mute_changed(clamped, self.mics_muted.load(Ordering::Relaxed));
        let raw_vol = clamped * 1000;
        let vendor_vol = (clamped * 2).to_string();
        let _ = fs::create_dir_all("/bkupgrade/data/quasar");
        for path in VOLUME_FILES {
            if let Some(parent) = Path::new(path).parent() {
                if parent.is_dir() {
                    let _ = fs::write(path, &vendor_vol);
                }
            }
        }
        self.append_maind_log(&format!("[sdk] onVolumeChanged: volume={}", clamped));
        self.log(format!(
            "Volume changed -> {}/10 (master_volume={}, LED volume.led arc active)",
            clamped, raw_vol
        ));
        crate::leds::sync_immediate_led_state(self);
    }

    pub fn log(&self, msg: impl Into<String>) {
        let s = msg.into();
        eprintln!("[hw-bridge] {}", s);
        let mut guard = self.recent_logs.lock().unwrap();
        if guard.len() >= 80 {
            guard.pop_front();
        }
        guard.push_back(s);
    }

    pub fn append_maind_log(&self, line: &str) {
        let _ = fs::create_dir_all("/tmp/daemons_logs");
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(MAIND_LOG_PATH)
        {
            let _ = writeln!(f, "{}", line);
        }
    }
}

/// Mirror the vendor volume that Quasar (or the emulator's Go stand-in)
/// writes to `last_vendor_volume` into the Hardware Bridge state.
///
/// The Virtual Speaker Twin used to learn the volume only from host-injected
/// commands, so a phone slider or the station buttons moved ALSA but left
/// "Громк N/10" frozen. `set_volume_level()` also paints the volume arc on the
/// LED Ring, which is what the real Quasar does on a volume change.
pub fn spawn_volume_file_watcher(state: std::sync::Arc<BridgeState>) {
    std::thread::spawn(move || {
        let mut last_seen: [Option<(u32, SystemTime)>; 3] = [None; 3];
        loop {
            let mut changed: Option<(u32, SystemTime)> = None;
            for (idx, path) in VOLUME_FILES.iter().enumerate() {
                let Ok(meta) = fs::metadata(path) else { continue };
                let Ok(raw) = fs::read_to_string(path) else { continue };
                let Ok(value) = raw.trim().parse::<u32>() else { continue };
                let mtime = meta.modified().unwrap_or(UNIX_EPOCH);
                let cur = (value, mtime);
                if last_seen[idx] != Some(cur) {
                    last_seen[idx] = Some(cur);
                    if changed.map(|(_, t)| mtime >= t).unwrap_or(true) {
                        changed = Some(cur);
                    }
                }
            }
            if let Some((raw, _)) = changed {
                // Quasar and Hardware Bridge store last_vendor_volume as
                // percent/5 (0..20); fallback branches cover raw percent
                // (21..100) and legacy master_volume (level * 1000).
                let level = match raw {
                    0..=20 => raw.div_ceil(2),
                    21..=100 => (raw + 5) / 10,
                    _ => (raw / 1000).min(10),
                };
                if level != state.volume_level.load(Ordering::Relaxed) {
                    state.set_volume_level(level);
                }
            }
            std::thread::sleep(Duration::from_millis(400));
        }
    });
}
