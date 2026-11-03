use crate::bridge_client::BridgeClient;
use crate::gui::host_audio::HostAudioPlayer;
pub use crate::gui::os_dialogs::{
    discover_images_in_folder, run_os_custom_file_dialog, run_os_file_dialog, run_os_folder_dialog,
};
pub use crate::gui::signal_log::{
    describe_host_command, diff_state_transitions, format_hms_now, SignalCategory, SignalDirection,
    SignalFilter, SignalLogEntry, MAX_SIGNAL_LOGS,
};
use crate::session::{
    discover_guest_images, resolve_kernel_image, resolve_qemu_binary, EmulatorSession,
    SessionConfig,
};
use protocol::{normalize_board_slug, BridgeStateSnapshot, HostCommand};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub const TOF_WAVE_DURATION_MS: u64 = 900;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub selected_board: String,
    pub mode: String,
    pub clean_boot_snapshot: bool,
    pub persist_data_disk: bool,
    pub bind_all_interfaces: bool,
    pub lan_tap_interface: String,
    pub lan_mac_address: String,
    pub bridge_port: u16,
    pub adb_port: u16,
    pub last_adb_target: String,
    pub custom_qemu_path: String,
    pub custom_kernel_path: String,
    pub custom_images_dir: String,
    pub custom_image_file: String,
    pub audio_muted: bool,
    pub build_source_mode: String,
    pub selected_dump_path: String,
    pub active_terminal_tab: String,
    pub dock_collapsed: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            selected_board: "cucumber".to_string(),
            mode: "stub".to_string(),
            clean_boot_snapshot: true,
            persist_data_disk: false,
            bind_all_interfaces: false,
            lan_tap_interface: String::new(),
            lan_mac_address: crate::session::DEFAULT_LAN_MAC_BASE.to_string(),
            bridge_port: 18091,
            adb_port: 15555,
            last_adb_target: "192.168.1.100:5555".to_string(),
            custom_qemu_path: String::new(),
            custom_kernel_path: String::new(),
            custom_images_dir: String::new(),
            custom_image_file: String::new(),
            audio_muted: false,
            build_source_mode: "adb".to_string(),
            selected_dump_path: String::new(),
            active_terminal_tab: "signals".to_string(),
            dock_collapsed: false,
        }
    }
}

pub struct GuiAppState {
    pub root_dir: PathBuf,
    pub config: UiConfig,
    pub session: Option<EmulatorSession>,
    pub bridge: Option<Arc<BridgeClient>>,
    pub latest_state: Option<BridgeStateSnapshot>,
    pub audio_player: HostAudioPlayer,
    pub guest_images: Vec<(String, PathBuf, u64)>,
    pub firmware_dumps: Vec<(String, PathBuf)>,
    pub qemu_path: Option<PathBuf>,
    pub kernel_path: Option<PathBuf>,
    pub bg_task_running: Arc<AtomicBool>,
    pub bg_task_status: Arc<Mutex<String>>,
    pub was_bg_running: bool,
    pub status_message: String,
    pub terminal_autoscroll: bool,
    pub show_image_manager_modal: bool,
    pub connecting_bridge: Arc<AtomicBool>,
    pub pending_bridge: Arc<Mutex<Option<Result<BridgeClient, String>>>>,
    pub pending_picked_file: Arc<Mutex<Option<PathBuf>>>,
    pub pending_picked_dir: Arc<Mutex<Option<PathBuf>>>,
    pub pending_picked_qemu: Arc<Mutex<Option<PathBuf>>>,
    pub pending_picked_kernel: Arc<Mutex<Option<PathBuf>>>,
    pub signal_logs: VecDeque<SignalLogEntry>,
    pub signal_filter: SignalFilter,
    pub last_logged_guest_line: String,
    pub tof_wave_reset_at: Option<Instant>,
    pub tof_restore_distance: u32,
    // Controls
    pub button_press_action: String,
    pub selected_led_pattern: String,
    pub clock_text_input: String,
    pub clock_brightness: u8,
    pub clock_active_mask: std::collections::BTreeMap<String, bool>,
    pub matrix_text_input: String,
    pub matrix_custom_pixels: Vec<u8>,
    pub matrix_active_mask: Vec<bool>,
    pub doa_slider: u32,
    pub tof_slider: u32,
    pub als_slider: u32,
    pub temp_slider: f32,
    pub volume_slider: u32,
    pub voice_phrase: String,
    pub voice_rms: u32,
    pub voice_wake_only: bool,
    pub ipc_vocalize_text: String,
    pub last_voice_inject_at: Option<Instant>,
}

