use super::{find_framebuffer_by_name, read_active_trigger, read_sysfs_u8};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

pub struct RingReadout {
    pub pixels: Vec<[u8; 3]>,
    pub trigger: String,
}

pub trait LedRingDevice: Send + Sync {
    fn driver_name(&self) -> &'static str;
    fn init(&self);
    fn read_ring(&self, native_frame: &[[u8; 3]], conflict_counter: &AtomicU32) -> RingReadout;
    fn write_native_frame(&self, px: &[[u8; 3]]);
}

#[cfg(target_os = "linux")]
mod i2c_sn3236 {
    use std::fs::OpenOptions;
    use std::os::unix::io::AsRawFd;
    use std::sync::{Mutex, OnceLock};

    extern "C" {
        fn ioctl(fd: i32, request: u32, ...) -> i32;
    }

    const I2C_RDWR: u32 = 0x0707;
    const I2C_M_RD: u16 = 0x0001;
    const ADDR: u16 = 0x003c;
    const IDLE_SENTINEL: u8 = 0x5a;

    struct Sn3236Latched {
        pwm: [u8; 18],
        ctrl: [u8; 18],
    }

    fn latched_state() -> &'static Mutex<Sn3236Latched> {
        static STATE: OnceLock<Mutex<Sn3236Latched>> = OnceLock::new();
        STATE.get_or_init(|| {
            Mutex::new(Sn3236Latched {
                pwm: [0u8; 18],
                ctrl: [0x01u8; 18],
            })
        })
    }

    #[repr(C)]
    pub struct I2cMsg {
        pub addr: u16,
        pub flags: u16,
        pub len: u16,
        pub buf: *mut u8,
    }

    #[repr(C)]
    struct I2cRdwrIoctlData {
        msgs: *mut I2cMsg,
        nmsgs: u32,
    }

    fn run_i2c_rdwr(msgs: &mut [I2cMsg]) -> bool {
        let Ok(file) = OpenOptions::new().read(true).write(true).open("/dev/i2c-0") else {
            return false;
        };
        let mut rdwr = I2cRdwrIoctlData {
            msgs: msgs.as_mut_ptr(),
            nmsgs: msgs.len() as u32,
        };
        let ret = unsafe { ioctl(file.as_raw_fd(), I2C_RDWR, &mut rdwr as *mut I2cRdwrIoctlData) };
        ret >= 0
    }

    fn scale_channel(pwm: u8, ctrl: u8, chip_enabled: bool) -> (u8, bool) {
        let out_en = chip_enabled && ((ctrl & 0x01) != 0);
        if !out_en {
            return (0, false);
        }
        let div: u16 = match (ctrl >> 1) & 0x03 {
            0 => 1,
            1 => 2,
            2 => 3,
            _ => 4,
        };
        (((pwm as u16) / div) as u8, true)
    }

    pub fn read_ring() -> Option<([[u8; 3]; 6], bool)> {
        let mut reg_addr = [0x00u8];
        let mut reg_data = [0u8; 76];
        let mut msgs = [
            I2cMsg {
                addr: ADDR,
                flags: 0,
                len: 1,
                buf: reg_addr.as_mut_ptr(),
            },
            I2cMsg {
                addr: ADDR,
                flags: I2C_M_RD,
                len: reg_data.len() as u16,
                buf: reg_data.as_mut_ptr(),
            },
        ];
        if !run_i2c_rdwr(&mut msgs) {
            return None;
        }

        let mut latched = latched_state().lock().unwrap();

        if reg_data[0x4a] != IDLE_SENTINEL {
            latched.pwm = [0u8; 18];
            latched.ctrl = [0u8; 18];
            reg_data.fill(0);
            let mut clear_buf = [0u8; 77];
            clear_buf[0] = 0x00;
            clear_buf[1 + 0x25] = IDLE_SENTINEL;
            clear_buf[1 + 0x4a] = IDLE_SENTINEL;
            let mut clear_msg = [I2cMsg {
                addr: ADDR,
                flags: 0,
                len: clear_buf.len() as u16,
                buf: clear_buf.as_mut_ptr(),
            }];
            let _ = run_i2c_rdwr(&mut clear_msg);
        } else if reg_data[0x25] != IDLE_SENTINEL {
            latched.pwm.copy_from_slice(&reg_data[0x01..=0x12]);
            latched.ctrl.copy_from_slice(&reg_data[0x26..=0x37]);
            let mut rearm = [0x25u8, IDLE_SENTINEL];
            let mut rearm_msg = [I2cMsg {
                addr: ADDR,
                flags: 0,
                len: 2,
                buf: rearm.as_mut_ptr(),
            }];
            let _ = run_i2c_rdwr(&mut rearm_msg);
        }

        let chip_enabled = (reg_data[0x00] & 0x01) != 0;
        let mut ring = [[0u8; 3]; 6];
        let mut any_ch_disabled = false;
        for i in 0..6 {
            let ch = i * 3;
            let (b, b_en) = scale_channel(latched.pwm[ch], latched.ctrl[ch], chip_enabled);
            let (g, g_en) = scale_channel(latched.pwm[ch + 1], latched.ctrl[ch + 1], chip_enabled);
            let (r, r_en) = scale_channel(latched.pwm[ch + 2], latched.ctrl[ch + 2], chip_enabled);
            if !b_en || !g_en || !r_en {
                any_ch_disabled = true;
            }
            ring[i] = [r, g, b];
        }
        let stealth = !chip_enabled || any_ch_disabled;
        Some((ring, stealth))
    }

    pub fn init_default() {
        {
            let mut latched = latched_state().lock().unwrap();
            latched.pwm = [0u8; 18];
            latched.ctrl = [0x01u8; 18];
        }
        let mut en_buf = [0x00u8, 0x01u8];
        let mut ctrl_buf = [0u8; 19];
        ctrl_buf[0] = 0x26;
        for b in &mut ctrl_buf[1..] {
            *b = 0x01;
        }
        let mut sentinel_25 = [0x25u8, IDLE_SENTINEL];
        let mut sentinel_4a = [0x4au8, IDLE_SENTINEL];
        let mut msgs = [
            I2cMsg {
                addr: ADDR,
                flags: 0,
                len: 2,
                buf: en_buf.as_mut_ptr(),
            },
            I2cMsg {
                addr: ADDR,
                flags: 0,
                len: ctrl_buf.len() as u16,
                buf: ctrl_buf.as_mut_ptr(),
            },
            I2cMsg {
                addr: ADDR,
                flags: 0,
                len: 2,
                buf: sentinel_25.as_mut_ptr(),
            },
            I2cMsg {
                addr: ADDR,
                flags: 0,
                len: 2,
                buf: sentinel_4a.as_mut_ptr(),
            },
        ];
        let _ = run_i2c_rdwr(&mut msgs);

        std::thread::spawn(|| loop {
            let _ = read_ring();
            std::thread::sleep(std::time::Duration::from_millis(1));
        });
    }

    pub fn write_pwm(pixels: &[[u8; 3]]) {
        let mut pwm_buf = [0u8; 19];
        pwm_buf[0] = 0x01;
        for i in 0..6.min(pixels.len()) {
            let [r, g, b] = pixels[i];
            pwm_buf[1 + i * 3] = b;
            pwm_buf[1 + i * 3 + 1] = g;
            pwm_buf[1 + i * 3 + 2] = r;
        }
        let mut upd_buf = [0x25u8, 0xaa];
        let mut msgs = [
            I2cMsg {
                addr: ADDR,
                flags: 0,
                len: pwm_buf.len() as u16,
                buf: pwm_buf.as_mut_ptr(),
            },
            I2cMsg {
                addr: ADDR,
                flags: 0,
                len: 2,
                buf: upd_buf.as_mut_ptr(),
            },
        ];
        let _ = run_i2c_rdwr(&mut msgs);
        let _ = read_ring();
    }
}

