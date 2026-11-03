mod audio;
mod bridge_server;
mod devices;
mod input;
mod ipc;
mod leds;
mod proto;
mod state;
mod tts;
mod voice_dialog;

use state::{BoardProfile, BridgeState};
use std::fs;
use std::sync::Arc;

fn ensure_guest_network_defaults() {
    let need_resolv = fs::read_to_string("/etc/resolv.conf")
        .map(|s| !s.lines().any(|l| l.trim_start().starts_with("nameserver")))
        .unwrap_or(true);
    if need_resolv {
        let resolv = "nameserver 10.0.2.3\nnameserver 1.1.1.1\nnameserver 8.8.8.8\n";
        let _ = fs::write("/tmp/resolv.conf", resolv);
        let _ = fs::write("/etc/resolv.conf", resolv);
    }
    for wgetrc in ["/root/.wgetrc", "/.wgetrc"] {
        let has_opt = fs::read_to_string(wgetrc)
            .map(|s| s.contains("check_certificate"))
            .unwrap_or(false);
        if !has_opt {
            let _ = fs::write(wgetrc, "check_certificate = off\n");
        }
    }
}

fn main() {
    ensure_guest_network_defaults();
    input::ensure_adbd_signals_unignored();
    audio::ensure_alsa_compat_lib();
    let profile = BoardProfile::detect();

    // Create /dev/input/event0..2 via /dev/uinput in exact order
    let tof_uinput = input::create_uinput_device(
        "STM VL53L0 proximity sensor",
        &[0, 3], // EV_SYN, EV_ABS
        &[],
        &[(0x12, 8191), (0x19, 255)],
    );
    let jack_uinput = input::create_uinput_device(
        "AML-AUGESOUND Headphones",
        &[0, 5], // EV_SYN, EV_SW
        &[2, 14],
        &[],
    );
    let key_uinput = input::create_uinput_device(
        "adc_keypad",
        &[0, 1], // EV_SYN, EV_KEY
        &[113, 114, 115, 582],
        &[],
    );

    let state = Arc::new(BridgeState::new(
        profile,
        tof_uinput,
        jack_uinput,
        key_uinput,
    ));

    state.log(format!(
        "Initialized Virtual Speaker Twin for {} (ring={} [{}], aux={} [{}], display={})",
        state.board_name,
        state.ring_leds,
        state.peripherals.ring.driver_name(),
        state.peripherals.aux_light.zone_count(),
        state.peripherals.aux_light.aux_kind(),
        state.peripherals.front_display.display_kind(),
    ));

    if state.full_mode {
        state.log(
            "Mode: Full (quasar.mode=full) — yielding IPC (:9850/:9875/:9903) to native firmware daemons (/system/vendor/quasar/onboot.sh)"
                .to_string(),
        );
    } else {
        state.log(
            "Mode: Stub (quasar.mode=stub) — autonomous Quasar IPC & Formant TTS active"
                .to_string(),
        );
        ipc::spawn_yio_audio_server(state.clone());
        ipc::spawn_aliced_ipc_server(state.clone());
        ipc::spawn_iohub_services_ipc_server(state.clone());
    }
    leds::spawn_native_led_animator(state.clone());
    audio::spawn_alsa_loopback_capture(state.clone());
    state::spawn_volume_file_watcher(state.clone());
    input::spawn_serial_console_watcher();

    bridge_server::run_server(state);
}