impl GuiAppState {
    pub fn ui_config_path(root_dir: &Path) -> PathBuf {
        root_dir.join("work").join("kuyase-ui.json")
    }

    pub fn load_config(root_dir: &Path) -> UiConfig {
        let path = Self::ui_config_path(root_dir);
        if let Ok(raw) = fs::read_to_string(&path) {
            if let Ok(mut cfg) = serde_json::from_str::<UiConfig>(&raw) {
                cfg.mode = "stub".to_string();
                if !matches!(
                    cfg.active_terminal_tab.as_str(),
                    "signals" | "serial" | "maind"
                ) {
                    cfg.active_terminal_tab = "signals".to_string();
                }
                return cfg;
            }
        }
        UiConfig::default()
    }

    pub fn save_config(&self) -> Result<(), String> {
        let path = Self::ui_config_path(&self.root_dir);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(&self.config).map_err(|e| e.to_string())?;
        fs::write(&path, json).map_err(|e| e.to_string())
    }

    pub fn new(root_dir: &Path) -> Self {
        let config = Self::load_config(root_dir);
        let audio_player = HostAudioPlayer::new(config.audio_muted);
        let mut app = Self {
            root_dir: root_dir.to_path_buf(),
            config,
            session: None,
            bridge: None,
            latest_state: None,
            audio_player,
            guest_images: Vec::new(),
            firmware_dumps: Vec::new(),
            qemu_path: None,
            kernel_path: None,
            bg_task_running: Arc::new(AtomicBool::new(false)),
            bg_task_status: Arc::new(Mutex::new(String::from("Idle"))),
            was_bg_running: false,
            status_message: String::from("Готов к запуску"),
            terminal_autoscroll: true,
            show_image_manager_modal: false,
            connecting_bridge: Arc::new(AtomicBool::new(false)),
            pending_bridge: Arc::new(Mutex::new(None)),
            pending_picked_file: Arc::new(Mutex::new(None)),
            pending_picked_dir: Arc::new(Mutex::new(None)),
            pending_picked_qemu: Arc::new(Mutex::new(None)),
            pending_picked_kernel: Arc::new(Mutex::new(None)),
            signal_logs: VecDeque::with_capacity(MAX_SIGNAL_LOGS),
            signal_filter: SignalFilter::All,
            last_logged_guest_line: String::new(),
            tof_wave_reset_at: None,
            tof_restore_distance: 8190,
            button_press_action: "single".to_string(),
            selected_led_pattern: String::new(),
            clock_text_input: "19:30".to_string(),
            clock_brightness: 255,
            clock_active_mask: crate::gui::canvas::build_local_segments_from_text("19:30", 255)
                .into_iter()
                .filter(|(_, v)| *v > 0)
                .map(|(k, _)| (k, true))
                .collect(),
            matrix_text_input: "19:30".to_string(),
            matrix_custom_pixels: crate::gui::canvas::render_text_to_matrix_21x12("19:30", 255),
            matrix_active_mask: crate::gui::canvas::render_text_to_matrix_21x12("19:30", 255)
                .into_iter()
                .map(|v| v > 0)
                .collect(),
            doa_slider: 90,
            tof_slider: 8190,
            als_slider: 240,
            temp_slider: 40.0,
            volume_slider: 7,
            voice_phrase: "включи ночной режим".to_string(),
            voice_rms: 2200,
            voice_wake_only: false,
            ipc_vocalize_text: "Привет! Сегодня в Москве плюс 21 градус, без осадков.".to_string(),
            last_voice_inject_at: None,
        };
        app.refresh_dependencies();
        app.refresh_images_and_dumps();
        app.push_signal(
            SignalDirection::Rx,
            SignalCategory::Bridge,
            format!(
                "Инициализирован интерфейс KUYASE (найдено образов: {})",
                app.guest_images.len()
            ),
        );
        let _ = app.save_config();
        app
    }

