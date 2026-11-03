use super::{gnome_action_row, gnome_preferences_group};
use crate::gui::state::GuiAppState;
use eframe::egui::{self, Color32, RichText};
use protocol::HostCommand;

pub fn render_board_specific_card(ui: &mut egui::Ui, app: &mut GuiAppState) {
    let board_slug = app
        .latest_state
        .as_ref()
        .map(|s| s.board_slug.clone())
        .unwrap_or_else(|| app.config.selected_board.clone());
    let desc = protocol::find_board_descriptor(&board_slug);
    let has_ultrasound = desc
        .map(|d| d.has_ultrasound)
        .unwrap_or(board_slug == "orion");
    let is_sn3236 = desc
        .map(|d| d.ring_driver == "i2c_sn3236")
        .unwrap_or(board_slug == "yandexmini");
    let has_battery = desc
        .map(|d| d.has_battery)
        .unwrap_or(board_slug == "mango");

    let title = format!("3. Специфичная периферия платы ({})", board_slug);
    gnome_preferences_group(
        ui,
        &title,
        "Аппаратные контроллеры, усилители и драйверы индикации выбранного профиля платы",
        |ui| {
            if has_ultrasound {
                render_orion_card_rows(ui, app);
            } else if is_sn3236 {
                render_sn3236_card_rows(ui, app);
            } else if has_battery {
                render_mango_card_rows(ui, app);
            } else {
                render_generic_sysfs_ring_rows(ui, app);
            }
        },
    );
}

fn render_orion_card_rows(ui: &mut egui::Ui, app: &mut GuiAppState) {
    let st_opt = app.latest_state.clone();
    gnome_action_row(
        ui,
        "Контур Ambilight 12x2 RGBW (/dev/fb0 + 96 каналов sysfs)",
        "Фоновая подсветка корпуса Яндекс Станции 3 (orion)",
        false,
        |ui| {
            for (lbl, pat) in [
                ("Фиолетовый (speaking)", "speaking"),
                ("Бирюзовый (rgb:#00E5FF)", "rgb:#00E5FF"),
                ("Теплый (rgb:#FF9933)", "rgb:#FF9933"),
            ] {
                if ui.button(lbl).clicked() {
                    app.selected_led_pattern = pat.to_string();
                    app.send_command(HostCommand::SetLedPattern {
                        pattern: pat.to_string(),
                    });
                }
            }
        },
    );

    if let Some(ref st) = st_opt {
        if !st.amplifiers.is_empty() {
            ui.label(
                RichText::new("4x Усилителя TI ACM8625S (I2C) + Тракт pcm.ultrasound (96 кГц / 38.4 кГц)")
                    .strong(),
            );
            egui::Grid::new("gnome_orion_amp_grid")
                .striped(true)
                .num_columns(6)
                .spacing([12.0, 5.0])
                .show(ui, |ui| {
                    for hdr in ["I2C", "Роль", "ALSA", "Профиль DSP", "Усиление", "Статус"] {
                        ui.label(RichText::new(hdr).small().strong());
                    }
                    ui.end_row();
                    for amp in &st.amplifiers {
                        ui.monospace(&amp.i2c_device);
                        ui.label(RichText::new(&amp.role).strong());
                        ui.monospace(&amp.alsa_prefix);
                        ui.monospace(&amp.settings_file);
                        ui.monospace(format!("{:+.1} / {:+.1} dB", amp.gain_db, amp.ramp_db));
                        let (txt, col) = if amp.muted {
                            ("MUTED", Color32::from_rgb(245, 95, 95))
                        } else {
                            ("ACTIVE", Color32::from_rgb(87, 227, 137))
                        };
                        ui.label(RichText::new(txt).small().strong().color(col));
                        ui.end_row();
                    }
                });
            if let Some(ref us) = st.ultrasound {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "Ultrasound TDM-A: {} | {} Гц | Несущая: {} Гц | Полоса: {} Гц | Каналов: {} | RMS: {}",
                        us.tdm_device,
                        us.sample_rate_hz,
                        us.carrier_freq_hz,
                        us.bandwidth_hz,
                        us.channels,
                        us.rms
                    ))
                    .small()
                    .monospace(),
                );
            }
            return;
        }
    }
    gnome_action_row(
        ui,
        "4x Усилителя TI ACM8625S (I2C) + Ультразвук TDM-A",
        "3-002d (woofer), 3-002e (lr), 3-002c (center), 2-002c (us) | pcm.ultrasound 96 кГц / 38.4 кГц",
        true,
        |ui| {
            ui.label(
                RichText::new("4x ACM8625S + Ultrasound 38.4 кГц")
                    .small()
                    .strong()
                    .color(Color32::from_rgb(87, 227, 137)),
            );
        },
    );
}

