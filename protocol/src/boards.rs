use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoardDescriptor {
    pub slug: &'static str,
    pub board_name: &'static str,
    pub gui_label: &'static str,
    pub sysfs_prefix: &'static str,
    pub ring_leds: usize,
    pub ring_driver: &'static str,
    pub ring_fb_dev: &'static str,
    pub logo_leds: usize,
    pub ambilight_leds: usize,
    pub has_clock: bool,
    pub has_matrix_21x12: bool,
    pub has_battery: bool,
    pub has_ultrasound: bool,
}

pub const BOARD_CATALOG: &[BoardDescriptor] = &[
    BoardDescriptor {
        slug: "cucumber",
        board_name: "Яндекс Станция Миди (YNDX-00054 / cucumber)",
        gui_label: "cucumber — Станция Миди (24 LED + Часы + 6 LED Лого)",
        sysfs_prefix: "cucumber",
        ring_leds: 24,
        ring_driver: "framebuffer",
        ring_fb_dev: "/dev/fb0",
        logo_leds: 6,
        ambilight_leds: 0,
        has_clock: true,
        has_matrix_21x12: false,
        has_battery: false,
        has_ultrasound: false,
    },
    BoardDescriptor {
        slug: "orion",
        board_name: "Яндекс Станция 3 (YNDX-00055 / orion)",
        gui_label: "orion — Яндекс Станция 3 (36 LED + Ambilight + Матрица 21x12)",
        sysfs_prefix: "orion",
        ring_leds: 36,
        ring_driver: "framebuffer",
        ring_fb_dev: "/dev/fb2",
        logo_leds: 0,
        ambilight_leds: 12,
        has_clock: false,
        has_matrix_21x12: true,
        has_battery: false,
        has_ultrasound: true,
    },
    BoardDescriptor {
        slug: "bergamot",
        board_name: "Яндекс Станция Мини 3 (YNDX-00027 / bergamot)",
        gui_label: "bergamot — Станция Мини 3 (6 LED + Часы)",
        sysfs_prefix: "bergamot",
        ring_leds: 6,
        ring_driver: "framebuffer",
        ring_fb_dev: "/dev/fb0",
        logo_leds: 0,
        ambilight_leds: 0,
        has_clock: true,
        has_matrix_21x12: false,
        has_battery: false,
        has_ultrasound: false,
    },
    BoardDescriptor {
        slug: "yandexmini2",
        board_name: "Яндекс Станция Мини 2 с часами (YNDX-00021 / yandexmini2)",
        gui_label: "yandexmini2 — Станция Мини 2 с часами (6 LED + Часы)",
        sysfs_prefix: "mini2",
        ring_leds: 6,
        ring_driver: "sysfs",
        ring_fb_dev: "/dev/fb0",
        logo_leds: 0,
        ambilight_leds: 0,
        has_clock: true,
        has_matrix_21x12: false,
        has_battery: false,
        has_ultrasound: false,
    },
    BoardDescriptor {
        slug: "yandexmini2nc",
        board_name: "Яндекс Станция Мини 2 без часов (YNDX-00020 / yandexmini2nc)",
        gui_label: "yandexmini2nc — Станция Мини 2 без часов (6 LED)",
        sysfs_prefix: "mini2",
        ring_leds: 6,
        ring_driver: "sysfs",
        ring_fb_dev: "/dev/fb0",
        logo_leds: 0,
        ambilight_leds: 0,
        has_clock: false,
        has_matrix_21x12: false,
        has_battery: false,
        has_ultrasound: false,
    },
    BoardDescriptor {
        slug: "plum",
        board_name: "Яндекс Станция Лайт 2 Plum (3 LED + часы)",
        gui_label: "plum — Станция Лайт 2 с часами (3 LED + Часы)",
        sysfs_prefix: "plum",
        ring_leds: 3,
        ring_driver: "sysfs",
        ring_fb_dev: "/dev/fb0",
        logo_leds: 0,
        ambilight_leds: 0,
        has_clock: true,
        has_matrix_21x12: false,
        has_battery: false,
        has_ultrasound: false,
    },
    BoardDescriptor {
        slug: "plumnc",
        board_name: "Яндекс Станция Лайт 2 Prune (3 LED, без часов)",
        gui_label: "plumnc — Станция Лайт 2 без часов (3 LED)",
        sysfs_prefix: "plum",
        ring_leds: 3,
        ring_driver: "sysfs",
        ring_fb_dev: "/dev/fb0",
        logo_leds: 0,
        ambilight_leds: 0,
        has_clock: false,
        has_matrix_21x12: false,
        has_battery: false,
        has_ultrasound: false,
    },
    BoardDescriptor {
        slug: "station2",
        board_name: "Яндекс Станция 2 (YNDX-00051 / station-midi)",
        gui_label: "station2 — Яндекс Станция 2 (88 LED световая панель)",
        sysfs_prefix: "station2",
        ring_leds: 88,
        ring_driver: "framebuffer",
        ring_fb_dev: "/dev/fb0",
        logo_leds: 0,
        ambilight_leds: 0,
        has_clock: false,
        has_matrix_21x12: false,
        has_battery: false,
        has_ultrasound: false,
    },
    BoardDescriptor {
        slug: "mango",
        board_name: "Яндекс Станция Стрит (mango)",
        gui_label: "mango — Станция Стрит (4 LED + 3 LED Лого + АКБ CW2217 / PD IP2366)",
        sysfs_prefix: "mango",
        ring_leds: 4,
        ring_driver: "framebuffer",
        ring_fb_dev: "/dev/fb0",
        logo_leds: 3,
        ambilight_leds: 0,
        has_clock: false,
        has_matrix_21x12: false,
        has_battery: true,
        has_ultrasound: false,
    },
    BoardDescriptor {
        slug: "yandexmini",
        board_name: "Яндекс Станция Мини 1 (YNDX-0004 / yandexmini)",
        gui_label: "yandexmini — Станция Мини 1 (6 LED SN3236 BGR + ToF)",
        sysfs_prefix: "yandexmini",
        ring_leds: 6,
        ring_driver: "i2c_sn3236",
        ring_fb_dev: "/dev/fb0",
        logo_leds: 0,
        ambilight_leds: 0,
        has_clock: false,
        has_matrix_21x12: false,
        has_battery: false,
        has_ultrasound: false,
    },
    BoardDescriptor {
        slug: "yandexmicro",
        board_name: "Яндекс Станция Лайт 1 (YNDX-00025 / yandexmicro)",
        gui_label: "yandexmicro — Станция Лайт 1 (3 LED sysfs)",
        sysfs_prefix: "micro",
        ring_leds: 3,
        ring_driver: "sysfs",
        ring_fb_dev: "/dev/fb0",
        logo_leds: 0,
        ambilight_leds: 0,
        has_clock: false,
        has_matrix_21x12: false,
        has_battery: false,
        has_ultrasound: false,
    },
    BoardDescriptor {
        slug: "pickle",
        board_name: "Яндекс Станция Мини 3 Про (pickle)",
        gui_label: "pickle — Станция Мини 3 Про (16 LED + Часы + 6 LED Лого)",
        sysfs_prefix: "pickle",
        ring_leds: 16,
        ring_driver: "sysfs",
        ring_fb_dev: "/dev/fb0",
        logo_leds: 6,
        ambilight_leds: 0,
        has_clock: true,
        has_matrix_21x12: false,
        has_battery: false,
        has_ultrasound: false,
    },
    BoardDescriptor {
        slug: "saturn",
        board_name: "Умный хаб Яндекса (YNDX-00050 / saturn)",
        gui_label: "saturn — Яндекс Хаб (1 LED)",
        sysfs_prefix: "saturn",
        ring_leds: 1,
        ring_driver: "sysfs",
        ring_fb_dev: "/dev/fb0",
        logo_leds: 0,
        ambilight_leds: 0,
        has_clock: false,
        has_matrix_21x12: false,
        has_battery: false,
        has_ultrasound: false,
    },
];