    pub fn push_signal(
        &mut self,
        direction: SignalDirection,
        category: SignalCategory,
        message: impl Into<String>,
    ) {
        if self.signal_logs.len() >= MAX_SIGNAL_LOGS {
            self.signal_logs.pop_front();
        }
        self.signal_logs.push_back(SignalLogEntry {
            timestamp: format_hms_now(),
            direction,
            category,
            message: message.into(),
        });
    }

    pub fn clear_signals(&mut self) {
        self.signal_logs.clear();
    }

    pub fn filtered_signals(&self) -> Vec<SignalLogEntry> {
        self.signal_logs
            .iter()
            .filter(|e| self.signal_filter.matches(e.category))
            .cloned()
            .collect()
    }

    pub fn record_state_transition(
        &mut self,
        prev_opt: Option<&BridgeStateSnapshot>,
        next: &BridgeStateSnapshot,
    ) {
        let mut last_line = std::mem::take(&mut self.last_logged_guest_line);
        let events = diff_state_transitions(prev_opt, next, &mut last_line);
        self.last_logged_guest_line = last_line;
        for (cat, msg) in events {
            self.push_signal(SignalDirection::Rx, cat, msg);
        }
    }

    pub fn trigger_tof_wave(&mut self) {
        if self.tof_wave_reset_at.is_none() {
            self.tof_restore_distance = if self.tof_slider <= 200 {
                8190
            } else {
                self.tof_slider
            };
        }
        self.tof_slider = 140;
        self.tof_wave_reset_at =
            Some(Instant::now() + Duration::from_millis(TOF_WAVE_DURATION_MS));
        self.send_command(HostCommand::SetSensor {
            doa_angle: None,
            mic_rms: None,
            als_lux: None,
            temp_c: None,
            tof_mm: Some(140),
            swipe: Some(true),
            swipe_direction: Some("wave".to_string()),
        });
    }

    pub fn set_custom_images_dir(&mut self, dir: PathBuf) {
        if dir.is_dir() {
            self.config.custom_images_dir = dir.display().to_string();
            self.refresh_images_and_dumps();
            let _ = self.save_config();
            self.push_signal(
                SignalDirection::Tx,
                SignalCategory::Bridge,
                format!(
                    "Выбрана папка образов: {} (найдено образов: {})",
                    dir.display(),
                    self.guest_images.len()
                ),
            );
        }
    }

    pub fn select_custom_file(&mut self, file: PathBuf) {
        if !file.is_file() {
            return;
        }
        let name = file
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if name.ends_with(".ext4") {
            let stem = name
                .trim_end_matches("-system.ext4")
                .trim_end_matches(".ext4");
            let slug = normalize_board_slug(stem);
            self.config.custom_image_file = file.display().to_string();
            if let Some(parent) = file.parent() {
                self.config.custom_images_dir = parent.display().to_string();
            }
            self.config.selected_board = slug.clone();
            self.refresh_images_and_dumps();
            let _ = self.save_config();
            self.push_signal(
                SignalDirection::Tx,
                SignalCategory::Bridge,
                format!("Выбран файл образа: {} (профиль: {})", file.display(), slug),
            );
            self.status_message = format!("Выбран образ: {}", file.display());
        } else {
            self.config.selected_dump_path = file.display().to_string();
            let _ = self.save_config();
            self.push_signal(
                SignalDirection::Tx,
                SignalCategory::Bridge,
                format!("Выбран файл дампа для сборки .ext4: {}", file.display()),
            );
            self.status_message = format!("Выбран файл дампа: {}", file.display());
        }
    }

