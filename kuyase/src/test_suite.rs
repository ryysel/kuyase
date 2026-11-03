use crate::adb::AdbClient;
use crate::bridge_client::BridgeClient;
use crate::session::{
    discover_guest_images, resolve_board_and_image, EmulatorSession, SessionConfig,
};
use protocol::HostCommand;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::thread;
use std::time::Duration;

struct GuestHwMeta {
    sysfs_prefix: String,
    led_driver: String,
    ring_leds: usize,
    logo_leds: usize,
    has_clock: bool,
}

pub fn run_multi_board_tests(
    root: &Path,
    requested_board: Option<&str>,
    base_bridge_port: u16,
    base_adb_port: u16,
    snapshot: bool,
) -> Result<(), String> {
    let images = if let Some(req) = requested_board {
        let trimmed = req.trim();
        if !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("auto") {
            let (slug, img) = resolve_board_and_image(root, Some(trimmed))?;
            vec![(slug, img)]
        } else {
            discover_guest_images(root)
        }
    } else {
        discover_guest_images(root)
    };

    if images.is_empty() {
        println!("==================================================================");
        println!("[TEST] В папке work/ ещё нет собранных образов *-system.ext4.");
        println!("Дампы прошивок не входят в комплект репозитория KUYASE.");
        println!("==================================================================");
        return Err("No *-system.ext4 Guest Images found in work/".to_string());
    }

    for (idx, (board, drive_img)) in images.iter().enumerate() {
        let bridge_port = base_bridge_port + (idx as u16) * 2;
        let adb_port = base_adb_port + (idx as u16);
        run_board_verification(root, board, drive_img, bridge_port, adb_port, snapshot)?;
    }

    let tested: Vec<&str> = images.iter().map(|(b, _)| b.as_str()).collect();
    println!(
        "\nALL KUYASE STANDALONE EMULATOR TESTS PASSED ({})!",
        tested.join(", ")
    );
    Ok(())
}

fn verify_bridge_and_profile(board: &str, bridge: &BridgeClient) -> Result<(), String> {
    let st = bridge.request_state()?;
    if !st.ok {
        return Err(format!("[{}] bridge state ok == false", board));
    }
    if st.board_slug != board {
        return Err(format!(
            "[{}] Expected board '{}', got '{}'",
            board, board, st.board_slug
        ));
    }
    println!(
        "[{}] Auto-detected board: {} (ring_leds={}, clock={})",
        board, st.board_name, st.ring_leds, st.has_clock
    );
    Ok(())
}

fn verify_adb_and_hw_json(board: &str, adb: &AdbClient) -> Result<GuestHwMeta, String> {
    let adb_out = adb.shell(
        "hostname; uname -m; cat /tmp/quasar-emu-hw.json",
        Duration::from_secs(12),
    )?;
    let expected_host = format!("qemu-{}", board);
    if !adb_out.contains(&expected_host) {
        return Err(format!("[{}] Unexpected adb output: {}", board, adb_out));
    }
    let json_start = adb_out
        .find('{')
        .ok_or_else(|| format!("[{}] Missing JSON in adb output: {}", board, adb_out))?;
    let hw_info: Value = serde_json::from_str(&adb_out[json_start..])
        .map_err(|e| format!("[{}] Invalid /tmp/quasar-emu-hw.json: {}", board, e))?;
    let sysfs_prefix = hw_info["sysfs_prefix"]
        .as_str()
        .ok_or_else(|| format!("[{}] Missing sysfs_prefix", board))?
        .to_string();
    let led_driver = hw_info["led_driver"]
        .as_str()
        .ok_or_else(|| format!("[{}] Missing led_driver", board))?
        .to_string();
    let ring_leds = hw_info["ring_leds"]
        .as_u64()
        .ok_or_else(|| format!("[{}] Missing ring_leds", board))? as usize;
    let logo_leds = hw_info["logo_leds"].as_u64().unwrap_or(0) as usize;
    let has_clock = hw_info["has_clock"].as_bool().unwrap_or(false);
    println!(
        "[{}] ADB & /tmp/quasar-emu-hw.json verified (driver={}, prefix={})",
        board, led_driver, sysfs_prefix
    );
    Ok(GuestHwMeta {
        sysfs_prefix,
        led_driver,
        ring_leds,
        logo_leds,
        has_clock,
    })
}

