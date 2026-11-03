use crate::gui::state::{GuiAppState, SignalDirection, SignalFilter};
use crate::session::strip_ansi_and_control_chars;
use eframe::egui::{self, Color32, FontId, Margin, RichText, Stroke};

const SERIAL_SNIPPETS: &[(&str, &str)] = &[
    (
        "Профиль железа (/tmp/quasar-emu-hw.json)",
        "cat /tmp/quasar-emu-hw.json",
    ),
    ("Список процессов (ps)", "ps"),
    ("Сетевые сокеты (netstat -tlpn)", "netstat -tlpn"),
    (
        "Фреймбуферы (/sys/class/graphics/fb*/name)",
        "head -n 1 /sys/class/graphics/fb*/name",
    ),
    (
        "LED: Бегущая радуга по кольцу (sysfs)",
        "cd /sys/class/leds;for d in *:ring*;do echo none>$d/trigger;P=${d%%:*};done;for f in $(seq 1 24);do for i in $(seq 0 36);do for c in red green blue;do f=$((f+2));echo $((((i+f)%6<3)*250))>$P:$c:ring$i/brightness;done;done 2>/dev/null;usleep 35000;done",
    ),
    (
        "LED: Ручной режим sysfs (trigger=none + Красный)",
        "for d in /sys/class/leds/*:ring*; do echo none > \"$d/trigger\"; done; for d in /sys/class/leds/*:red:ring*; do echo 220 > \"$d/brightness\"; done",
    ),
    (
        "LED: Возврат триггера Quasar (ledfb-ring)",
        "for d in /sys/class/leds/*:ring*; do echo 0 > \"$d/brightness\"; echo ledfb-ring > \"$d/trigger\"; done",
    ),
    (
        "LED: Демо конфликта триггера (запись при [ledfb-ring])",
        "for d in /sys/class/leds/*:red:ring*; do echo ledfb-ring > \"$d/trigger\"; echo 200 > \"$d/brightness\"; break; done",
    ),
    (
        "Часы: Перебор всех 45 сегментов (cucumber / mini2 / bergamot / plum)",
        "for d in /sys/class/leds/*:white:clock-*; do echo 0 > \"$d/brightness\"; done; for d in /sys/class/leds/*:white:clock-*; do echo 255 > \"$d/brightness\"; usleep 35000; done",
    ),
    (
        "Orion: Бегущая волна на матрице 21x12 (/dev/fb1)",
        "for f in $(seq 0 35);do awk -v f=$f 'BEGIN{for(y=0;y<12;y++)for(x=0;x<21;x++){k=(x+f)%10;h=(k<5)?3+k:13-k;v=(y==h||y==h+1)?255:(y>h?60:0);printf \"%c%c%c\",v,v,v}}' >/dev/fb1;usleep 45000;done",
    ),
    (
        "Orion: Перелив контура Ambilight (12x2 RGBW)",
        "cd /sys/class/leds;for f in $(seq 1 24);do for i in $(seq 0 11);do echo $((((i+f)%3==0)*250))>orion:red:ambilight${i}a/brightness;echo $((((i+f)%3==1)*250))>orion:green:ambilight${i}a/brightness;echo $((((i+f)%3==2)*250))>orion:blue:ambilight${i}b/brightness;done;usleep 50000;done",
    ),
    (
        "Mango (Стрит): Кулонометр CW2217 + зарядка IP2366 PD + battery",
        "head -n 1 /sys/devices/virtual/cw2217_cls/cw2217_coulomb/* /sys/devices/virtual/ip2366_cls/ip2366_charger/* /sys/class/power_supply/battery/*",
    ),
    (
        "Mango (Стрит): Усилитель ACM8625S (2-002c) и ревизии pcba_ids",
        "head -n 1 /sys/bus/i2c/devices/2-002c/name /sys/bus/i2c/devices/2-002c/settings_file /tmp/amp_settings_file /sys/devices/platform/pcba_ids/pcba_ids_drv/pcba_ids",
    ),
    (
        "Mango / Cucumber: Логотип (/dev/fb1) — тест цветов Алисы",
        "printf '\\x00\\xff\\x80\\x6b\\x0f\\xff\\x22\\xcc\\xff\\x00\\xff\\x80\\x6b\\x0f\\xff\\x22\\xcc\\xff' > /dev/fb1",
    ),
    (
        "Mini 1 (SN3236): Теневые регистры I2C 0x3c -> защёлка 0x25",
        "i2cset -y 0 0x3c 0x00 0x01; for r in $(seq 1 3 16); do i2cset -y 0 0x3c $r 0xff; i2cset -y 0 0x3c $((r+1)) 0x40; i2cset -y 0 0x3c $((r+2)) 0x90; done; sleep 1; i2cset -y 0 0x3c 0x25 0x01",
    ),
    (
        "Аудио: Вывод звука через ALSA (aplay -> GUI)",
        "dd if=/dev/urandom bs=4800 count=20 2>/dev/null | aplay -D default -q -t raw -f S16_LE -r 48000 -c 2 -",
    ),
];