    pub fn open_native_file_dialog(&self) {
        let slot = self.pending_picked_file.clone();
        let start_dir = Path::new(self.config.selected_dump_path.trim())
            .parent()
            .filter(|p| p.is_dir())
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| {
                let backups = self.root_dir.join("backups");
                if backups.is_dir() {
                    backups
                } else {
                    self.root_dir.clone()
                }
            });
        thread::spawn(move || {
            if let Some(p) = run_os_file_dialog(&start_dir) {
                *slot.lock().unwrap() = Some(p);
            }
        });
    }

    pub fn open_native_folder_dialog(&self) {
        let slot = self.pending_picked_dir.clone();
        let start_dir = if !self.config.custom_images_dir.trim().is_empty() {
            PathBuf::from(self.config.custom_images_dir.trim())
        } else {
            self.root_dir.join("work")
        };
        thread::spawn(move || {
            if let Some(p) = run_os_folder_dialog(&start_dir) {
                *slot.lock().unwrap() = Some(p);
            }
        });
    }

    pub fn open_qemu_file_dialog(&self) {
        let slot = self.pending_picked_qemu.clone();
        let start_dir = self
            .qemu_path
            .as_ref()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| self.root_dir.clone());
        thread::spawn(move || {
            if let Some(p) = run_os_custom_file_dialog(
                &start_dir,
                "Выберите исполняемый файл qemu-system-aarch64",
                "QEMU (qemu-system-aarch64*)|qemu-system-aarch64*;*.exe|Все файлы (*.*)|*.*",
            ) {
                *slot.lock().unwrap() = Some(p);
            }
        });
    }

    pub fn open_kernel_file_dialog(&self) {
        let slot = self.pending_picked_kernel.clone();
        let start_dir = self
            .kernel_path
            .as_ref()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| self.root_dir.join("kernel"));
        thread::spawn(move || {
            if let Some(p) = run_os_custom_file_dialog(
                &start_dir,
                "Выберите образ гостевого ядра Linux (kuyase-kernel-arm64.img / Image)",
                "Ядро Linux (kuyase-kernel-arm64.img; Image*)|*.img;Image*;*vmlinuz*|Все файлы (*.*)|*.*",
            ) {
                *slot.lock().unwrap() = Some(p);
            }
        });
    }

    pub fn resolve_custom_or_rel_file(&self, raw: &str) -> Option<PathBuf> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        let direct = PathBuf::from(trimmed);
        if direct.is_file() {
            return Some(direct);
        }
        let rel = self.root_dir.join(trimmed);
        if rel.is_file() {
            return Some(rel);
        }
        None
    }

    pub fn refresh_dependencies(&mut self) {
        self.qemu_path = if !self.config.custom_qemu_path.trim().is_empty() {
            self.resolve_custom_or_rel_file(&self.config.custom_qemu_path)
        } else {
            resolve_qemu_binary(&self.root_dir).ok()
        };

        self.kernel_path = if !self.config.custom_kernel_path.trim().is_empty() {
            self.resolve_custom_or_rel_file(&self.config.custom_kernel_path)
        } else {
            resolve_kernel_image(&self.root_dir).ok()
        };
    }

    pub fn refresh_images_and_dumps(&mut self) {
        let mut list: Vec<(String, PathBuf, u64)> = Vec::new();

        if !self.config.custom_images_dir.trim().is_empty() {
            let custom_dir = PathBuf::from(self.config.custom_images_dir.trim());
            for (slug, path) in discover_images_in_folder(&custom_dir) {
                let sz = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                list.push((slug, path, sz));
            }
        }

        for (slug, path) in discover_guest_images(&self.root_dir) {
            if !list.iter().any(|(_, p, _)| p == &path) {
                let sz = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                list.push((slug, path, sz));
            }
        }

        if !self.config.custom_image_file.trim().is_empty() {
            let cf = PathBuf::from(self.config.custom_image_file.trim());
            if cf.is_file() && !list.iter().any(|(_, p, _)| p == &cf) {
                let stem = cf
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("custom")
                    .trim_end_matches("-system.ext4")
                    .trim_end_matches(".ext4")
                    .trim_end_matches(".img");
                let slug = normalize_board_slug(stem);
                let sz = fs::metadata(&cf).map(|m| m.len()).unwrap_or(0);
                list.push((slug, cf, sz));
            }
        }

        list.sort_by(|a, b| a.0.cmp(&b.0));
        self.guest_images = list;

        if !self.guest_images.is_empty()
            && !self
                .guest_images
                .iter()
                .any(|(b, _, _)| b == &self.config.selected_board)
        {
            self.config.selected_board = self.guest_images[0].0.clone();
        }

        let dumps = crate::image_builder::discover_firmware_dumps(&self.root_dir);
        if self.config.selected_dump_path.is_empty() {
            if let Some((_, first)) = dumps.first() {
                self.config.selected_dump_path = first.display().to_string();
            }
        }
        self.firmware_dumps = dumps;
    }

    pub fn active_images_dir(&self) -> PathBuf {
        let trimmed = self.config.custom_images_dir.trim();
        if !trimmed.is_empty() {
            let p = PathBuf::from(trimmed);
            if p.is_dir() {
                return p;
            }
        }
        self.root_dir.join("work")
    }

    pub fn selected_board_data_image_path(&self) -> PathBuf {
        crate::image_builder::persistent_data_image_path(
            &self.active_images_dir(),
            &self.config.selected_board,
        )
    }

    pub fn resolve_selected_image_path(&self) -> Option<PathBuf> {
        if !self.config.custom_image_file.trim().is_empty() {
            let cf = PathBuf::from(self.config.custom_image_file.trim());
            if cf.is_file()
                && cf
                    .file_name()
                    .and_then(|s| s.to_str())
                    .map(|n| n.starts_with(&self.config.selected_board))
                    .unwrap_or(false)
            {
                return Some(cf);
            }
        }
        self.guest_images
            .iter()
            .find(|(slug, _, _)| slug == &self.config.selected_board)
            .map(|(_, path, _)| path.clone())
    }

    pub fn start_vm(&mut self) {
        self.stop_vm();
        self.refresh_dependencies();
        self.refresh_images_and_dumps();

        let data_img_exists = self.selected_board_data_image_path().is_file();
        let use_persist_data = self.config.persist_data_disk || data_img_exists;
        let lan_tap = Some(self.config.lan_tap_interface.trim().to_string())
            .filter(|s| !s.is_empty());
        let lan_mac = Some(self.config.lan_mac_address.trim().to_string())
            .filter(|s| !s.is_empty());

        let mut cfg = match SessionConfig::for_board(
            &self.root_dir,
            Some(&self.config.selected_board),
        ) {
            Ok(c) => c,
            Err(_) => {
                if let Some(img_path) = self.resolve_selected_image_path() {
                    SessionConfig {
                        root_dir: self.root_dir.clone(),
                        board: self.config.selected_board.clone(),
                        board_cmdline: "auto".to_string(),
                        mode: self.config.mode.clone(),
                        snapshot: self.config.clean_boot_snapshot,
                        persist_data: use_persist_data,
                        bind_all: self.config.bind_all_interfaces,
                        lan_tap: lan_tap.clone(),
                        lan_mac: lan_mac.clone(),
                        port_offset: 0,
                        bridge_port: self.config.bridge_port,
                        adb_port: self.config.adb_port,
                        sdk_port: Some(18090),
                        drive_img: img_path,
                    }
                } else {
                    self.status_message = format!(
                        "Не найден образ для платы '{}'",
                        self.config.selected_board
                    );
                    return;
                }
            }
        };
        if let Some(custom_img) = self.resolve_selected_image_path() {
            cfg.drive_img = custom_img;
        }
        cfg.mode = self.config.mode.clone();
        cfg.snapshot = self.config.clean_boot_snapshot;
        cfg.persist_data = use_persist_data;
        cfg.bind_all = self.config.bind_all_interfaces;
        cfg.lan_tap = lan_tap;
        cfg.lan_mac = lan_mac;
        cfg.bridge_port = self.config.bridge_port;
        cfg.adb_port = self.config.adb_port;

        let cq = self.qemu_path.as_deref();
        let ck = self.kernel_path.as_deref();
        match EmulatorSession::start_with_overrides(cfg.clone(), cq, ck) {
            Ok(sess) => {
                self.status_message = format!(
                    "Запущена ВМ '{}' ({}, mode={}, snapshot={}). Подключение к :{}...",
                    cfg.board,
                    cfg.drive_img.display(),
                    cfg.mode,
                    cfg.snapshot,
                    cfg.bridge_port
                );
                self.push_signal(
                    SignalDirection::Tx,
                    SignalCategory::Bridge,
                    format!(
                        "Старт QEMU: плата={}, образ={}, режим={}, snapshot={}",
                        cfg.board,
                        cfg.drive_img.display(),
                        cfg.mode,
                        cfg.snapshot
                    ),
                );
                self.session = Some(sess);
                let _ = self.save_config();
                self.spawn_bridge_connector(cfg.bridge_port);
            }
            Err(e) => {
                self.status_message = format!("Ошибка запуска ВМ: {}", e);
                self.push_signal(
                    SignalDirection::Rx,
                    SignalCategory::Bridge,
                    format!("Ошибка запуска QEMU: {}", e),
                );
            }
        }
    }

    fn spawn_bridge_connector(&mut self, bridge_port: u16) {
        if self.connecting_bridge.swap(true, Ordering::SeqCst) {
            return;
        }
        let conn_flag = self.connecting_bridge.clone();
        let slot = self.pending_bridge.clone();
        thread::spawn(move || {
            let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), bridge_port);
            let res =
                BridgeClient::connect_with_timeout(addr, crate::session::bridge_connect_timeout());
            *slot.lock().unwrap() = Some(res);
            conn_flag.store(false, Ordering::SeqCst);
        });
    }

    pub fn stop_vm(&mut self) {
        let was_running = self.session.is_some() || self.bridge.is_some();
        if let Some(ref b) = self.bridge {
            b.close();
        }
        self.bridge = None;
        self.latest_state = None;
        self.connecting_bridge.store(false, Ordering::SeqCst);
        *self.pending_bridge.lock().unwrap() = None;
        if let Some(mut s) = self.session.take() {
            s.stop();
            self.status_message = "ВМ остановлена.".to_string();
        }
        if was_running {
            self.push_signal(
                SignalDirection::Tx,
                SignalCategory::Bridge,
                "Виртуальная машина QEMU остановлена".to_string(),
            );
        }
    }

    pub fn restart_vm(&mut self) {
        self.stop_vm();
        self.start_vm();
    }

    pub fn poll_runtime(&mut self) {
        let picked_f = self.pending_picked_file.lock().unwrap().take();
        if let Some(f) = picked_f {
            self.select_custom_file(f);
        }
        let picked_d = self.pending_picked_dir.lock().unwrap().take();
        if let Some(d) = picked_d {
            self.set_custom_images_dir(d);
        }
        let picked_q = self.pending_picked_qemu.lock().unwrap().take();
        if let Some(q) = picked_q {
            self.config.custom_qemu_path = q.display().to_string();
            self.refresh_dependencies();
            let _ = self.save_config();
        }
        let picked_k = self.pending_picked_kernel.lock().unwrap().take();
        if let Some(k) = picked_k {
            self.config.custom_kernel_path = k.display().to_string();
            self.refresh_dependencies();
            let _ = self.save_config();
        }

        if let Some(reset_at) = self.tof_wave_reset_at {
            if Instant::now() >= reset_at {
                self.tof_wave_reset_at = None;
                let restore = self.tof_restore_distance;
                self.tof_slider = restore;
                if self.bridge.is_some() {
                    self.send_command(HostCommand::SetSensor {
                        doa_angle: None,
                        mic_rms: None,
                        als_lux: None,
                        temp_c: None,
                        tof_mm: Some(restore),
                        swipe: None,
                        swipe_direction: None,
                    });
                } else {
                    self.push_signal(
                        SignalDirection::Rx,
                        SignalCategory::TofButtons,
                        format!(
                            "Автовозврат датчика ToF (VL53L0X) после взмаха -> {} мм",
                            restore
                        ),
                    );
                }
            }
        }

        let now_bg = self.bg_task_running.load(Ordering::Relaxed);
        if self.was_bg_running && !now_bg {
            self.refresh_dependencies();
            self.refresh_images_and_dumps();
            let msg = self.bg_task_status.lock().unwrap().clone();
            self.push_signal(SignalDirection::Rx, SignalCategory::Bridge, msg);
        }
        self.was_bg_running = now_bg;

        let pending_res = self.pending_bridge.lock().unwrap().take();
        if let Some(res) = pending_res {
            match res {
                Ok(client) => {
                    self.audio_player.attach_bridge(&client);
                    let snap = client.latest_state();
                    self.volume_slider = snap.volume_level;
                    self.doa_slider = snap.doa_angle;
                    self.tof_slider = snap.tof_mm;
                    self.als_slider = snap.als_lux;
                    self.temp_slider = snap.temp_c;
                    self.record_state_transition(None, &snap);
                    self.latest_state = Some(snap);
                    self.bridge = Some(Arc::new(client));
                    if let Some(ref sess) = self.session {
                        let _ = sess.send_serial_input("\n");
                    }
                    self.status_message =
                        format!("Мост подключен на 127.0.0.1:{}", self.config.bridge_port);
                }
                Err(e) => {
                    self.status_message = format!("Ошибка подключения моста: {}", e);
                    self.push_signal(
                        SignalDirection::Rx,
                        SignalCategory::Bridge,
                        format!("Ошибка подключения моста: {}", e),
                    );
                }
            }
        }
        if let Some(ref b) = self.bridge.clone() {
            if b.is_connected() {
                let next_snap = b.latest_state();
                let prev_snap = self.latest_state.clone();
                self.record_state_transition(prev_snap.as_ref(), &next_snap);
                self.latest_state = Some(next_snap);
            } else {
                self.status_message = "Мост отключен.".to_string();
                self.push_signal(
                    SignalDirection::Rx,
                    SignalCategory::Bridge,
                    "Дуплексный мост отключился".to_string(),
                );
                self.bridge = None;
            }
        }
        if let Some(ref mut s) = self.session {
            if !s.is_running() {
                self.status_message = "Процесс QEMU завершился.".to_string();
                self.session = None;
                self.bridge = None;
            }
        }
    }

    pub fn send_command(&mut self, cmd: HostCommand) {
        if matches!(cmd, HostCommand::InjectVoice { .. }) {
            self.last_voice_inject_at = Some(Instant::now());
        }
        let (cat, desc) = describe_host_command(&cmd);
        self.push_signal(SignalDirection::Tx, cat, desc);

        let Some(bridge) = self.bridge.clone() else {
            self.status_message =
                "Команда записана в лог сигналов (ВМ не запущена).".to_string();
            return;
        };
        match bridge.send_command(&cmd) {
            Ok(ack) => {
                let next_snap = bridge.latest_state();
                let prev_snap = self.latest_state.clone();
                self.record_state_transition(prev_snap.as_ref(), &next_snap);
                self.latest_state = Some(next_snap);
                if !ack.ok {
                    let err_msg = ack.error.unwrap_or_else(|| "error".to_string());
                    self.status_message = format!("Команда отклонена: {}", err_msg);
                    self.push_signal(
                        SignalDirection::Rx,
                        cat,
                        format!("Отклонено мостом: {}", err_msg),
                    );
                }
            }
            Err(e) => {
                self.status_message = format!("Ошибка отправки команды: {}", e);
                self.push_signal(
                    SignalDirection::Rx,
                    SignalCategory::Bridge,
                    format!("Ошибка отправки команды: {}", e),
                );
            }
        }
    }

    pub fn send_serial_raw(&mut self, bytes_str: &str) {
        let Some(ref sess) = self.session else {
            self.status_message = "ВМ не запущена.".to_string();
            return;
        };
        if let Err(e) = sess.send_serial_input(bytes_str) {
            self.status_message = format!("Ошибка записи в ttyAMA0: {}", e);
        }
    }

    pub fn send_serial_command(&mut self, cmd_line: &str) {
        let Some(ref sess) = self.session else {
            self.status_message = "ВМ не запущена.".to_string();
            return;
        };
        let trimmed = cmd_line.trim().to_string();
        let payload = if cmd_line == "\x03" {
            "\x03".to_string()
        } else if cmd_line == "\x04" {
            "\x04".to_string()
        } else {
            format!("\x15{}\n", cmd_line.trim_end_matches(&['\r', '\n'][..]))
        };
        if let Err(e) = sess.send_serial_input(&payload) {
            self.status_message = format!("Ошибка записи в ttyAMA0: {}", e);
            return;
        }
        if !trimmed.is_empty() {
            let label = if cmd_line == "\x03" {
                "^C (SIGINT)"
            } else if cmd_line == "\x04" {
                "^D (EOF)"
            } else {
                &trimmed
            };
            self.push_signal(
                SignalDirection::Tx,
                SignalCategory::Bridge,
                format!("ttyAMA0 # {}", label),
            );
        }
    }
}