fn verify_sn3236_i2c_ring(
    board: &str,
    bridge: &BridgeClient,
    adb: &AdbClient,
) -> Result<(), String> {
    adb.shell(
        "i2cset -y 0 0x3c 0x00 0x01 && \
         i2cset -y 0 0x3c 0x26 0x01 0x01 0x01 0x01 0x01 0x01 0x01 0x01 0x01 0x01 0x01 0x01 0x01 0x01 0x01 0x01 0x01 0x01 i && \
         i2cset -y 0 0x3c 0x25 0xaa",
        Duration::from_secs(8),
    )?;
    let _ = bridge.request_state()?;

    adb.shell(
        "i2cset -y 0 0x3c 0x01 0xff 0x0f 0x6b 0xff 0x0f 0x6b 0xff 0x0f 0x6b 0xff 0x0f 0x6b 0xff 0x0f 0x6b 0xff 0x0f 0x6b i",
        Duration::from_secs(8),
    )?;
    let st_unlatched = bridge.request_state()?;
    if st_unlatched.ring_pixels.first().copied() != Some([0, 0, 0]) {
        return Err(format!(
            "[{}] Expected unlatched SN3236 PWM to stay [0, 0, 0], got {:?}",
            board,
            st_unlatched.ring_pixels.first()
        ));
    }

    adb.shell("i2cset -y 0 0x3c 0x25 0xaa", Duration::from_secs(8))?;
    let st_latched = bridge.request_state()?;
    if st_latched.ring_pixels.first().copied() != Some([107, 15, 255]) {
        return Err(format!(
            "[{}] Expected latched SN3236 [107, 15, 255], got {:?}",
            board,
            st_latched.ring_pixels.first()
        ));
    }

    adb.shell(
        "i2cset -y 0 0x3c 0x26 0x03 0x03 0x03 i && i2cset -y 0 0x3c 0x25 0xaa",
        Duration::from_secs(8),
    )?;
    let st_half = bridge.request_state()?;
    if st_half.ring_pixels.first().copied() != Some([53, 7, 127]) {
        return Err(format!(
            "[{}] Expected Imax/2 [53, 7, 127], got {:?}",
            board,
            st_half.ring_pixels.first()
        ));
    }

    adb.shell(
        "i2cset -y 0 0x3c 0x26 0x02 0x02 0x02 i && i2cset -y 0 0x3c 0x25 0xaa",
        Duration::from_secs(8),
    )?;
    let st_off = bridge.request_state()?;
    if st_off.ring_pixels.first().copied() != Some([0, 0, 0]) {
        return Err(format!(
            "[{}] Expected OUT_EN=0 [0, 0, 0], got {:?}",
            board,
            st_off.ring_pixels.first()
        ));
    }

    adb.shell("i2cset -y 0 0x3c 0x4a 0x01", Duration::from_secs(8))?;
    let st_reset = bridge.request_state()?;
    if st_reset.ring_pixels.first().copied() != Some([0, 0, 0])
        || !st_reset.ring_trigger.contains("Stealth")
    {
        return Err(format!(
            "[{}] Expected reset SN3236, got {:?} / {}",
            board,
            st_reset.ring_pixels.first(),
            st_reset.ring_trigger
        ));
    }

    adb.shell(
        "i2cset -y 0 0x3c 0x00 0x01 && \
         i2cset -y 0 0x3c 0x26 0x01 0x01 0x01 0x01 0x01 0x01 0x01 0x01 0x01 0x01 0x01 0x01 0x01 0x01 0x01 0x01 0x01 0x01 i && \
         i2cset -y 0 0x3c 0x25 0xaa",
        Duration::from_secs(8),
    )?;
    let _ = bridge.request_state()?;
    println!(
        "[{}] SN3236 I2C (0x3c) 0x25 latch, BGR order & 0x4a reset verified!",
        board
    );
    Ok(())
}

fn verify_sysfs_fb_ring_and_conflict(
    board: &str,
    meta: &GuestHwMeta,
    bridge: &BridgeClient,
    adb: &AdbClient,
) -> Result<(), String> {
    adb.shell(
        &format!(
            "r=/sys/class/leds/{p}:red:ring0; [ -d \"$r\" ] || r=/sys/class/leds/{p}:red:ring1; \
             echo '[ledfb-ring] none' > \"$r/trigger\" 2>/dev/null || true; \
             echo 200 > \"$r/brightness\" 2>/dev/null || true",
            p = meta.sysfs_prefix
        ),
        Duration::from_secs(8),
    )?;
    let st_conflict = bridge.request_state()?;
    if st_conflict.led_trigger_conflict_count < 1 {
        return Err(format!(
            "[{}] Expected trigger conflict when writing brightness with [ledfb-ring] active, got {}",
            board, st_conflict.led_trigger_conflict_count
        ));
    }

    adb.shell(
        &format!(
            "for i in $(seq 0 {n}); do \
               echo none > /sys/class/leds/{p}:red:ring$i/trigger 2>/dev/null || true; \
               echo 34 > /sys/class/leds/{p}:red:ring$i/brightness 2>/dev/null || true; \
               echo 204 > /sys/class/leds/{p}:green:ring$i/brightness 2>/dev/null || true; \
               echo 255 > /sys/class/leds/{p}:blue:ring$i/brightness 2>/dev/null || true; \
             done",
            n = meta.ring_leds,
            p = meta.sysfs_prefix
        ),
        Duration::from_secs(8),
    )?;
    thread::sleep(Duration::from_millis(120));
    let st_led = bridge.request_state()?;
    if st_led.ring_pixels.len() != meta.ring_leds {
        return Err(format!(
            "[{}] Expected {} ring_pixels, got {}",
            board,
            meta.ring_leds,
            st_led.ring_pixels.len()
        ));
    }
    if st_led.ring_pixels.first().copied() != Some([34, 204, 255]) {
        return Err(format!(
            "[{}] Unexpected ring pixel: {:?}",
            board,
            st_led.ring_pixels.first()
        ));
    }
    println!(
        "[{}] {}-LED ring & [ledfb-ring] trigger guard verified: {:?}",
        board, meta.ring_leds, st_led.ring_pixels[0]
    );

    adb.shell(
        &format!(
            "for i in $(seq 0 {n}); do \
               echo 0 > /sys/class/leds/{p}:red:ring$i/brightness 2>/dev/null || true; \
               echo 0 > /sys/class/leds/{p}:green:ring$i/brightness 2>/dev/null || true; \
               echo 0 > /sys/class/leds/{p}:blue:ring$i/brightness 2>/dev/null || true; \
               echo '[ledfb-ring] none' > /sys/class/leds/{p}:red:ring$i/trigger 2>/dev/null || true; \
             done",
            n = meta.ring_leds,
            p = meta.sysfs_prefix
        ),
        Duration::from_secs(8),
    )?;
    Ok(())
}