const TTY_TERM_FOCUS_ID: &str = "tty_ama0_native_terminal";

pub fn render_bottom_dock(ui: &mut egui::Ui, app: &mut GuiAppState) {
    ui.horizontal_wrapped(|ui| {
        let collapse_label = if app.config.dock_collapsed {
            "Развернуть панель логов [+]"
        } else {
            "Свернуть панель логов [-]"
        };
        if ui
            .button(RichText::new(collapse_label).strong())
            .on_hover_text("Свернуть или развернуть нижнюю панель. Высоту открытой панели можно плавно менять перетаскиванием её верхней границы с зажатой ЛКМ.")
            .clicked()
        {
            app.config.dock_collapsed = !app.config.dock_collapsed;
            let _ = app.save_config();
        }

        ui.separator();

        for (tab_id, label) in [
            ("signals", "Лог сигналов (TX / RX)"),
            ("serial", "Интерактивный терминал ttyAMA0"),
            ("maind", "Лог демона maind.log"),
        ] {
            let active = app.config.active_terminal_tab == tab_id && !app.config.dock_collapsed;
            if ui
                .selectable_label(active, RichText::new(label).strong())
                .clicked()
            {
                app.config.active_terminal_tab = tab_id.to_string();
                app.config.dock_collapsed = false;
                if tab_id == "serial" {
                    let term_id = egui::Id::new(TTY_TERM_FOCUS_ID);
                    ui.ctx().memory_mut(|m| m.request_focus(term_id));
                }
                let _ = app.save_config();
            }
        }

        ui.separator();
        let ipc_errs = app
            .latest_state
            .as_ref()
            .map(|s| s.ipc_error_count)
            .unwrap_or(0);
        let pill_col = if ipc_errs == 0 {
            Color32::from_rgb(155, 205, 255)
        } else {
            Color32::from_rgb(255, 115, 115)
        };
        ui.add(
            egui::Button::new(
                RichText::new(format!(
                    "Событий: {} | Ошибок IPC: {}",
                    app.signal_logs.len(),
                    ipc_errs
                ))
                .small()
                .strong()
                .color(pill_col),
            )
            .fill(Color32::from_rgb(38, 50, 68))
            .rounding(5.0)
            .sense(egui::Sense::hover()),
        );

        if !app.config.dock_collapsed {
            ui.separator();
            if app.config.active_terminal_tab == "signals" {
                for (flt, title) in [
                    (SignalFilter::All, "Все"),
                    (SignalFilter::TofButtons, "ToF / Кнопки"),
                    (SignalFilter::TtsIpc, "TTS / IPC"),
                    (SignalFilter::LedDisplay, "LED / Экран"),
                    (SignalFilter::Audio, "Аудио"),
                ] {
                    if ui
                        .selectable_label(app.signal_filter == flt, title)
                        .clicked()
                    {
                        app.signal_filter = flt;
                    }
                }
                if ui.button("Очистить").clicked() {
                    app.clear_signals();
                }
            } else if app.config.active_terminal_tab == "serial" {
                let vm_running = app.session.is_some();
                ui.menu_button("Быстрые команды ▾", |ui| {
                    ui.set_min_width(320.0);
                    ui.label(
                        RichText::new("Готовые диагностические сниппеты для ttyAMA0:")
                            .small()
                            .color(Color32::from_rgb(160, 170, 185)),
                    );
                    ui.separator();
                    for &(label, cmd) in SERIAL_SNIPPETS {
                        if ui
                            .add_enabled(vm_running, egui::Button::new(label))
                            .on_hover_text(cmd)
                            .clicked()
                        {
                            app.send_serial_command(cmd);
                            let term_id = egui::Id::new(TTY_TERM_FOCUS_ID);
                            ui.ctx().memory_mut(|m| m.request_focus(term_id));
                            ui.close_menu();
                        }
                    }
                });
                if ui
                    .add_enabled(vm_running, egui::Button::new("Ctrl+C"))
                    .on_hover_text("Отправить сигнал прерывания SIGINT (0x03) в консоль ttyAMA0")
                    .clicked()
                {
                    app.send_serial_command("\x03");
                    let term_id = egui::Id::new(TTY_TERM_FOCUS_ID);
                    ui.ctx().memory_mut(|m| m.request_focus(term_id));
                }
                if ui
                    .add_enabled(vm_running, egui::Button::new("Ctrl+D"))
                    .on_hover_text("Отправить сигнал конца ввода EOF (0x04) в консоль ttyAMA0")
                    .clicked()
                {
                    app.send_serial_command("\x04");
                    let term_id = egui::Id::new(TTY_TERM_FOCUS_ID);
                    ui.ctx().memory_mut(|m| m.request_focus(term_id));
                }
                if ui
                    .button("Копировать")
                    .on_hover_text("Скопировать весь вывод консоли ttyAMA0 в буфер обмена")
                    .clicked()
                {
                    if let Some(ref sess) = app.session {
                        let joined = sess.serial_lines().join("\n");
                        ui.ctx().copy_text(joined);
                    }
                    let term_id = egui::Id::new(TTY_TERM_FOCUS_ID);
                    ui.ctx().memory_mut(|m| m.request_focus(term_id));
                }
                if ui
                    .button("Очистить")
                    .on_hover_text("Очистить вывод терминала (горячая клавиша: Ctrl+L)")
                    .clicked()
                {
                    if let Some(ref sess) = app.session {
                        sess.clear_serial_lines();
                    }
                    let term_id = egui::Id::new(TTY_TERM_FOCUS_ID);
                    ui.ctx().memory_mut(|m| m.request_focus(term_id));
                }
                ui.checkbox(&mut app.terminal_autoscroll, "Автопрокрутка");
            } else if app.config.active_terminal_tab == "maind" {
                ui.checkbox(&mut app.terminal_autoscroll, "Автопрокрутка");
            }
        }
    });

    if app.config.dock_collapsed {
        return;
    }

    ui.separator();

    match app.config.active_terminal_tab.as_str() {
        "serial" => render_serial_console_tab(ui, app),
        "maind" => render_maind_log_tab(ui, app),
        _ => render_signal_log_tab(ui, app),
    }
}