fn render_sn3236_card_rows(ui: &mut egui::Ui, app: &mut GuiAppState) {
    let trig = app
        .latest_state
        .as_ref()
        .map(|s| s.ring_trigger.clone())
        .unwrap_or_else(|| "i2c-0 SN3236 (0x3c Active)".to_string());
    gnome_action_row(
        ui,
        "I2C Контроллер SN3236 (адрес 0x3c, 18 каналов BGR)",
        &format!(
            "Запись в 0x01..0x12 меняет только теневой буфер; перенос в активное свечение происходит только при записи в защёлку 0x25 (Register Latch), 0x00=0 включает Stealth Lock | Статус: {}",
            trig
        ),
        true,
        |ui| {
            if ui.button("Проверка BGR (rgb:#6B0FFF)").clicked() {
                app.selected_led_pattern = "rgb:#6B0FFF".to_string();
                app.send_command(HostCommand::SetLedPattern {
                    pattern: "rgb:#6B0FFF".to_string(),
                });
            }
            if ui.button("Авто (Quasar)").clicked() {
                app.selected_led_pattern.clear();
                app.send_command(HostCommand::SetLedPattern {
                    pattern: String::new(),
                });
            }
            if app.session.is_some() {
                if ui
                    .button("Демо защёлки 0x25 (I2C)")
                    .on_hover_text("Записать BGR в теневые регистры 0x01..0x12, выждать 1 с (кольцо ещё не меняется) и защёлкнуть регистром 0x25")
                    .clicked()
                {
                    app.send_serial_command(
                        "i2cset -y 0 0x3c 0x00 0x01; for r in $(seq 1 3 16); do i2cset -y 0 0x3c $r 0xff; i2cset -y 0 0x3c $((r+1)) 0x40; i2cset -y 0 0x3c $((r+2)) 0x90; done; sleep 1; i2cset -y 0 0x3c 0x25 0x01",
                    );
                }
                if ui
                    .button("Stealth Lock (0x00=0)")
                    .on_hover_text("Программно погасить и заблокировать чип SN3236 записью 0x00 в регистр 0x00")
                    .clicked()
                {
                    app.send_serial_command("i2cset -y 0 0x3c 0x00 0x00");
                }
            }
        },
    );
}