fn verify_buttons_and_volume(
    board: &str,
    bridge: &BridgeClient,
    adb: &AdbClient,
) -> Result<(), String> {
    let ack_mute = bridge.send_command(&HostCommand::PressButton {
        button: "mute".to_string(),
        action: "single".to_string(),
    })?;
    if !ack_mute.ok {
        return Err(format!(
            "[{}] PressButton(mute) failed: {:?}",
            board, ack_mute.error
        ));
    }
    thread::sleep(Duration::from_millis(150));
    let st_mute = bridge.request_state()?;
    if !st_mute.mics_muted {
        return Err(format!(
            "[{}] Expected mics_muted=true after mute button press",
            board
        ));
    }
    if st_mute.ring_pixels.first().copied() != Some([255, 0, 0]) {
        return Err(format!(
            "[{}] Expected RED LED ring on MUTE MIC, got {:?}",
            board,
            st_mute.ring_pixels.first()
        ));
    }

    let ack_unmute = bridge.send_command(&HostCommand::PressButton {
        button: "mute".to_string(),
        action: "single".to_string(),
    })?;
    if !ack_unmute.ok {
        return Err(format!(
            "[{}] PressButton(unmute) failed: {:?}",
            board, ack_unmute.error
        ));
    }
    thread::sleep(Duration::from_millis(120));
    let st_unmute = bridge.request_state()?;
    if st_unmute.mics_muted {
        return Err(format!(
            "[{}] Expected mics_muted=false after second mute button press",
            board
        ));
    }

    let ack_pat = bridge.send_command(&HostCommand::SetLedPattern {
        pattern: "rgb:6b0fff".to_string(),
    })?;
    if !ack_pat.ok {
        return Err(format!("[{}] SetLedPattern failed", board));
    }
    let st_pat = bridge.request_state()?;
    if st_pat.forced_led_pattern != "rgb:6b0fff"
        || st_pat.ring_pixels.first().copied() != Some([107, 15, 255])
    {
        return Err(format!(
            "[{}] Expected forced_led_pattern='rgb:6b0fff' ([107, 15, 255]), got '{}' / {:?}",
            board,
            st_pat.forced_led_pattern,
            st_pat.ring_pixels.first()
        ));
    }
    bridge.send_command(&HostCommand::SetLedPattern {
        pattern: String::new(),
    })?;

    let ack_vol = bridge.send_command(&HostCommand::SetVolume { volume_level: 8 })?;
    if !ack_vol.ok {
        return Err(format!(
            "[{}] SetVolume(8) failed: {:?}",
            board, ack_vol.error
        ));
    }
    thread::sleep(Duration::from_millis(120));
    let st_vol = bridge.request_state()?;
    if st_vol.volume_level != 8 || st_vol.master_volume != 8000 {
        return Err(format!(
            "[{}] Expected volume_level=8 & master_volume=8000, got {} / {}",
            board, st_vol.volume_level, st_vol.master_volume
        ));
    }
    let p0 = st_vol.ring_pixels.first().copied().unwrap_or([0, 0, 0]);
    if (p0[0] as u32 + p0[1] as u32 + p0[2] as u32) == 0 {
        return Err(format!(
            "[{}] Expected lit volume.led arc on volume change, got {:?}",
            board, st_vol.ring_pixels
        ));
    }
    adb.shell(
        "mkdir -p /data/quasar && echo 12 > /data/quasar/last_vendor_volume",
        Duration::from_secs(8),
    )?;
    thread::sleep(Duration::from_millis(550));
    let st_vol_mirror = bridge.request_state()?;
    if st_vol_mirror.volume_level != 6 || st_vol_mirror.master_volume != 6000 {
        return Err(format!(
            "[{}] Expected last_vendor_volume=12 to mirror into volume_level=6 (6000), got {} ({})",
            board, st_vol_mirror.volume_level, st_vol_mirror.master_volume
        ));
    }
    println!(
        "[{}] MUTE MIC red ring ([255, 0, 0]) & Volume (8/10 -> {:?}, vendor mirror 12 -> 6/10) verified!",
        board, p0
    );
    Ok(())
}