fn render_signal_log_tab(ui: &mut egui::Ui, app: &mut GuiAppState) {
    let entries = app.filtered_signals();
    let avail_h = ui.available_height().max(60.0);

    egui::Frame::canvas(ui.style())
        .fill(Color32::from_rgb(18, 19, 22))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("signal_monitor_scroll")
                .auto_shrink([false, false])
                .max_height(avail_h)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    if entries.is_empty() {
                        ui.label(
                            RichText::new("Нет записей в логе сигналов для выбранного фильтра.")
                                .font(FontId::monospace(13.5))
                                .color(Color32::from_rgb(145, 155, 170)),
                        );
                    } else {
                        for entry in &entries {
                            let color = match entry.direction {
                                SignalDirection::Tx => Color32::from_rgb(120, 180, 245),
                                SignalDirection::Rx => Color32::from_rgb(87, 227, 137),
                            };
                            let line = format!(
                                "[{}] [{} {}] {}",
                                entry.timestamp,
                                entry.direction.as_str(),
                                entry.category.as_str(),
                                strip_ansi_and_control_chars(&entry.message)
                            );
                            ui.add(
                                egui::Label::new(
                                    RichText::new(line)
                                        .font(FontId::monospace(13.2))
                                        .color(color),
                                )
                                .selectable(true),
                            );
                        }
                    }
                });
        });
}