pub fn find_board_descriptor(slug: &str) -> Option<&'static BoardDescriptor> {
    let norm = normalize_board_slug(slug);
    BOARD_CATALOG.iter().find(|b| b.slug == norm)
}

pub fn board_display_label(slug: &str) -> String {
    find_board_descriptor(slug)
        .map(|b| b.gui_label.to_string())
        .unwrap_or_else(|| slug.to_string())
}

pub fn default_board_ring_leds(slug: &str) -> usize {
    find_board_descriptor(slug)
        .map(|b| b.ring_leds)
        .unwrap_or(6)
}

pub fn board_has_segment_clock(slug: &str, snapshot_has_clock: bool) -> bool {
    if slug == "yandexmini2nc" || slug == "plumnc" {
        return false;
    }
    snapshot_has_clock
        || find_board_descriptor(slug)
            .map(|b| b.has_clock)
            .unwrap_or(false)
}

pub fn board_has_matrix_21x12(slug: &str, display_kind: &str) -> bool {
    display_kind == "matrix_21x12"
        || find_board_descriptor(slug)
            .map(|b| b.has_matrix_21x12)
            .unwrap_or(false)
}

pub fn normalize_board_slug(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "station-lite" | "lite" | "lite1" | "micro" | "yandexmicro" => "yandexmicro".to_string(),
        "mini" | "mini1" | "yandexmini" => "yandexmini".to_string(),
        "mini2" | "yandexmini_2" | "yandexmini2" => "yandexmini2".to_string(),
        "mini2nc" | "yandexmini_2_nc" | "yandexmini2nc" => "yandexmini2nc".to_string(),
        "mini3" | "yandexmini_3" | "yandexmini3" | "bergamot" => "bergamot".to_string(),
        "mini3pro" | "pickle" => "pickle".to_string(),
        "lite2" | "plum" => "plum".to_string(),
        "prune" | "lite2nc" | "plumnc" => "plumnc".to_string(),
        "midi" | "cucumber" => "cucumber".to_string(),
        "station3" | "orion" => "orion".to_string(),
        "station_2" | "station-midi" | "yandexmidi" | "station2" | "yandexstation_2" => {
            "station2".to_string()
        }
        "street" | "mango" => "mango".to_string(),
        "hub" | "saturn" => "saturn".to_string(),
        other => other.to_string(),
    }
}