fn verify_segment_clock_and_aux_logo(
    board: &str,
    meta: &GuestHwMeta,
    bridge: &BridgeClient,
    adb: &AdbClient,
) -> Result<(), String> {
    if meta.has_clock {
        let ack_clk = bridge.send_command(&HostCommand::SetClockDisplay {
            segments: None,
            replace: None,
            pixels: None,
            led_frame: None,
            brightness: Some(255),
            scale_brightness: None,
            text: Some("19:30°".to_string()),
        })?;
        if !ack_clk.ok {
            return Err(format!("[{}] SetClockDisplay text failed", board));
        }
        thread::sleep(Duration::from_millis(100));
        let st_clk = bridge.request_state()?;
        if st_clk.clock_segments.get("b1").copied().unwrap_or(0) != 255
            || st_clk.clock_segments.get("deg").copied().unwrap_or(0) != 255
        {
            return Err(format!(
                "[{}] Expected clock_segments b1=255 & deg=255, got {:?}",
                board, st_clk.clock_segments
            ));
        }

        let mut partial_segs = BTreeMap::new();
        partial_segs.insert("a1".to_string(), 144);
        partial_segs.insert("p4".to_string(), 80);
        partial_segs.insert("deg".to_string(), 0);
        let ack_clk2 = bridge.send_command(&HostCommand::SetClockDisplay {
            segments: Some(partial_segs),
            replace: Some(false),
            pixels: None,
            led_frame: None,
            brightness: None,
            scale_brightness: None,
            text: None,
        })?;
        if !ack_clk2.ok {
            return Err(format!("[{}] SetClockDisplay segments failed", board));
        }
        thread::sleep(Duration::from_millis(80));
        let st_clk2 = bridge.request_state()?;
        if st_clk2.clock_segments.get("a1").copied() != Some(144)
            || st_clk2.clock_segments.get("p4").copied() != Some(80)
            || st_clk2.clock_segments.get("deg").copied() != Some(0)
        {
            return Err(format!(
                "[{}] Expected partial clock_segments a1=144, p4=80, deg=0, got {:?}",
                board, st_clk2.clock_segments
            ));
        }
        if board == "bergamot" || board == "plum" {
            adb.shell(
                &format!(
                    "echo 188 > /sys/class/leds/{p}:white:clockA0/brightness && \
                     echo 212 > /sys/class/leds/{p}:white:clockG3/brightness && \
                     echo 160 > /sys/class/leds/{p}:white:clockDH/brightness && \
                     echo 250 > /sys/class/leds/{p}:white:clockDEGREE/brightness",
                    p = meta.sysfs_prefix
                ),
                Duration::from_secs(8),
            )?;
            let st_clk0 = bridge.request_state()?;
            if st_clk0.clock_segments.get("a1").copied() != Some(188)
                || st_clk0.clock_segments.get("g4").copied() != Some(212)
                || st_clk0.clock_segments.get("p3").copied() != Some(160)
                || st_clk0.clock_segments.get("deg").copied() != Some(250)
            {
                return Err(format!(
                    "[{}] Expected 0-indexed clock symlinks (clockA0->a1=188, clockG3->g4=212, clockDH->p3=160, clockDEGREE->deg=250), got {:?}",
                    board, st_clk0.clock_segments
                ));
            }
        }

        println!(
            "[{}] 7-segment clock display ('19:30°' + custom PWM segments) verified!",
            board
        );
    }

    if meta.logo_leds > 0 {
        adb.shell(
            "printf '\\x6b\\x0f\\xff\\x6b\\x0f\\xff\\x6b\\x0f\\xff\\x6b\\x0f\\xff\\x6b\\x0f\\xff\\x6b\\x0f\\xff' > /dev/fb1",
            Duration::from_secs(8),
        )?;
        let st_logo_fb = bridge.request_state()?;
        if st_logo_fb.aux_light_kind != "logo"
            || st_logo_fb.logo_pixels.len() != meta.logo_leds
            || st_logo_fb.logo_pixels.first().copied() != Some([107, 15, 255])
        {
            return Err(format!(
                "[{}] Expected /dev/fb1 logo_pixels[0] == [107, 15, 255], got kind={}, pixels={:?}",
                board, st_logo_fb.aux_light_kind, st_logo_fb.logo_pixels
            ));
        }
        adb.shell(
            &format!(
                "echo 34 > /sys/class/leds/{p}:red:logo0/brightness && \
                 echo 204 > /sys/class/leds/{p}:green:logo0/brightness && \
                 echo 255 > /sys/class/leds/{p}:blue:logo0/brightness",
                p = meta.sysfs_prefix
            ),
            Duration::from_secs(8),
        )?;
        let st_logo_sys = bridge.request_state()?;
        if st_logo_sys.logo_pixels.first().copied() != Some([34, 204, 255]) {
            return Err(format!(
                "[{}] Expected sysfs logo_pixels[0] == [34, 204, 255], got {:?}",
                board,
                st_logo_sys.logo_pixels.first()
            ));
        }
        adb.shell(
            &format!(
                "echo 0 > /sys/class/leds/{p}:red:logo0/brightness && \
                 echo 0 > /sys/class/leds/{p}:green:logo0/brightness && \
                 echo 0 > /sys/class/leds/{p}:blue:logo0/brightness && \
                 dd if=/dev/zero of=/dev/fb1 bs=18 count=1 2>/dev/null || true",
                p = meta.sysfs_prefix
            ),
            Duration::from_secs(8),
        )?;
        println!(
            "[{}] {}-LED Aux Logo (/dev/fb1 & sysfs) verified!",
            board, meta.logo_leds
        );
    }
    Ok(())
}

fn verify_bergamot_peripherals(board: &str, adb: &AdbClient) -> Result<(), String> {
    let berg_out = adb.shell(
        "cat /sys/bus/i2c/devices/2-002c/settings_file; \
         cat /sys/class/power_supply/ya_pd/device/name; \
         cat /sys/class/power_supply/ya_pd/health; \
         cat /sys/class/power_supply/ya_pd/voltage_now; \
         cat /sys/class/leds/bergamot:white:clockA0/device/brightness_scale",
        Duration::from_secs(8),
    )?;
    let lines: Vec<&str> = berg_out
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    if lines.first().copied() != Some("bergamot-acm8625.cfg")
        || lines.get(1).copied() != Some("husb238_pd")
        || lines.get(2).copied() != Some("Good")
        || lines.get(3).copied() != Some("12000")
        || !lines.get(4).map(|s| s.contains(';')).unwrap_or(false)
    {
        return Err(format!(
            "[{}] Unexpected bergamot ACM8625P / HUSB238 PD / AW21036 sysfs state: {:?}",
            board, lines
        ));
    }
    println!(
        "[{}] Bergamot ACM8625P (bergamot-acm8625.cfg), HUSB238 PD (12V Good) & AW21036 brightness_scale verified!",
        board
    );
    Ok(())
}