pub struct Sn3236I2cRing;

impl LedRingDevice for Sn3236I2cRing {
    fn driver_name(&self) -> &'static str {
        "i2c_sn3236"
    }

    fn init(&self) {
        #[cfg(target_os = "linux")]
        i2c_sn3236::init_default();
    }

    fn read_ring(&self, _native_frame: &[[u8; 3]], _conflict_counter: &AtomicU32) -> RingReadout {
        #[cfg(target_os = "linux")]
        if let Some((ring6, stealth)) = i2c_sn3236::read_ring() {
            return RingReadout {
                pixels: ring6.to_vec(),
                trigger: if stealth {
                    "i2c-0 SN3236 (Stealth Locked)".to_string()
                } else {
                    "i2c-0 SN3236 (0x3c Active)".to_string()
                },
            };
        }
        RingReadout {
            pixels: vec![[0u8; 3]; 6],
            trigger: "i2c-0 SN3236".to_string(),
        }
    }

    fn write_native_frame(&self, px: &[[u8; 3]]) {
        #[cfg(target_os = "linux")]
        i2c_sn3236::write_pwm(px);
        #[cfg(not(target_os = "linux"))]
        let _ = px;
    }
}

pub struct SysfsFbLedRing {
    pub sysfs_prefix: String,
    pub leds: usize,
    pub fallback_fb_dev: String,
    pub driver_label: &'static str,
}

