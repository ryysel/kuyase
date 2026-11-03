use std::fs::{File, OpenOptions};
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(target_os = "linux")]
extern "C" {
    fn ioctl(fd: i32, request: u32, ...) -> i32;
    fn setsid() -> i32;
    fn signal(signum: i32, handler: usize) -> usize;
    fn sigprocmask(how: i32, set: *const u64, oldset: *mut u64) -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct Winsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct UinputUserDev {
    name: [u8; 80],
    id_bustype: u16,
    id_vendor: u16,
    id_product: u16,
    id_version: u16,
    ff_effects_max: u32,
    absmax: [i32; 64],
    absmin: [i32; 64],
    absfuzz: [i32; 64],
    absflat: [i32; 64],
}

#[cfg(target_os = "linux")]
pub fn create_uinput_device(
    name: &str,
    ev_bits: &[u32],
    key_bits: &[u32],
    abs_bits: &[(u32, i32)],
) -> Option<File> {
    use std::os::unix::io::AsRawFd;

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/uinput")
        .ok()?;
    let fd = file.as_raw_fd();

    const UI_SET_EVBIT: u32 = 0x4004_5564;
    const UI_SET_KEYBIT: u32 = 0x4004_5565;
    const UI_SET_ABSBIT: u32 = 0x4004_5567;
    const UI_SET_SWBIT: u32 = 0x4004_5569;
    const UI_DEV_CREATE: u32 = 0x5501;

    unsafe {
        for &ev in ev_bits {
            ioctl(fd, UI_SET_EVBIT, ev);
        }
        for &k in key_bits {
            if ev_bits.contains(&5) {
                ioctl(fd, UI_SET_SWBIT, k);
            } else {
                ioctl(fd, UI_SET_KEYBIT, k);
            }
        }
        for &(abs_code, _) in abs_bits {
            ioctl(fd, UI_SET_ABSBIT, abs_code);
        }
    }

    let mut udev = UinputUserDev {
        name: [0u8; 80],
        id_bustype: 0x0010,
        id_vendor: 0x0001,
        id_product: 0x0001,
        id_version: 0x0100,
        ff_effects_max: 0,
        absmax: [0i32; 64],
        absmin: [0i32; 64],
        absfuzz: [0i32; 64],
        absflat: [0i32; 64],
    };
    let name_bytes = name.as_bytes();
    let copy_len = name_bytes.len().min(79);
    udev.name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
    for &(abs_code, max_val) in abs_bits {
        if (abs_code as usize) < 64 {
            udev.absmax[abs_code as usize] = max_val;
        }
    }

    let udev_slice = unsafe {
        std::slice::from_raw_parts(
            &udev as *const UinputUserDev as *const u8,
            std::mem::size_of::<UinputUserDev>(),
        )
    };
    file.write_all(udev_slice).ok()?;
    let ret = unsafe { ioctl(fd, UI_DEV_CREATE) };
    if ret < 0 {
        return None;
    }
    Some(file)
}

#[cfg(not(target_os = "linux"))]
pub fn create_uinput_device(
    _name: &str,
    _ev_bits: &[u32],
    _key_bits: &[u32],
    _abs_bits: &[(u32, i32)],
) -> Option<File> {
    None
}

pub fn emit_compat_input_event(file: &mut File, ev_type: u16, code: u16, value: i32) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let sec = now.as_secs() as u32;
    let usec = now.subsec_micros();
    let mut buf = [0u8; 16];
    buf[0..4].copy_from_slice(&sec.to_le_bytes());
    buf[4..8].copy_from_slice(&usec.to_le_bytes());
    buf[8..10].copy_from_slice(&ev_type.to_le_bytes());
    buf[10..12].copy_from_slice(&code.to_le_bytes());
    buf[12..16].copy_from_slice(&value.to_le_bytes());
    let _ = file.write_all(&buf);
    let _ = file.flush();
}