fn verify_mango_peripherals(board: &str, adb: &AdbClient) -> Result<(), String> {
    let mango_out = adb.shell(
        "cat /sys/bus/i2c/devices/2-002c/settings_file; \
         cat /sys/devices/virtual/cw2217_cls/cw2217_coulomb/capacity; \
         cat /sys/devices/virtual/ip2366_cls/ip2366_charger/is_pd; \
         cat /sys/class/power_supply/battery/status",
        Duration::from_secs(8),
    )?;
    let lines: Vec<&str> = mango_out
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    if lines.first().copied() != Some("mango-acm8625.cfg")
        || lines.get(1).copied() != Some("85")
        || lines.get(2).copied() != Some("1")
        || lines.get(3).copied() != Some("Charging")
    {
        return Err(format!(
            "[{}] Unexpected mango ACM8625S / CW2217 / IP2366 sysfs state: {:?}",
            board, lines
        ));
    }
    println!(
        "[{}] Mango ACM8625S (mango-acm8625.cfg), CW2217 battery (85%) & IP2366 PD charger verified!",
        board
    );
    Ok(())
}

fn verify_orion_peripherals(
    board: &str,
    bridge: &BridgeClient,
    adb: &AdbClient,
) -> Result<(), String> {
    thread::sleep(Duration::from_millis(1550));
    bridge.send_command(&HostCommand::SetClockDisplay {
        segments: None,
        replace: None,
        pixels: None,
        led_frame: None,
        brightness: Some(255),
        scale_brightness: None,
        text: Some("19:30°".to_string()),
    })?;
    thread::sleep(Duration::from_millis(120));
    let st_orion = bridge.request_state()?;
    if st_orion.display_kind != "matrix_21x12"
        || st_orion.screen_width != 21
        || st_orion.screen_height != 12
        || st_orion.screen_pixels.len() != 252
    {
        return Err(format!(
            "[{}] Unexpected orion display metadata: kind={}, {}x{}, len={}",
            board,
            st_orion.display_kind,
            st_orion.screen_width,
            st_orion.screen_height,
            st_orion.screen_pixels.len()
        ));
    }
    let lit_count = st_orion.screen_pixels.iter().filter(|&&px| px > 0).count();
    if lit_count <= 25 {
        return Err(format!(
            "[{}] Expected >25 lit pixels on 21x12 matrix for '19:30°', got {}",
            board, lit_count
        ));
    }

    let mut custom_px = vec![0u8; 252];
    custom_px[0] = 255;
    custom_px[10] = 128;
    custom_px[251] = 64;
    bridge.send_command(&HostCommand::SetClockDisplay {
        segments: None,
        replace: None,
        pixels: Some(custom_px),
        led_frame: None,
        brightness: None,
        scale_brightness: None,
        text: None,
    })?;
    thread::sleep(Duration::from_millis(80));
    let st_orion_px = bridge.request_state()?;
    if st_orion_px.screen_pixels.first().copied() != Some(255)
        || st_orion_px.screen_pixels.get(10).copied() != Some(128)
        || st_orion_px.screen_pixels.get(251).copied() != Some(64)
    {
        return Err(format!(
            "[{}] Custom 21x12 matrix pixels mismatch: [0]={:?}, [10]={:?}, [251]={:?}",
            board,
            st_orion_px.screen_pixels.first(),
            st_orion_px.screen_pixels.get(10),
            st_orion_px.screen_pixels.get(251)
        ));
    }

    adb.shell(
        "echo 180 > /sys/class/leds/orion:red:ambilight0a/brightness && \
         echo 90 > /sys/class/leds/orion:green:ambilight0b/brightness && \
         echo 240 > /sys/class/leds/orion:blue:ambilight0a/brightness && \
         echo 40 > /sys/class/leds/orion:white:ambilight0b/brightness",
        Duration::from_secs(8),
    )?;
    let st_ambi = bridge.request_state()?;
    if st_ambi.aux_light_kind != "ambilight" || st_ambi.logo_pixels.len() != 12 {
        return Err(format!(
            "[{}] Expected aux_light_kind='ambilight' and 12 zones, got {} / {}",
            board,
            st_ambi.aux_light_kind,
            st_ambi.logo_pixels.len()
        ));
    }
    if st_ambi.logo_pixels.first().copied() != Some([200, 110, 255]) {
        return Err(format!(
            "[{}] Unexpected ambilight zone 0: {:?}",
            board,
            st_ambi.logo_pixels.first()
        ));
    }

    if st_ambi.amplifiers.len() != 4 {
        return Err(format!(
            "[{}] Expected 4 ACM8625S amplifiers, got {}",
            board,
            st_ambi.amplifiers.len()
        ));
    }
    for expected_role in ["woofer", "lr", "center", "us"] {
        if !st_ambi.amplifiers.iter().any(|a| a.role == expected_role) {
            return Err(format!(
                "[{}] Missing ACM8625S amplifier role '{}'",
                board, expected_role
            ));
        }
    }
    let woofer = st_ambi
        .amplifiers
        .iter()
        .find(|a| a.role == "woofer")
        .unwrap();
    if woofer.settings_file != "orion-acm8625-woofer.cfg" {
        return Err(format!(
            "[{}] Expected woofer settings_file='orion-acm8625-woofer.cfg', got '{}'",
            board, woofer.settings_file
        ));
    }
    let us = st_ambi
        .ultrasound
        .as_ref()
        .ok_or_else(|| format!("[{}] Missing ultrasound state on orion", board))?;
    if us.sample_rate_hz != 96000 || us.carrier_freq_hz != 38400 {
        return Err(format!(
            "[{}] Unexpected ultrasound state: {:?}",
            board, us
        ));
    }
    let fb_check = adb.shell(
        "cat /sys/class/graphics/fb0/name; cat /sys/class/graphics/fb1/name; cat /sys/class/graphics/fb2/name; \
         aplay -D ultrasound -q -t raw -f S16_LE -c 2 -r 96000 -s 1 /dev/zero && echo ULTRASOUND_PCM_OK",
        Duration::from_secs(8),
    )?;
    if !fb_check.contains("ambilight")
        || !fb_check.contains("YNDX LED screen")
        || !fb_check.contains("ring")
        || !fb_check.contains("ULTRASOUND_PCM_OK")
    {
        return Err(format!(
            "[{}] Orion framebuffer / ultrasound check failed: {}",
            board, fb_check
        ));
    }
    println!(
        "[{}] Orion 21x12 matrix, 12x2 RGBW ambilight, 4x ACM8625S & 96kHz/38.4kHz Ultrasound TDM verified!",
        board
    );
    Ok(())
}