impl Drop for GuiAppState {
    fn drop(&mut self) {
        self.stop_vm();
        let _ = self.save_config();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ui_config_persistence_and_custom_image_paths() {
        let tmp =
            std::env::temp_dir().join(format!("kuyase-gui-state-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("bin")).unwrap();
        fs::create_dir_all(tmp.join("kernel")).unwrap();
        let extra_dir = tmp.join("extra_images");
        fs::create_dir_all(&extra_dir).unwrap();
        fs::write(tmp.join("bin").join("qemu-system-aarch64.exe"), b"mock").unwrap();
        fs::write(tmp.join("kernel").join("Image"), b"mock-kernel").unwrap();
        let custom_ext4 = extra_dir.join("orion-system.ext4");
        fs::write(&custom_ext4, b"mock-ext4-image").unwrap();

        let mut app = GuiAppState::new(&tmp);
        assert!(GuiAppState::ui_config_path(&tmp).is_file());

        app.set_custom_images_dir(extra_dir.clone());
        assert_eq!(app.config.custom_images_dir, extra_dir.display().to_string());
        assert!(app.guest_images.iter().any(|(slug, _, _)| slug == "orion"));

        app.select_custom_file(custom_ext4.clone());
        assert_eq!(app.config.selected_board, "orion");
        assert_eq!(
            app.config.custom_image_file,
            custom_ext4.display().to_string()
        );

        let reloaded = GuiAppState::load_config(&tmp);
        assert_eq!(reloaded.selected_board, "orion");
        assert_eq!(reloaded.custom_images_dir, extra_dir.display().to_string());
        assert_eq!(reloaded.custom_image_file, custom_ext4.display().to_string());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_signal_log_tx_rx_and_tof_wave_pulse() {
        let tmp =
            std::env::temp_dir().join(format!("kuyase-signal-log-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("work")).unwrap();

        let mut app = GuiAppState::new(&tmp);
        app.clear_signals();

        app.trigger_tof_wave();
        assert_eq!(app.tof_slider, 140);
        assert!(app.tof_wave_reset_at.is_some());
        assert_eq!(app.signal_logs.len(), 1);
        assert_eq!(app.signal_logs[0].direction, SignalDirection::Tx);
        assert_eq!(app.signal_logs[0].category, SignalCategory::TofButtons);
        assert!(app.signal_logs[0].message.contains("tof_mm=140"));

        app.send_command(HostCommand::IpcTest {
            cmd: "vocalize".to_string(),
            text: Some("Проверка TTS".to_string()),
        });
        assert_eq!(app.signal_logs.len(), 2);
        assert_eq!(app.signal_logs[1].category, SignalCategory::TtsIpc);

        let s1 = BridgeStateSnapshot {
            alice_state: "IDLE".to_string(),
            ..Default::default()
        };
        let mut s2 = s1.clone();
        s2.alice_state = "SPEAKING".to_string();
        s2.last_spoken_text = "Проверка TTS".to_string();
        s2.led_trigger_conflict_count = 1;
        app.record_state_transition(Some(&s1), &s2);

        app.signal_filter = SignalFilter::TtsIpc;
        let tts_signals = app.filtered_signals();
        assert!(tts_signals.len() >= 3);
        assert!(tts_signals
            .iter()
            .any(|e| e.direction == SignalDirection::Rx && e.message.contains("IDLE -> SPEAKING")));

        app.signal_filter = SignalFilter::LedDisplay;
        let led_signals = app.filtered_signals();
        assert_eq!(led_signals.len(), 1);
        assert!(led_signals[0].message.contains("led_trigger_conflict_count = 1"));

        let _ = fs::remove_dir_all(&tmp);
    }
}