pub fn spawn_button_press_uinput(
    state: std::sync::Arc<crate::state::BridgeState>,
    btn: &str,
    act: &str,
) {
    let btn_s = btn.to_string();
    let act_s = act.to_string();
    thread::spawn(move || {
        let code: u16 = match btn_s.as_str() {
            "volume_up" => 115,
            "volume_down" => 114,
            "mute" => 113,
            _ => 582,
        };
        let clicks = match act_s.as_str() {
            "double" => 2,
            "triple" => 3,
            _ => 1,
        };
        let hold_ms = if act_s == "long" { 900 } else { 50 };
        for c in 0..clicks {
            if let Some(ref mut f) = *state.key_uinput.lock().unwrap() {
                emit_compat_input_event(f, 1, code, 1);
                emit_compat_input_event(f, 0, 0, 0);
            }
            thread::sleep(Duration::from_millis(hold_ms));
            if let Some(ref mut f) = *state.key_uinput.lock().unwrap() {
                emit_compat_input_event(f, 1, code, 0);
                emit_compat_input_event(f, 0, 0, 0);
            }
            if c + 1 < clicks {
                thread::sleep(Duration::from_millis(90));
            }
        }
    });
}

pub fn spawn_tof_swipe_uinput(
    state: std::sync::Arc<crate::state::BridgeState>,
    dir_code: i32,
    restore_tof: u32,
) {
    use std::sync::atomic::Ordering;
    thread::spawn(move || {
        if let Some(ref mut f) = *state.tof_uinput.lock().unwrap() {
            emit_compat_input_event(f, 3, 0x19, 0);
            emit_compat_input_event(f, 3, 0x12, 140);
            if dir_code != 0 {
                emit_compat_input_event(f, 3, 0x18, dir_code);
            }
            emit_compat_input_event(f, 0, 0, 0);
        }
        state.tof_mm.store(140, Ordering::SeqCst);
        thread::sleep(Duration::from_millis(110));
        if let Some(ref mut f) = *state.tof_uinput.lock().unwrap() {
            emit_compat_input_event(f, 3, 0x19, if restore_tof < 500 { 0 } else { 4 });
            emit_compat_input_event(f, 3, 0x12, restore_tof as i32);
            emit_compat_input_event(f, 0, 0, 0);
        }
        state.tof_mm.store(restore_tof, Ordering::SeqCst);
    });
}

pub fn write_tof_distance_uinput(state: &crate::state::BridgeState, tof: u32) {
    use std::sync::atomic::Ordering;
    let mm = tof as i32;
    state.tof_mm.store(tof, Ordering::SeqCst);
    if let Some(ref mut f) = *state.tof_uinput.lock().unwrap() {
        emit_compat_input_event(f, 3, 0x19, if mm < 500 { 0 } else { 4 });
        emit_compat_input_event(f, 3, 0x12, mm);
        emit_compat_input_event(f, 0, 0, 0);
    }
}