fn verify_sensors_and_voice_tts(
    root: &Path,
    board: &str,
    bridge: &BridgeClient,
    adb: &AdbClient,
) -> Result<(), String> {
    bridge.send_command(&HostCommand::SetSensor {
        temp_c: Some(51.4),
        als_lux: Some(640),
        tof_mm: Some(165),
        doa_angle: Some(90),
        mic_rms: Some(1800),
        swipe: None,
        swipe_direction: None,
    })?;
    let sens_out = adb.shell(
        "cat /sys/class/thermal/thermal_zone0/temp; cat /sys/bus/iio/devices/iio:device0/in_illuminance_raw",
        Duration::from_secs(8),
    )?;
    let sens_lines: Vec<&str> = sens_out.lines().collect();
    if sens_lines.first().map(|s| s.contains("51400")) != Some(true)
        || sens_lines.last().map(|s| s.contains("640")) != Some(true)
    {
        return Err(format!(
            "[{}] Unexpected sensor sysfs: {}",
            board, sens_out
        ));
    }
    println!(
        "[{}] Sensors verified in sysfs: temp=51400 mC, als=640 lux",
        board
    );

    bridge.send_command(&HostCommand::ClearAudio)?;
    let ack_tone = bridge.send_command(&HostCommand::PlayTestTone)?;
    if !ack_tone.ok {
        return Err(format!("[{}] PlayTestTone failed", board));
    }
    bridge.send_command(&HostCommand::InjectVoice {
        phrase: "включи ночной режим".to_string(),
        doa_angle: Some(90),
        rms: None,
        wake_only: false,
    })?;
    bridge.send_command(&HostCommand::IpcTest {
        cmd: "cancel_dialog".to_string(),
        text: None,
    })?;
    let st_hijack = bridge.request_state()?;
    if st_hijack.aliced_hijack_count < 1 || st_hijack.alice_state != "IDLE" {
        return Err(format!(
            "[{}] Expected aliced_hijack_count >= 1 & IDLE after cancel_dialog within 1680ms Hijack Window, got count={} state={}",
            board, st_hijack.aliced_hijack_count, st_hijack.alice_state
        ));
    }
    let log_out = adb.shell("cat /tmp/daemons_logs/maind.log", Duration::from_secs(8))?;
    if !log_out.contains("recognizedPhrase='включи ночной режим'") {
        return Err(format!(
            "[{}] Missing phrase in maind.log: {}",
            board, log_out
        ));
    }

    bridge.send_command(&HostCommand::InjectVoice {
        phrase: "громкость 4".to_string(),
        doa_angle: Some(45),
        rms: None,
        wake_only: false,
    })?;
    thread::sleep(Duration::from_millis(1800));
    let st_voice_vol = bridge.request_state()?;
    if st_voice_vol.volume_level != 4 || st_voice_vol.master_volume != 4000 {
        return Err(format!(
            "[{}] Expected volume_level=4 (4000) after voice 'громкость 4', got {} ({})",
            board, st_voice_vol.volume_level, st_voice_vol.master_volume
        ));
    }
    let vendor_files_out = adb.shell(
        "cat /bkupgrade/data/quasar/last_vendor_volume; echo ''; cat /data/quasar/last_vendor_volume",
        Duration::from_secs(8),
    )?;
    let vendor_vals: Vec<&str> = vendor_files_out
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    if vendor_vals != ["8", "8"] {
        return Err(format!(
            "[{}] Expected vendor volume files to equal ['8', '8'] after 'громкость 4', got {:?}",
            board, vendor_vals
        ));
    }
    bridge.send_command(&HostCommand::IpcTest {
        cmd: "stop_tts".to_string(),
        text: None,
    })?;
    thread::sleep(Duration::from_millis(100));
    println!(
        "[{}] Voice Input -> /tmp/daemons_logs/maind.log, 1680ms Hijack Window & voice volume ('громкость 4' -> 4/10) verified!",
        board
    );

    bridge.send_command(&HostCommand::IpcTest {
        cmd: "vocalize".to_string(),
        text: Some(
            "Привет! Сегодня в Москве плюс 21 градус, без осадков. Включить музыку?".to_string(),
        ),
    })?;
    thread::sleep(Duration::from_millis(1250));
    let st_tts = bridge.request_state()?;
    if st_tts.alice_state != "SPEAKING" || st_tts.aliced_say_count < 1 {
        return Err(format!(
            "[{}] Expected AliceState=SPEAKING at 1.25s during long TTS phrase, got {} (say_count={})",
            board, st_tts.alice_state, st_tts.aliced_say_count
        ));
    }
    bridge.send_command(&HostCommand::IpcTest {
        cmd: "stop_tts".to_string(),
        text: None,
    })?;
    thread::sleep(Duration::from_millis(150));
    let st_stopped = bridge.request_state()?;
    if st_stopped.alice_state != "IDLE" {
        return Err(format!(
            "[{}] Expected IDLE after STOP_TTS, got {}",
            board, st_stopped.alice_state
        ));
    }

    for burst_i in 0..4 {
        bridge.send_command(&HostCommand::IpcTest {
            cmd: "vocalize".to_string(),
            text: Some(format!("Тест {}", burst_i)),
        })?;
        thread::sleep(Duration::from_millis(50));
    }
    let mut returned_idle = false;
    for _ in 0..35 {
        thread::sleep(Duration::from_millis(150));
        if bridge.request_state()?.alice_state == "IDLE" {
            returned_idle = true;
            break;
        }
    }
    if !returned_idle {
        return Err(format!(
            "[{}] Expected AliceState to return to IDLE after rapid START_VOCALIZE burst",
            board
        ));
    }

    thread::sleep(Duration::from_millis(200));
    let final_st = bridge.request_state()?;
    let captured_wav = bridge.captured_wav_bytes();
    if captured_wav.len() <= 32000 || &captured_wav[0..4] != b"RIFF" {
        return Err(format!(
            "[{}] Expected captured TTS WAV > 32000B from snd-aloop PcmAudioChunk stream, got {}B (guest captured_audio_bytes={})",
            board,
            captured_wav.len(),
            final_st.captured_audio_bytes
        ));
    }
    let out_wav = root.join("work").join(format!("{}-captured.wav", board));
    fs::write(&out_wav, &captured_wav)
        .map_err(|e| format!("[{}] Failed to write {}: {}", board, out_wav.display(), e))?;
    if final_st.ipc_error_count != 0 {
        return Err(format!(
            "[{}] Unexpected IPC errors in logs: {:?}",
            board, final_st.recent_logs
        ));
    }
    println!(
        "[{}] Expressive Formant TTS, STOP_TTS interrupt, ALSA snd-aloop capture & 0 IPC errors verified ({} bytes WAV -> {})!",
        board,
        captured_wav.len(),
        out_wav
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("captured.wav")
    );
    Ok(())
}