fn render_mango_card_rows(ui: &mut egui::Ui, app: &mut GuiAppState) {
    let vm_on = app.session.is_some();
    gnome_action_row(
        ui,
        "Кулонометр АКБ CW2217 (/sys/devices/virtual/cw2217_cls/cw2217_coulomb/*) и /sys/class/power_supply/battery/*",
        "Ёмкость capacity (%), напряжение battery_vol (мВ), ток battery_current (мА), температура battery_temp (0.1 °C), soh и cycle",
        false,
        |ui| {
            for (lbl, cap, vol_mv, status) in [
                ("Заряд 100%", 100, 8350, "Full"),
                ("Заряд 85%", 85, 7800, "Charging"),
                ("Заряд 25%", 25, 7100, "Discharging"),
                ("Низкий 5%", 5, 6600, "Discharging"),
            ] {
                if ui
                    .add_enabled(vm_on, egui::Button::new(lbl))
                    .on_hover_text(format!(
                        "Записать capacity={}, battery_vol={} мВ в cw2217_coulomb и /sys/class/power_supply/battery",
                        cap, vol_mv
                    ))
                    .clicked()
                {
                    app.send_serial_command(&format!(
                        "echo {cap} > /sys/devices/virtual/cw2217_cls/cw2217_coulomb/capacity; \
                         echo {vol} > /sys/devices/virtual/cw2217_cls/cw2217_coulomb/battery_vol; \
                         echo {cap} > /sys/class/power_supply/battery/capacity; \
                         echo $(( {vol} * 1000 )) > /sys/class/power_supply/battery/voltage_now; \
                         echo {st} > /sys/class/power_supply/battery/status",
                        cap = cap,
                        vol = vol_mv,
                        st = status
                    ));
                }
            }
            if ui
                .add_enabled(vm_on, egui::Button::new("Прочитать CW2217 + battery"))
                .on_hover_text("Вывести все атрибуты cw2217_coulomb и /sys/class/power_supply/battery в консоль ttyAMA0")
                .clicked()
            {
                app.config.active_terminal_tab = "serial".to_string();
                app.config.dock_collapsed = false;
                app.send_serial_command(
                    "head -n 1 /sys/devices/virtual/cw2217_cls/cw2217_coulomb/* /sys/class/power_supply/battery/*",
                );
            }
        },
    );

    gnome_action_row(
        ui,
        "Контроллер USB-C PD IP2366 (/sys/devices/virtual/ip2366_cls/ip2366_charger/*), усилитель ACM8625S (2-002c) и pcba_ids",
        "Режимы зарядки (is_pd, charge_status, charge_voltage, pd_protocol), DSP-профиль (помещение / улица) и ревизия плат",
        false,
        |ui| {
            if ui
                .add_enabled(vm_on, egui::Button::new("PD 12 В (Fast Charge)"))
                .on_hover_text("Включить быструю зарядку USB-C PD: is_pd=1, charge_status=1, charge_voltage=12000, pd_protocol=PD")
                .clicked()
            {
                app.send_serial_command(
                    "echo 1 > /sys/devices/virtual/ip2366_cls/ip2366_charger/charge_status; \
                     echo 1 > /sys/devices/virtual/ip2366_cls/ip2366_charger/is_pd; \
                     echo 12000 > /sys/devices/virtual/ip2366_cls/ip2366_charger/charge_voltage; \
                     echo PD > /sys/devices/virtual/ip2366_cls/ip2366_charger/pd_protocol; \
                     echo Charging > /sys/class/power_supply/battery/status",
                );
            }
            if ui
                .add_enabled(vm_on, egui::Button::new("От АКБ (Без кабеля)"))
                .on_hover_text("Отключить зарядку: charge_status=0, is_pd=0, charge_voltage=0, status=Discharging")
                .clicked()
            {
                app.send_serial_command(
                    "echo 0 > /sys/devices/virtual/ip2366_cls/ip2366_charger/charge_status; \
                     echo 0 > /sys/devices/virtual/ip2366_cls/ip2366_charger/is_pd; \
                     echo 0 > /sys/devices/virtual/ip2366_cls/ip2366_charger/charge_voltage; \
                     echo NONE > /sys/devices/virtual/ip2366_cls/ip2366_charger/pd_protocol; \
                     echo Discharging > /sys/class/power_supply/battery/status",
                );
            }
            if ui
                .add_enabled(vm_on, egui::Button::new("DSP: В помещении"))
                .on_hover_text("Записать профиль mango-acm8625.cfg в /tmp/amp_settings_file (/sys/bus/i2c/devices/2-002c/settings_file)")
                .clicked()
            {
                app.send_serial_command("echo mango-acm8625.cfg > /tmp/amp_settings_file");
            }
            if ui
                .add_enabled(vm_on, egui::Button::new("DSP: На улице (Street)"))
                .on_hover_text("Записать уличный профиль mango-acm8625-outside.cfg в /tmp/amp_settings_file")
                .clicked()
            {
                app.send_serial_command("echo mango-acm8625-outside.cfg > /tmp/amp_settings_file");
            }
            if ui
                .add_enabled(vm_on, egui::Button::new("Прочитать IP2366 + ACM8625S + pcba_ids"))
                .on_hover_text("Вывести состояние зарядки IP2366, активный профиль усилителя 2-002c и JSON pcba_ids в консоль ttyAMA0")
                .clicked()
            {
                app.config.active_terminal_tab = "serial".to_string();
                app.config.dock_collapsed = false;
                app.send_serial_command(
                    "head -n 1 /sys/devices/virtual/ip2366_cls/ip2366_charger/* /sys/bus/i2c/devices/2-002c/name /sys/bus/i2c/devices/2-002c/settings_file /sys/devices/platform/pcba_ids/pcba_ids_drv/pcba_ids",
                );
            }
        },
    );

    let (trig, conflicts) = app
        .latest_state
        .as_ref()
        .map(|s| (s.ring_trigger.clone(), s.led_trigger_conflict_count))
        .unwrap_or_else(|| ("ledfb-ring (Quasar Native)".to_string(), 0));
    gnome_action_row(
        ui,
        "4-LED шкала заряда (/dev/fb0 + mango:*:ring1..4) и 3-LED логотип oknix (/dev/fb1 + mango:*:logo0..2)",
        &format!(
            "Активный триггер: {} | Конфликтов триггера: {}",
            trig, conflicts
        ),
        true,
        |ui| {
            if ui.button("Алиса Про (#00FF80 + #430FFF)").clicked() {
                if vm_on {
                    app.send_serial_command(
                        "printf '\\x00\\xff\\x80\\x43\\x0f\\xff\\x43\\x0f\\xff' > /dev/fb1",
                    );
                }
                app.selected_led_pattern = "listening".to_string();
                app.send_command(HostCommand::SetLedPattern {
                    pattern: "listening".to_string(),
                });
            }
            if ui.button("Шкала заряда 75% (3/4 LED)").clicked() && vm_on {
                app.send_serial_command(
                    "for i in 1 2 3 4; do echo none > /sys/class/leds/mango:red:ring$i/trigger; done; \
                     for i in 1 2 3; do echo 255 > /sys/class/leds/mango:red:ring$i/brightness; echo 255 > /sys/class/leds/mango:green:ring$i/brightness; echo 255 > /sys/class/leds/mango:blue:ring$i/brightness; done; \
                     echo 0 > /sys/class/leds/mango:red:ring4/brightness; echo 0 > /sys/class/leds/mango:green:ring4/brightness; echo 0 > /sys/class/leds/mango:blue:ring4/brightness",
                );
            }
            if ui.button("Авто (ledfb-ring)").clicked() {
                app.selected_led_pattern.clear();
                app.send_command(HostCommand::SetLedPattern {
                    pattern: String::new(),
                });
                if vm_on {
                    app.send_serial_command(
                        "for d in /sys/class/leds/mango:*:ring*; do echo 0 > \"$d/brightness\"; echo ledfb-ring > \"$d/trigger\"; done",
                    );
                }
            }
        },
    );
}