/// Определяет канонический `board_slug`, отдавая приоритет аппаратным признакам Device Tree и SoC
/// (`/proc/device-tree/compatible`, `/proc/device-tree/amlogic-dt-id`, `ro.product.device`),
/// а `deviceType` из `quasar.cfg` использует только как вторичный признак (т.к. у Станции 2 на
/// Amlogic AXG в `quasar.cfg` записано внутреннее `deviceType: "yandexmidi"`).
pub fn detect_board_slug_from_probe(dev_type_cfg: &str, model_raw: &str) -> String {
    let model_lc = model_raw.to_ascii_lowercase();
    let model_compact: String = model_lc.chars().filter(|c| !c.is_whitespace()).collect();
    if model_lc.contains("prune") {
        return "plumnc".to_string();
    }

    // 1. Однозначные признаки Device Tree и SoC (как в yam/install.sh)
    if model_lc.contains("cucumber") {
        return "cucumber".to_string();
    }
    if model_lc.contains("station-midi") && model_compact.contains("amlogic,axg") {
        return "station2".to_string();
    }
    if model_lc.contains("orion") {
        return "orion".to_string();
    }
    if model_lc.contains("bergamot")
        || model_lc.contains("yandexmini_3")
        || model_lc.contains("yandexmini3")
    {
        return "bergamot".to_string();
    }
    if model_lc.contains("pickle") {
        return "pickle".to_string();
    }
    if model_lc.contains("mango") || model_lc.contains("street") {
        return "mango".to_string();
    }
    if model_lc.contains("saturn") {
        return "saturn".to_string();
    }

    // 2. Уточнение по dev_type_cfg (с защитой от коллизии yandexmidi vs cucumber на Amlogic A5)
    let dev = dev_type_cfg.trim();
    if !dev.is_empty() && !dev.eq_ignore_ascii_case("unknown") {
        if (dev.eq_ignore_ascii_case("yandexmidi") || dev.eq_ignore_ascii_case("midi"))
            && model_compact.contains("amlogic,a5")
        {
            return "cucumber".to_string();
        }
        return normalize_board_slug(dev);
    }

    // 3. Остальные признаки из строки DT/модели
    if model_lc.contains("mini3") {
        "bergamot".to_string()
    } else if model_lc.contains("plum") {
        "plum".to_string()
    } else if model_lc.contains("station-midi") {
        "station2".to_string()
    } else if model_lc.contains("yandexmini2nc") || model_lc.contains("mini2nc") {
        "yandexmini2nc".to_string()
    } else if model_lc.contains("yandexmini_2")
        || model_lc.contains("station-mini2")
        || model_lc.contains("mini2")
    {
        "yandexmini2".to_string()
    } else if model_lc.contains("yandexmicro") || model_lc.contains("station-lite") {
        "yandexmicro".to_string()
    } else if model_lc.contains("axg") {
        "yandexmini".to_string()
    } else {
        "unknown".to_string()
    }
}