fn wait_for_serial_active_line(
    session: &EmulatorSession,
    attempts: usize,
    step_ms: u64,
    pred: impl Fn(&str) -> bool,
) -> bool {
    for _ in 0..attempts {
        if pred(&session.serial_snapshot().active_line) {
            return true;
        }
        thread::sleep(Duration::from_millis(step_ms));
    }
    pred(&session.serial_snapshot().active_line)
}

fn wait_for_serial_completed_line(
    session: &EmulatorSession,
    attempts: usize,
    step_ms: u64,
    expected: &str,
) -> bool {
    for _ in 0..attempts {
        thread::sleep(Duration::from_millis(step_ms));
        if session.serial_lines().iter().any(|l| l.trim() == expected) {
            return true;
        }
    }
    false
}

fn verify_native_serial_shell(board: &str, session: &EmulatorSession) -> Result<(), String> {
    let expected_root_prompt = format!("root@qemu-{}:/#", board);
    let mut prompt_ready =
        wait_for_serial_active_line(session, 25, 80, |line| line.contains(&expected_root_prompt));
    if !prompt_ready {
        session.send_serial_input("\n")?;
        prompt_ready = wait_for_serial_active_line(session, 15, 80, |line| {
            line.contains(&expected_root_prompt)
        });
    }
    if !prompt_ready {
        return Err(format!(
            "[{}] Expected native /bin/sh prompt '{}' on ttyAMA0, got active_line='{}'",
            board,
            expected_root_prompt,
            session.serial_snapshot().active_line
        ));
    }

    // Verify native BusyBox ash Tab completion (`cd /sys/cl\t` -> `/sys/class/`) and live CWD prompt
    session.send_serial_input("cd /sys/cl\t")?;
    let expected_completed_line = format!("root@qemu-{}:/# cd /sys/class/", board);
    if !wait_for_serial_active_line(session, 20, 60, |line| {
        line.contains(&expected_completed_line)
    }) {
        return Err(format!(
            "[{}] Native /bin/sh Tab completion ('cd /sys/cl\\t') did not produce '{}', active_line='{}'",
            board,
            expected_completed_line,
            session.serial_snapshot().active_line
        ));
    }

    session.send_serial_input("\n")?;
    let expected_sub_prompt = format!("root@qemu-{}:/sys/class#", board);
    if !wait_for_serial_active_line(session, 20, 60, |line| line.contains(&expected_sub_prompt)) {
        return Err(format!(
            "[{}] Native /bin/sh Tab completion ('cd /sys/cl\\t') did not reach '{}', active_line='{}'",
            board,
            expected_sub_prompt,
            session.serial_snapshot().active_line
        ));
    }

    // Verify Ctrl+D (0x04 / EOF) at an empty shell prompt cleanly exits `/sys/class` shell and respawns `/bin/sh -i` at `/`
    session.send_serial_input("\x04")?;
    if !wait_for_serial_active_line(session, 30, 60, |line| line.contains(&expected_root_prompt)) {
        return Err(format!(
            "[{}] Ctrl+D (0x04 / EOF) at empty prompt did not exit and respawn `/bin/sh -i` at '/', active_line='{}'",
            board,
            session.serial_snapshot().active_line
        ));
    }

    session.send_serial_input("echo Привет_UTF8\n")?;
    if !wait_for_serial_completed_line(session, 20, 60, "Привет_UTF8") {
        return Err(format!(
            "[{}] Expected intact UTF-8 output 'Привет_UTF8' on ttyAMA0, got {:?}",
            board,
            session.serial_lines().iter().rev().take(6).collect::<Vec<_>>()
        ));
    }

    // Verify Ctrl+C (0x03) at the interactive prompt cancels the current input line without executing it
    session.send_serial_input("echo SHOULD_NOT_RUN_ON_CTRL_C")?;
    thread::sleep(Duration::from_millis(120));
    session.send_serial_input("\x03")?;
    thread::sleep(Duration::from_millis(120));
    session.send_serial_input("echo AFTER_PROMPT_CTRL_C\n")?;
    let prompt_ctrl_c_ok = wait_for_serial_completed_line(session, 25, 60, "AFTER_PROMPT_CTRL_C");
    if !prompt_ctrl_c_ok
        || session
            .serial_lines()
            .iter()
            .any(|l| l.trim() == "SHOULD_NOT_RUN_ON_CTRL_C")
    {
        return Err(format!(
            "[{}] Ctrl+C (0x03) at prompt did not cancel pending command line on ttyAMA0, lines={:?}",
            board,
            session.serial_lines().iter().rev().take(6).collect::<Vec<_>>()
        ));
    }

    // Verify Ctrl+C (0x03 / SIGINT) interrupts a blocking foreground command (`sleep 30`)
    session.send_serial_input("sleep 30\n")?;
    thread::sleep(Duration::from_millis(180));
    session.send_serial_input("\x03")?;
    if !wait_for_serial_active_line(session, 25, 60, |line| {
        line.contains(&expected_root_prompt) && !line.contains("sleep")
    }) {
        return Err(format!(
            "[{}] Ctrl+C (0x03 / SIGINT) did not interrupt foreground `sleep 30` on ttyAMA0, active_line='{}'",
            board,
            session.serial_snapshot().active_line
        ));
    }

    // Verify Ctrl+C (0x03) exits full-screen raw-mode `top`
    session.send_serial_input("top\n")?;
    thread::sleep(Duration::from_millis(250));
    session.send_serial_input("\x03")?;
    if !wait_for_serial_active_line(session, 25, 60, |line| {
        line.contains(&expected_root_prompt) && !line.contains("top")
    }) {
        return Err(format!(
            "[{}] Ctrl+C (0x03) did not exit foreground `top` on ttyAMA0, active_line='{}'",
            board,
            session.serial_snapshot().active_line
        ));
    }

    // Verify Ctrl+D (0x04 / VEOF) terminates a foreground stdin reader (`cat`)
    session.send_serial_input("cat\n")?;
    thread::sleep(Duration::from_millis(180));
    session.send_serial_input("\x04")?;
    if !wait_for_serial_active_line(session, 25, 60, |line| {
        line.contains(&expected_root_prompt) && !line.contains("cat")
    }) {
        return Err(format!(
            "[{}] Ctrl+D (0x04 / EOF) did not terminate foreground `cat` on ttyAMA0, active_line='{}'",
            board,
            session.serial_snapshot().active_line
        ));
    }

    if session
        .serial_lines()
        .iter()
        .any(|l| l.contains("can't access tty"))
    {
        return Err(format!(
            "[{}] Unexpected 'can't access tty' on ttyAMA0 (missing setsid/TIOCSCTTY)",
            board
        ));
    }
    println!(
        "[{}] Native /bin/sh on ttyAMA0 (setsid/TIOCSCTTY, Ctrl+C/Ctrl+D, live prompt & Tab completion) verified!",
        board
    );
    Ok(())
}