fn render_generic_sysfs_ring_rows(ui: &mut egui::Ui, app: &mut GuiAppState) {
    let (trig, conflicts) = app
        .latest_state
        .as_ref()
        .map(|s| (s.ring_trigger.clone(), s.led_trigger_conflict_count))
        .unwrap_or_else(|| ("ledfb-ring (Quasar Native)".to_string(), 0));
    gnome_action_row(
        ui,
        "Драйвер светового кольца (sysfs / ledfb-ring)",
        &format!(
            "Активный триггер: {} | Отклонено записей sysfs при [ledfb-ring] (Trigger Conflict): {}",
            trig, conflicts
        ),
        true,
        |ui| {
            if ui.button("Бирюзовый (#00E5FF)").clicked() {
                app.selected_led_pattern = "rgb:#00E5FF".to_string();
                app.send_command(HostCommand::SetLedPattern {
                    pattern: "rgb:#00E5FF".to_string(),
                });
            }
            if ui.button("Авто (ledfb-ring)").clicked() {
                app.selected_led_pattern.clear();
                app.send_command(HostCommand::SetLedPattern {
                    pattern: String::new(),
                });
            }
            if app.session.is_some() {
                if ui
                    .button("Ручной sysfs (trigger=none)")
                    .on_hover_text("Переключить триггер кольца в 'none' для прямой записи в /sys/class/leds/*:ring*/brightness без конфликта")
                    .clicked()
                {
                    app.send_serial_command(
                        "for d in /sys/class/leds/*:ring*; do echo none > \"$d/trigger\"; done",
                    );
                }
                if ui
                    .button("Вернуть триггер ledfb-ring")
                    .on_hover_text("Обнулить brightness в sysfs и вернуть управление кольцом штатному триггеру драйвера ledfb-ring")
                    .clicked()
                {
                    app.send_serial_command(
                        "for d in /sys/class/leds/*:ring*; do echo 0 > \"$d/brightness\"; echo ledfb-ring > \"$d/trigger\"; done",
                    );
                }
            }
        },
    );
}