fn classify_serial_line_color(clean: &str) -> Color32 {
    if clean.starts_with("===") {
        Color32::from_rgb(87, 227, 137)
    } else if clean.starts_with("~ #") || clean.starts_with("/ #") || clean.starts_with("root@") {
        Color32::from_rgb(130, 200, 255)
    } else if clean.contains("error")
        || clean.contains("Error")
        || clean.contains("FAIL")
        || clean.contains("panic")
    {
        Color32::from_rgb(255, 120, 120)
    } else if clean.starts_with("[qemu]") || clean.contains("can't access tty") {
        Color32::from_rgb(165, 175, 190)
    } else {
        Color32::from_rgb(222, 230, 240)
    }
}

fn build_active_line_job(active_line: &str, cursor_col: usize, has_focus: bool) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let font_id = FontId::monospace(13.5);
    let chars: Vec<char> = active_line.chars().collect();
    let col = cursor_col.min(chars.len());
    let before: String = chars[..col].iter().collect();
    let cursor_ch: String = chars
        .get(col)
        .copied()
        .unwrap_or(' ')
        .to_string();
    let after: String = if col + 1 < chars.len() {
        chars[col + 1..].iter().collect()
    } else {
        String::new()
    };

    let text_fmt = |color: Color32| egui::TextFormat {
        font_id: font_id.clone(),
        color,
        ..Default::default()
    };

    if let Some(hash_idx) = before.find("# ") {
        let split_at = hash_idx + 2;
        job.append(
            &before[..split_at],
            0.0,
            text_fmt(Color32::from_rgb(87, 227, 137)),
        );
        if split_at < before.len() {
            job.append(
                &before[split_at..],
                0.0,
                text_fmt(Color32::from_rgb(242, 247, 255)),
            );
        }
    } else {
        job.append(&before, 0.0, text_fmt(Color32::from_rgb(130, 200, 255)));
    }

    let cursor_fmt = egui::TextFormat {
        font_id: font_id.clone(),
        color: if has_focus {
            Color32::from_rgb(14, 16, 20)
        } else {
            Color32::from_rgb(230, 236, 245)
        },
        background: if has_focus {
            Color32::from_rgb(87, 227, 137)
        } else {
            Color32::from_rgb(65, 75, 92)
        },
        ..Default::default()
    };
    job.append(&cursor_ch, 0.0, cursor_fmt);

    if !after.is_empty() {
        job.append(&after, 0.0, text_fmt(Color32::from_rgb(242, 247, 255)));
    }

    job
}