pub fn run_board_verification(
    root: &Path,
    board: &str,
    drive_img: &Path,
    bridge_port: u16,
    adb_port: u16,
    snapshot: bool,
) -> Result<(), String> {
    let cfg = SessionConfig {
        root_dir: root.to_path_buf(),
        board: board.to_string(),
        board_cmdline: "auto".to_string(),
        mode: "stub".to_string(),
        snapshot,
        persist_data: false,
        bind_all: false,
        lan_tap: None,
        lan_mac: None,
        port_offset: 0,
        bridge_port,
        adb_port,
        sdk_port: None,
        drive_img: drive_img.to_path_buf(),
    };

    println!(
        "\n=== [TEST] Starting KUYASE for board='{}' (Twin :{}, ADB :{}) ===",
        board, bridge_port, adb_port
    );

    let mut session = EmulatorSession::start(cfg)?;

    let res = (|| -> Result<(), String> {
        let bridge_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), bridge_port);
        let bridge = BridgeClient::connect_with_timeout(
            bridge_addr,
            crate::session::bridge_connect_timeout(),
        )?;
        verify_bridge_and_profile(board, &bridge)?;

        let adb = AdbClient::from_host_port("127.0.0.1", adb_port)?;
        let meta = verify_adb_and_hw_json(board, &adb)?;
        verify_native_serial_shell(board, &session)?;

        if meta.led_driver == "i2c_sn3236" {
            verify_sn3236_i2c_ring(board, &bridge, &adb)?;
        } else {
            verify_sysfs_fb_ring_and_conflict(board, &meta, &bridge, &adb)?;
        }

        verify_buttons_and_volume(board, &bridge, &adb)?;
        verify_segment_clock_and_aux_logo(board, &meta, &bridge, &adb)?;

        if board == "bergamot" {
            verify_bergamot_peripherals(board, &adb)?;
        }
        if board == "mango" {
            verify_mango_peripherals(board, &adb)?;
        }
        if board == "orion" {
            verify_orion_peripherals(board, &bridge, &adb)?;
        }

        verify_sensors_and_voice_tts(root, board, &bridge, &adb)?;

        bridge.close();
        Ok(())
    })();

    session.stop();
    res
}