pub fn detect_board_hint_from_filename(name: &str) -> Option<String> {
    let mut lower = name.trim().to_ascii_lowercase();
    if lower.is_empty() || lower == "backups" || lower == "work" {
        return None;
    }

    for suffix in [
        "-system.tar.gz",
        "-system.tgz",
        "-system.tar",
        "-system.ext4",
        "-system-orig.ext4",
        "-rootfs.tar.gz",
        "-rootfs.tgz",
        "-backup.zip",
        ".tar.gz",
        ".tgz",
        ".tar",
        ".zip",
        ".ext4",
        ".img.gz",
        ".img",
        ".bin",
    ] {
        if let Some(prefix) = lower.strip_suffix(suffix) {
            if !prefix.is_empty() {
                lower = prefix.to_string();
                break;
            }
        }
    }

    if let Some(rest) = lower.strip_prefix("dump-") {
        if !rest.is_empty() {
            lower = rest.to_string();
        }
    }

    let check_candidate = |raw: &str| -> Option<String> {
        let norm = normalize_board_slug(raw);
        if find_board_descriptor(&norm).is_some() {
            Some(norm)
        } else {
            None
        }
    };

    if let Some(slug) = check_candidate(&lower) {
        return Some(slug);
    }

    let tokens: Vec<&str> = lower
        .split(['-', '_'])
        .filter(|s| !s.is_empty())
        .collect();
    if tokens.len() >= 2 {
        let two_hyphen = format!("{}-{}", tokens[0], tokens[1]);
        if let Some(slug) = check_candidate(&two_hyphen) {
            return Some(slug);
        }
        let two_under = format!("{}_{}", tokens[0], tokens[1]);
        if let Some(slug) = check_candidate(&two_under) {
            return Some(slug);
        }
        if tokens[0] == "yandex" {
            if let Some(slug) = check_candidate(tokens[1]) {
                return Some(slug);
            }
        }
    }
    if let Some(&first) = tokens.first() {
        if let Some(slug) = check_candidate(first) {
            return Some(slug);
        }
    }
    for &tok in &tokens {
        if let Some(slug) = check_candidate(tok) {
            return Some(slug);
        }
    }

    None
}

pub fn detect_board_hint_from_path(path: &Path) -> Option<String> {
    for ancestor in path.ancestors() {
        let Some(fname) = ancestor.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if fname.eq_ignore_ascii_case("backups") || fname.eq_ignore_ascii_case("work") {
            break;
        }
        if let Some(slug) = detect_board_hint_from_filename(fname) {
            return Some(slug);
        }
    }
    None
}