fn collect_terminal_key_events(ctx: &egui::Context, term_id: egui::Id) -> (String, bool) {
    ctx.memory_mut(|m| {
        m.set_focus_lock_filter(
            term_id,
            egui::EventFilter {
                tab: true,
                horizontal_arrows: true,
                vertical_arrows: true,
                escape: true,
            },
        );
    });

    let mut tx = String::new();
    let mut clear_scrollback = false;

    ctx.input_mut(|i| {
        for ev in &i.events {
            match ev {
                egui::Event::Copy => {
                    // `egui-winit` translates Ctrl+C on Windows/Linux (and Cmd+C on macOS) into
                    // `Event::Copy` and returns early before pushing `Event::Key { key: Key::C }`.
                    tx.push('\x03');
                }
                egui::Event::Cut => {
                    tx.push('\x18');
                }
                egui::Event::Paste(pasted) => {
                    let norm = pasted.replace("\r\n", "\n").replace('\r', "\n");
                    tx.push_str(&norm);
                }
                egui::Event::Text(text) | egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                    if !i.modifiers.ctrl && !i.modifiers.command {
                        for ch in text.chars() {
                            if ch >= ' ' {
                                tx.push(ch);
                            }
                        }
                    }
                }
                egui::Event::Key {
                    key,
                    physical_key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    let eff_key = physical_key.unwrap_or(*key);
                    if modifiers.ctrl || modifiers.command || i.modifiers.ctrl || i.modifiers.command {
                        match eff_key {
                            egui::Key::A => tx.push('\x01'),
                            egui::Key::B => tx.push('\x02'),
                            egui::Key::C => tx.push('\x03'),
                            egui::Key::D => tx.push('\x04'),
                            egui::Key::E => tx.push('\x05'),
                            egui::Key::F => tx.push('\x06'),
                            egui::Key::G => tx.push('\x07'),
                            egui::Key::H => tx.push('\x08'),
                            egui::Key::J => tx.push('\n'),
                            egui::Key::K => tx.push('\x0b'),
                            egui::Key::L => {
                                clear_scrollback = true;
                                tx.push('\x0c');
                            }
                            egui::Key::N => tx.push('\x0e'),
                            egui::Key::P => tx.push('\x10'),
                            egui::Key::Q => tx.push('\x11'),
                            egui::Key::R => tx.push('\x12'),
                            egui::Key::S => tx.push('\x13'),
                            egui::Key::T => tx.push('\x14'),
                            egui::Key::U => tx.push('\x15'),
                            egui::Key::W => tx.push('\x17'),
                            egui::Key::X => tx.push('\x18'),
                            egui::Key::Y => tx.push('\x19'),
                            egui::Key::Z => tx.push('\x1a'),
                            egui::Key::Backslash => tx.push('\x1c'),
                            egui::Key::CloseBracket => tx.push('\x1d'),
                            _ => {}
                        }
                    } else if !modifiers.alt && !i.modifiers.alt {
                        match eff_key {
                            egui::Key::Tab => tx.push('\t'),
                            egui::Key::Enter => tx.push('\n'),
                            egui::Key::Backspace => tx.push('\x7f'),
                            egui::Key::Delete => tx.push_str("\x1b[3~"),
                            egui::Key::Escape => tx.push('\x1b'),
                            egui::Key::ArrowUp => tx.push_str("\x1b[A"),
                            egui::Key::ArrowDown => tx.push_str("\x1b[B"),
                            egui::Key::ArrowRight => tx.push_str("\x1b[C"),
                            egui::Key::ArrowLeft => tx.push_str("\x1b[D"),
                            egui::Key::Home => tx.push('\x01'),
                            egui::Key::End => tx.push('\x05'),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }

        i.events
            .retain(|ev| !matches!(ev, egui::Event::Copy | egui::Event::Cut));

        for key in [
            egui::Key::Tab,
            egui::Key::Enter,
            egui::Key::Backspace,
            egui::Key::Delete,
            egui::Key::Escape,
            egui::Key::ArrowUp,
            egui::Key::ArrowDown,
            egui::Key::ArrowLeft,
            egui::Key::ArrowRight,
            egui::Key::Home,
            egui::Key::End,
        ] {
            let _ = i.consume_key(egui::Modifiers::NONE, key);
        }
    });

    (tx, clear_scrollback)
}

fn render_serial_console_tab(ui: &mut egui::Ui, app: &mut GuiAppState) {
    let vm_running = app.session.is_some();
    let snap = app
        .session
        .as_ref()
        .map(|s| s.serial_snapshot())
        .unwrap_or_default();

    let scroll_height = ui.available_height().max(60.0);
    let term_id = egui::Id::new(TTY_TERM_FOCUS_ID);
    let layer_id = ui.layer_id();

    // Register `term_id` in egui's focus cache every frame so `Focus::end_pass` never drops it.
    ui.ctx()
        .memory_mut(|m| m.interested_in_focus(term_id, layer_id));

    // Automatically focus the terminal when the serial tab is open and no other input widget has focus.
    if ui.ctx().memory(|m| m.focused().is_none()) {
        ui.ctx().memory_mut(|m| m.request_focus(term_id));
    }

    let has_focus = ui.ctx().memory(|m| m.has_focus(term_id));

    if has_focus {
        let (tx, clear_scrollback) = collect_terminal_key_events(ui.ctx(), term_id);
        if vm_running {
            if clear_scrollback {
                if let Some(ref sess) = app.session {
                    sess.clear_serial_lines();
                }
            }
            if !tx.is_empty() {
                app.send_serial_raw(&tx);
                app.terminal_autoscroll = true;
            }
        }
    }

    let stroke_col = if has_focus && vm_running {
        Color32::from_rgb(70, 145, 110)
    } else {
        Color32::from_rgb(45, 52, 64)
    };

    let mut active_cursor_rect = None;

    let frame_resp = egui::Frame::canvas(ui.style())
        .fill(Color32::from_rgb(14, 16, 20))
        .stroke(Stroke::new(1.0, stroke_col))
        .inner_margin(Margin::symmetric(10.0, 6.0))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("tty_serial_scroll")
                .auto_shrink([false, false])
                .max_height(scroll_height)
                .stick_to_bottom(app.terminal_autoscroll)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 2.0;

                    if !vm_running && snap.lines.is_empty() && snap.active_line.is_empty() {
                        ui.label(
                            RichText::new(
                                "Терминал ttyAMA0 не активен (ВМ остановлена). Нажмите «Запустить образ (Старт ВМ)» в верхней панели.",
                            )
                            .font(FontId::monospace(13.2))
                            .color(Color32::from_rgb(145, 155, 170)),
                        );
                    } else {
                        for line in &snap.lines {
                            let clean = strip_ansi_and_control_chars(line);
                            let col = classify_serial_line_color(&clean);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&clean)
                                        .font(FontId::monospace(13.2))
                                        .color(col),
                                )
                                .selectable(true),
                            );
                        }

                        if vm_running || !snap.active_line.is_empty() {
                            let job = build_active_line_job(
                                &snap.active_line,
                                snap.cursor_col,
                                has_focus && vm_running,
                            );
                            let active_resp = ui.add(
                                egui::Label::new(job).sense(egui::Sense::hover()),
                            );
                            active_cursor_rect = Some(active_resp.rect);
                        }
                    }

                    let rem_h = ui.available_height().max(18.0);
                    ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), rem_h),
                        egui::Sense::hover(),
                    );
                });
        });

    let term_rect = frame_resp.response.rect;
    ui.ctx()
        .check_for_id_clash(term_id, term_rect, TTY_TERM_FOCUS_ID);
    if ui.input(|i| i.pointer.any_pressed()) {
        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
            if term_rect.contains(pos) && !ui.ctx().memory(|m| m.has_focus(term_id)) {
                ui.ctx().memory_mut(|m| m.request_focus(term_id));
            }
        }
    }

    ui.ctx().memory_mut(|m| {
        m.set_focus_lock_filter(
            term_id,
            egui::EventFilter {
                tab: true,
                horizontal_arrows: true,
                vertical_arrows: true,
                escape: true,
            },
        );
    });

    if has_focus && vm_running {
        let cursor_rect = active_cursor_rect.unwrap_or(term_rect);
        ui.ctx().output_mut(|o| {
            o.ime = Some(egui::output::IMEOutput {
                rect: term_rect,
                cursor_rect,
            });
        });
    }
}