fn reset_tty_ama0_termios() {
    let _ = Command::new("stty")
        .args([
            "-F",
            "/dev/ttyAMA0",
            "sane",
            "rows",
            "40",
            "cols",
            "120",
            "isig",
            "icanon",
            "echo",
            "echoe",
            "echok",
            "icrnl",
            "onlcr",
            "intr",
            "^C",
            "quit",
            "^\\",
            "erase",
            "^?",
            "kill",
            "^U",
            "eof",
            "^D",
            "susp",
            "^Z",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = Command::new("stty")
        .args(["-F", "/dev/ttyAMA0", "iutf8"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(target_os = "linux")]
unsafe fn reset_child_signal_dispositions() {
    const SIG_SETMASK: i32 = 2;
    const SIG_DFL: usize = 0;
    let empty_mask = [0u64; 16];
    sigprocmask(SIG_SETMASK, empty_mask.as_ptr(), std::ptr::null_mut());
    // Backgrounded processes in non-interactive init scripts inherit
    // SIGINT/SIGQUIT = SIG_IGN (and Rust std sets SIGPIPE = SIG_IGN),
    // which causes BusyBox ash to mark SIGINT/SIGQUIT as S_HARD_IGN.
    // Reset all standard job-control and termination signals to SIG_DFL.
    for sig in [1, 2, 3, 13, 15, 17, 18, 20, 21, 22] {
        signal(sig, SIG_DFL);
    }
}

pub fn spawn_serial_console_watcher() {
    thread::spawn(|| {
        thread::sleep(Duration::from_millis(600));
        // Suppress post-boot KERN_INFO/KERN_WARNING spam (e.g. ALSA ShareContextCreate / oom_adj)
        // from corrupting the interactive ttyAMA0 shell prompt; full log remains in `dmesg`.
        let _ = std::fs::write("/proc/sys/kernel/printk", "3 4 1 3\n");
        reset_tty_ama0_termios();
        if let Ok(mut tty) = OpenOptions::new().write(true).open("/dev/ttyAMA0") {
            let _ = tty.write_all(
                b"\r\n=== [ttyAMA0] Root shell active (pure serial console, no ADB required) ===\r\n",
            );
            let _ = tty.flush();
        }
        loop {
            reset_tty_ama0_termios();
            if let (Ok(sin), Ok(sout), Ok(serr)) = (
                OpenOptions::new().read(true).write(true).open("/dev/ttyAMA0"),
                OpenOptions::new().read(true).write(true).open("/dev/ttyAMA0"),
                OpenOptions::new().read(true).write(true).open("/dev/ttyAMA0"),
            ) {
                let mut cmd = Command::new("/bin/sh");
                cmd.arg("-i")
                    .current_dir("/")
                    .env("TERM", "vt100")
                    .env("LANG", "C.UTF-8")
                    .env("LC_ALL", "C.UTF-8")
                    .env("HOME", "/root")
                    .env("USER", "root")
                    .env("LOGNAME", "root")
                    .env("PS1", "\\u@\\h:\\w# ")
                    .env(
                        "PATH",
                        "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/system/vendor/quasar:/data/yam/bin",
                    )
                    .stdin(Stdio::from(sin))
                    .stdout(Stdio::from(sout))
                    .stderr(Stdio::from(serr));

                #[cfg(target_os = "linux")]
                unsafe {
                    use std::os::unix::process::CommandExt;
                    const TIOCSCTTY: u32 = 0x540E;
                    const TIOCSPGRP: u32 = 0x5410;
                    const TIOCSWINSZ: u32 = 0x5414;
                    cmd.pre_exec(|| {
                        let sid = setsid();
                        ioctl(0, TIOCSCTTY, 1);
                        if sid > 0 {
                            ioctl(0, TIOCSPGRP, &sid as *const i32);
                        }
                        reset_child_signal_dispositions();
                        let ws = Winsize {
                            ws_row: 40,
                            ws_col: 120,
                            ws_xpixel: 0,
                            ws_ypixel: 0,
                        };
                        ioctl(0, TIOCSWINSZ, &ws as *const Winsize);
                        Ok(())
                    });
                }

                if let Ok(mut child) = cmd.spawn() {
                    drop(cmd);
                    let _ = child.wait();
                }
                if let Ok(mut tty) = OpenOptions::new().write(true).open("/dev/ttyAMA0") {
                    let _ = tty.write_all(b"\r\n");
                    let _ = tty.flush();
                }
            }
            thread::sleep(Duration::from_millis(80));
        }
    });
}

pub fn ensure_adbd_signals_unignored() {
    #[cfg(target_os = "linux")]
    {
        if ! std::path::Path::new("/usr/bin/adbd").exists() {
            return;
        }
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return;
        };
        let mut need_respawn = false;
        for entry in entries.flatten() {
            let fname = entry.file_name();
            let Some(pid_str) = fname.to_str() else {
                continue;
            };
            let Ok(pid) = pid_str.parse::<i32>() else {
                continue;
            };
            let comm_path = format!("/proc/{}/comm", pid);
            let Ok(comm) = std::fs::read_to_string(&comm_path) else {
                continue;
            };
            if comm.trim() != "adbd" {
                continue;
            }
            let status_path = format!("/proc/{}/status", pid);
            if let Ok(status) = std::fs::read_to_string(&status_path) {
                for line in status.lines() {
                    if let Some(hex_str) = line.strip_prefix("SigIgn:") {
                        if let Ok(mask) = u64::from_str_radix(hex_str.trim(), 16) {
                            if (mask & 0x6) != 0 {
                                unsafe {
                                    kill(pid, 9);
                                }
                                need_respawn = true;
                            }
                        }
                    }
                }
            }
        }
        if need_respawn {
            thread::sleep(Duration::from_millis(20));
            let log_out = OpenOptions::new()
                .create(true)
                .append(true)
                .open("/tmp/adbd.log")
                .map(Stdio::from)
                .unwrap_or_else(|_| Stdio::null());
            let log_err = OpenOptions::new()
                .create(true)
                .append(true)
                .open("/tmp/adbd.log")
                .map(Stdio::from)
                .unwrap_or_else(|_| Stdio::null());
            let mut cmd = Command::new("/usr/bin/adbd");
            cmd.stdin(Stdio::null()).stdout(log_out).stderr(log_err);
            unsafe {
                use std::os::unix::process::CommandExt;
                cmd.pre_exec(|| {
                    setsid();
                    reset_child_signal_dispositions();
                    Ok(())
                });
            }
            let _ = cmd.spawn();
        }
    }
}