impl SysfsFbLedRing {
    fn ring_fb_path(&self) -> String {
        find_framebuffer_by_name("ring", &self.fallback_fb_dev)
    }
}

impl LedRingDevice for SysfsFbLedRing {
    fn driver_name(&self) -> &'static str {
        self.driver_label
    }

    fn init(&self) {
        let fb_path = self.ring_fb_path();
        if !Path::new(&fb_path).exists() {
            let _ = fs::write(&fb_path, vec![0u8; self.leds * 3]);
        }
    }

    fn read_ring(&self, native_frame: &[[u8; 3]], conflict_counter: &AtomicU32) -> RingReadout {
        let prefix = &self.sysfs_prefix;
        let first = if Path::new(&format!(
            "/sys/class/leds/{}:red:ring0/brightness",
            prefix
        ))
        .exists()
        {
            0
        } else {
            1
        };

        let trig = read_active_trigger(&format!(
            "/sys/class/leds/{}:red:ring{}/trigger",
            prefix, first
        ));
        let trigger = if trig == "none" {
            "none (Custom Override)".to_string()
        } else {
            format!("{} (Quasar Native)", trig)
        };

        let mut any_sysfs = false;
        let mut sysfs_pixels = vec![[0u8; 3]; self.leds];
        for (i, slot) in sysfs_pixels.iter_mut().enumerate().take(self.leds) {
            let idx = i + first;
            let r = read_sysfs_u8(&format!(
                "/sys/class/leds/{}:red:ring{}/brightness",
                prefix, idx
            ));
            let g = read_sysfs_u8(&format!(
                "/sys/class/leds/{}:green:ring{}/brightness",
                prefix, idx
            ));
            let b = read_sysfs_u8(&format!(
                "/sys/class/leds/{}:blue:ring{}/brightness",
                prefix, idx
            ));
            if r > 0 || g > 0 || b > 0 {
                any_sysfs = true;
            }
            *slot = [r, g, b];
        }

        if trig == "none" {
            return RingReadout {
                pixels: sysfs_pixels,
                trigger,
            };
        }

        if any_sysfs {
            for i in first..(first + self.leds) {
                for c in ["red", "green", "blue"] {
                    let _ = fs::write(
                        format!("/sys/class/leds/{}:{}:ring{}/brightness", prefix, c, i),
                        "0",
                    );
                }
            }
            conflict_counter.fetch_add(1, Ordering::SeqCst);
        }

        let any_native = native_frame
            .iter()
            .any(|px| px[0] > 0 || px[1] > 0 || px[2] > 0);
        if any_native && native_frame.len() == self.leds {
            return RingReadout {
                pixels: native_frame.to_vec(),
                trigger,
            };
        }

        let mut ring_pixels = vec![[0u8; 3]; self.leds];
        let fb_path = self.ring_fb_path();
        if let Ok(raw) = fs::read(&fb_path) {
            if raw.len() == (self.leds + 1) * 4 || raw.len() == self.leds * 4 {
                for (i, slot) in ring_pixels.iter_mut().enumerate().take(self.leds) {
                    let base = i * 4;
                    let w = raw[base + 3] / 2;
                    *slot = [
                        raw[base].saturating_add(w),
                        raw[base + 1].saturating_add(w),
                        raw[base + 2].saturating_add(w),
                    ];
                }
            } else if raw.len() >= self.leds * 3 {
                for (i, slot) in ring_pixels.iter_mut().enumerate().take(self.leds) {
                    let base = i * 3;
                    *slot = [raw[base], raw[base + 1], raw[base + 2]];
                }
            }
        }

        RingReadout {
            pixels: ring_pixels,
            trigger,
        }
    }

    fn write_native_frame(&self, px: &[[u8; 3]]) {
        if self.driver_label == "framebuffer" {
            let fb_path = self.ring_fb_path();
            let mut raw = Vec::with_capacity(self.leds * 3);
            for i in 0..self.leds {
                let c = px.get(i).copied().unwrap_or([0, 0, 0]);
                raw.extend_from_slice(&c);
            }
            let _ = fs::write(fb_path, raw);
        }
    }
}