fn render_maind_log_tab(ui: &mut egui::Ui, app: &mut GuiAppState) {
    let lines = app
        .latest_state
        .as_ref()
        .map(|s| s.maind_logs.clone())
        .unwrap_or_default();
    let avail_h = ui.available_height().max(60.0);

    egui::Frame::canvas(ui.style())
        .fill(Color32::from_rgb(16, 18, 22))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("maind_log_scroll")
                .auto_shrink([false, false])
                .max_height(avail_h)
                .stick_to_bottom(app.terminal_autoscroll)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    if lines.is_empty() {
                        ui.label(
                            RichText::new("Лог /tmp/daemons_logs/maind.log пока пуст.")
                                .font(FontId::monospace(13.5))
                                .color(Color32::from_rgb(145, 155, 170)),
                        );
                    } else {
                        for line in &lines {
                            let clean = strip_ansi_and_control_chars(line);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(clean)
                                        .font(FontId::monospace(13.2))
                                        .color(Color32::from_rgb(215, 225, 240)),
                                )
                                .selectable(true),
                            );
                        }
                    }
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_egui_focus_retention_ascii_and_unicode_events() {
        let ctx = egui::Context::default();
        let term_id = egui::Id::new(TTY_TERM_FOCUS_ID);

        // Frame 1: acquire focus
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let rect = ui.max_rect();
                ui.ctx()
                    .memory_mut(|m| m.interested_in_focus(term_id, ui.layer_id()));
                if ui.ctx().memory(|m| m.focused().is_none()) {
                    ui.ctx().memory_mut(|m| m.request_focus(term_id));
                }
                ui.ctx().check_for_id_clash(term_id, rect, TTY_TERM_FOCUS_ID);
            });
        });

        // Frame 2: lock focus filter
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let rect = ui.max_rect();
                ui.ctx()
                    .memory_mut(|m| m.interested_in_focus(term_id, ui.layer_id()));
                assert!(ui.ctx().memory(|m| m.has_focus(term_id)));
                let _ = collect_terminal_key_events(ui.ctx(), term_id);
                ui.ctx().check_for_id_clash(term_id, rect, TTY_TERM_FOCUS_ID);
            });
        });

        // Frame 3: send ASCII + Unicode/Cyrillic + Tab + Enter
        let mut raw = egui::RawInput::default();
        raw.events.push(egui::Event::Text("echo Привет ".to_string()));
        raw.events
            .push(egui::Event::Ime(egui::ImeEvent::Commit("мир".to_string())));
        raw.events.push(egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: Some(egui::Key::Tab),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        raw.events.push(egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: Some(egui::Key::Enter),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });

        let mut captured = String::new();
        let _ = ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let rect = ui.max_rect();
                ui.ctx()
                    .memory_mut(|m| m.interested_in_focus(term_id, ui.layer_id()));
                assert!(ui.ctx().memory(|m| m.has_focus(term_id)));
                let (tx, _) = collect_terminal_key_events(ui.ctx(), term_id);
                captured = tx;
                ui.ctx().check_for_id_clash(term_id, rect, TTY_TERM_FOCUS_ID);
            });
        });
        assert_eq!(captured, "echo Привет мир\t\n");

        // Frame 4: verify Tab on Frame 3 did NOT steal focus away from `term_id`
        let mut raw4 = egui::RawInput::default();
        raw4.events.push(egui::Event::Key {
            key: egui::Key::ArrowUp,
            physical_key: Some(egui::Key::ArrowUp),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        let mut captured4 = String::new();
        let _ = ctx.run(raw4, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let rect = ui.max_rect();
                ui.ctx()
                    .memory_mut(|m| m.interested_in_focus(term_id, ui.layer_id()));
                assert!(ui.ctx().memory(|m| m.has_focus(term_id)));
                let (tx, _) = collect_terminal_key_events(ui.ctx(), term_id);
                captured4 = tx;
                ui.ctx().check_for_id_clash(term_id, rect, TTY_TERM_FOCUS_ID);
            });
        });
        assert_eq!(captured4, "\x1b[A");

        // Frame 5: verify Ctrl+C (`Event::Copy` from `egui-winit` and `Event::Key::C`) -> \x03
        // and Ctrl+D (`Event::Key::D`) -> \x04
        let mut raw5 = egui::RawInput::default();
        raw5.modifiers.ctrl = true;
        raw5.modifiers.command = true;
        raw5.events.push(egui::Event::Copy);
        raw5.events.push(egui::Event::Key {
            key: egui::Key::D,
            physical_key: Some(egui::Key::D),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers {
                ctrl: true,
                command: true,
                ..Default::default()
            },
        });
        let mut captured5 = String::new();
        let _ = ctx.run(raw5, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let rect = ui.max_rect();
                ui.ctx()
                    .memory_mut(|m| m.interested_in_focus(term_id, ui.layer_id()));
                assert!(ui.ctx().memory(|m| m.has_focus(term_id)));
                let (tx, _) = collect_terminal_key_events(ui.ctx(), term_id);
                captured5 = tx;
                ui.ctx().check_for_id_clash(term_id, rect, TTY_TERM_FOCUS_ID);
            });
        });
        assert_eq!(captured5, "\x03\x04");
    }
}
