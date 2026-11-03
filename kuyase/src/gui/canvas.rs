pub use crate::gui::display_geom::{
    board_has_matrix_21x12, board_has_segment_clock, build_local_segments_from_text,
    clock_segment_lines, default_board_ring_leds, doa_angle_from_pointer, hit_test_clock_segment,
    hit_test_matrix_pixel, led_ring_position, render_text_to_matrix_21x12, segment_brightness,
};
use crate::gui::faceplates::{
    render_aux_logo_strip, render_matrix_21x12_faceplate, render_segment_clock_faceplate,
};
use crate::gui::state::GuiAppState;
use eframe::egui::{self, Color32, Margin, Pos2, RichText, Sense, Stroke, Vec2};
use protocol::{BridgeStateSnapshot, HostCommand};
use std::collections::BTreeMap;
use std::time::Duration;

pub(crate) fn draw_ring_led(painter: &egui::Painter, pos: Pos2, radius: f32, rgb: [u8; 3]) {
    let [r, g, b] = rgb;
    if r > 0 || g > 0 || b > 0 {
        painter.circle_filled(
            pos,
            radius * 1.55,
            Color32::from_rgba_unmultiplied(r, g, b, 52),
        );
        painter.circle_filled(pos, radius, Color32::from_rgb(r, g, b));
        painter.circle_stroke(
            pos,
            radius + 0.5,
            Stroke::new(0.8, Color32::from_rgba_unmultiplied(15, 18, 24, 180)),
        );
    } else {
        painter.circle_filled(pos, radius, Color32::from_rgb(28, 32, 40));
        painter.circle_stroke(pos, radius, Stroke::new(1.0, Color32::from_rgb(56, 64, 78)));
    }
}

fn stage_box_frame(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(Color32::from_rgb(23, 25, 30))
        .stroke(Stroke::new(1.0, Color32::from_rgb(56, 62, 74)))
        .rounding(8.0)
        .inner_margin(Margin::same(10.0))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add_contents(ui);
        });
}

pub fn render_twin_canvas(ui: &mut egui::Ui, app: &mut GuiAppState) {
    let bridge_online = app
        .bridge
        .as_ref()
        .map(|b| b.is_connected())
        .unwrap_or(false);
    if bridge_online {
        ui.ctx().request_repaint_after(Duration::from_millis(16));
    }

    let board_slug = app
        .latest_state
        .as_ref()
        .map(|s| s.board_slug.clone())
        .unwrap_or_else(|| app.config.selected_board.clone());

    let mut st = app.latest_state.clone().unwrap_or_else(|| {
        let mut segs = BTreeMap::new();
        for (k, &on) in &app.clock_active_mask {
            if on && app.clock_brightness > 0 {
                segs.insert(k.clone(), app.clock_brightness);
            }
        }
        BridgeStateSnapshot {
            board_slug: board_slug.clone(),
            board_name: crate::gui::board_display_label(&board_slug),
            ring_leds: default_board_ring_leds(&board_slug),
            tof_mm: app.tof_slider,
            doa_angle: app.doa_slider,
            volume_level: app.volume_slider,
            clock_segments: segs,
            screen_pixels: app.matrix_custom_pixels.clone(),
            ..Default::default()
        }
    });
    if !bridge_online {
        st.tof_mm = app.tof_slider;
        st.doa_angle = app.doa_slider;
        st.volume_level = app.volume_slider;
    }

    egui::ScrollArea::vertical()
        .id_salt("device_stage_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::none()
                .fill(Color32::from_rgb(43, 43, 43))
                .stroke(Stroke::new(1.0, Color32::from_rgb(60, 60, 60)))
                .rounding(10.0)
                .inner_margin(Margin::same(10.0))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(format!("{} ({})", st.board_name, st.board_slug))
                                .strong(),
                        );
                        ui.add(
                            egui::Button::new(
                                RichText::new(format!("Alice: {}", st.alice_state))
                                    .small()
                                    .strong()
                                    .color(Color32::from_rgb(130, 190, 255)),
                            )
                            .fill(Color32::from_rgb(28, 52, 82))
                            .rounding(99.0)
                            .sense(Sense::hover()),
                        );
                    });

                    ui.add_space(6.0);

                    // 1. Top Enclosure Box (Spacious LED Ring + Ambilight + DOA + ToF Hand)
                    stage_box_frame(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            let trig_txt = if st.ring_trigger.is_empty() {
                                format!("Драйвер LED: Кольцо {} LED", st.ring_leds)
                            } else {
                                format!("Драйвер LED: {}", st.ring_trigger)
                            };
                            let trig_col = if st.ring_trigger.contains("none") {
                                Color32::from_rgb(120, 210, 255)
                            } else if st.ring_trigger.contains("Stealth") {
                                Color32::from_rgb(255, 175, 70)
                            } else {
                                Color32::from_rgb(165, 175, 190)
                            };
                            ui.label(RichText::new(trig_txt).small().color(trig_col))
                                .on_hover_text(
                                    "Режим драйвера светового кольца:\n\
                                     • ledfb-ring (Quasar Native) — штатная анимация прошивки через фреймбуфер\n\
                                     • none (Custom Override) — прямое ручное управление яркостью через /sys/class/leds/*:ring*/brightness\n\
                                     • SN3236 (0x3c) — аппаратный I2C-контроллер на yandexmini",
                                );

                            if board_slug == "yandexmini" {
                                let tof_near = st.tof_mm < 400;
                                let (tof_fg, tof_bg) = if tof_near {
                                    (
                                        Color32::from_rgb(255, 225, 100),
                                        Color32::from_rgb(66, 52, 18),
                                    )
                                } else {
                                    (
                                        Color32::from_rgb(155, 205, 250),
                                        Color32::from_rgb(28, 42, 60),
                                    )
                                };
                                let tof_label = if tof_near {
                                    format!("Датчик ToF (VL53L0X): {} мм (РУКА)", st.tof_mm)
                                } else {
                                    format!("Датчик ToF (VL53L0X): {} мм", st.tof_mm)
                                };
                                ui.add(
                                    egui::Button::new(
                                        RichText::new(tof_label).small().monospace().color(tof_fg),
                                    )
                                    .fill(tof_bg)
                                    .rounding(99.0)
                                    .sense(Sense::hover()),
                                )
                                .on_hover_text(
                                    "Оптический лазерный дальномер приближения STM VL53L0X (устанавливался только в Яндекс Станцию Мини 1 / yandexmini).\n\
                                     Дистанция < 400 мм означает поднесённую ладонь над крышкой колонки (жест управления громкостью / остановки).\n\
                                     Дистанция 8190 мм — препятствий над колонкой нет.",
                                );
                            } else if board_slug == "orion" {
                                let us_near = st.tof_mm < 1500;
                                let (us_fg, us_bg) = if us_near {
                                    (
                                        Color32::from_rgb(255, 225, 100),
                                        Color32::from_rgb(66, 52, 18),
                                    )
                                } else {
                                    (
                                        Color32::from_rgb(155, 205, 250),
                                        Color32::from_rgb(28, 42, 60),
                                    )
                                };
                                let us_label = format!("Ультразвук 38.4 кГц: эхо {} мм", st.tof_mm);
                                ui.add(
                                    egui::Button::new(
                                        RichText::new(us_label).small().monospace().color(us_fg),
                                    )
                                    .fill(us_bg)
                                    .rounding(99.0)
                                    .sense(Sense::hover()),
                                )
                                .on_hover_text(
                                    "На Станции 3 (orion) нет оптического датчика VL53L0X (ToF).\n\
                                     Вместо него используется отдельный ультразвуковой TDM-тракт pcm.ultrasound (96 кГц, несущая 38.4 кГц) через четвёртый усилитель ACM8625S (2-002c) и микрофонную матрицу.",
                                );
                            } else {
                                let als_label =
                                    format!("Датчик света (ALS): {} лк", st.als_lux);
                                ui.add(
                                    egui::Button::new(
                                        RichText::new(als_label)
                                            .small()
                                            .monospace()
                                            .color(Color32::from_rgb(155, 205, 250)),
                                    )
                                    .fill(Color32::from_rgb(28, 42, 60))
                                    .rounding(99.0)
                                    .sense(Sense::hover()),
                                )
                                .on_hover_text(
                                    "Датчик внешней освещённости ALS (ltr308als01 в /sys/bus/iio/devices/iio:device0/in_illuminance_raw).\n\
                                     Используется прошивкой для автояркости светового кольца и дисплея часов (оптического датчика жестов VL53L0X на этой модели нет).",
                                );

                                if board_slug == "mango" {
                                    ui.add(
                                        egui::Button::new(
                                            RichText::new("АКБ CW2217 + IP2366 PD")
                                                .small()
                                                .monospace()
                                                .color(Color32::from_rgb(135, 235, 165)),
                                        )
                                        .fill(Color32::from_rgb(24, 52, 36))
                                        .rounding(99.0)
                                        .sense(Sense::hover()),
                                    )
                                    .on_hover_text(
                                        "Портативная подсистема питания Яндекс Станции Стрит (mango):\n\
                                         • Кулонометр АКБ CW2217: /sys/devices/virtual/cw2217_cls/cw2217_coulomb/{capacity,battery_vol,battery_current,battery_temp,soh,cycle}\n\
                                         • Контроллер зарядки IP2366 USB-C PD: /sys/devices/virtual/ip2366_cls/ip2366_charger/{charge_status,is_pd,charge_voltage,pd_protocol,switch_charge}\n\
                                         • Стандартный интерфейс Linux: /sys/class/power_supply/battery/*\n\
                                         • Усилитель TI ACM8625S (2-002c): профили mango-acm8625.cfg (в помещении) и mango-acm8625-outside.cfg (на улице)\n\
                                         Управлять параметрами в 1 клик можно справа в блоке «3. Специфичная периферия платы (mango)».",
                                    );
                                }
                            }
                        });

                        if st.led_trigger_conflict_count > 0 {
                            ui.add_space(2.0);
                            ui.horizontal_wrapped(|ui| {
                                let conflict_badge = egui::Button::new(
                                    RichText::new(format!(
                                        "Конфликт записи sysfs LED: {} (?)",
                                        st.led_trigger_conflict_count
                                    ))
                                    .small()
                                    .strong()
                                    .color(Color32::from_rgb(255, 130, 130)),
                                )
                                .fill(Color32::from_rgb(68, 26, 30))
                                .stroke(Stroke::new(1.0, Color32::from_rgb(150, 55, 60)))
                                .rounding(6.0)
                                .sense(Sense::hover());

                                ui.add(conflict_badge).on_hover_text(
                                    "Что означает «Конфликт записи sysfs LED» (Trigger Conflict):\n\
                                     Была выполнена прямая запись яркости в /sys/class/leds/*:ring*/brightness,\n\
                                     пока световое кольцо находилось под управлением триггера драйвера [ledfb-ring].\n\n\
                                     При активном [ledfb-ring] драйвер отклоняет прямую запись в sysfs и сбрасывает brightness в 0.\n\
                                     Чтобы управлять светодиодами вручную через sysfs, сначала переключите триггер в режим 'none':\n\
                                     echo none > /sys/class/leds/*:ring*/trigger",
                                );

                                if app.session.is_some() {
                                    let is_custom = st.ring_trigger.contains("none");
                                    if !is_custom
                                        && ui
                                            .small_button("Переключить в sysfs (none)")
                                            .on_hover_text("Отключить триггер ledfb-ring для прямой записи яркости через sysfs")
                                            .clicked()
                                    {
                                        app.send_serial_command(
                                            "for d in /sys/class/leds/*:ring*; do echo none > \"$d/trigger\"; done",
                                        );
                                    }
                                }
                            });
                        }

                        let ring_box_h = if board_slug == "orion" { 265.0 } else { 225.0 };
                        let canvas_size = Vec2::new(ui.available_width().max(260.0), ring_box_h);
                        let (rect, response) =
                            ui.allocate_exact_size(canvas_size, Sense::click_and_drag());
                        let painter = ui.painter_at(rect);
                        let center = rect.center();
                        let outer_r = (rect.height().min(rect.width()) * 0.40).clamp(80.0, 105.0);

                        if response.clicked() || response.dragged() {
                            if let Some(ptr) = response.interact_pointer_pos() {
                                let dist = ptr.distance(center);
                                if dist >= outer_r * 0.50 && dist <= outer_r * 1.32 {
                                    let angle = doa_angle_from_pointer(center, ptr);
                                    app.doa_slider = angle;
                                    app.send_command(HostCommand::SetSensor {
                                        doa_angle: Some(angle),
                                        mic_rms: None,
                                        als_lux: None,
                                        temp_c: None,
                                        tof_mm: None,
                                        swipe: None,
                                        swipe_direction: None,
                                    });
                                }
                            }
                        }

                        // Ambilight 12x2 RGBW outer halo (orion)
                        if st.aux_light_kind == "ambilight" || board_slug == "orion" {
                            let zones = st.logo_pixels.len().max(12);
                            painter.circle_stroke(
                                center,
                                outer_r + 16.0,
                                Stroke::new(1.0, Color32::from_rgb(46, 52, 66)),
                            );
                            for i in 0..zones {
                                let rgb = st
                                    .logo_pixels
                                    .get(i)
                                    .copied()
                                    .unwrap_or([90, 45, 130]);
                                let p = led_ring_position(center, outer_r + 16.0, i, zones);
                                draw_ring_led(&painter, p, 4.6, rgb);
                            }
                        }

                        painter.circle_filled(center, outer_r, Color32::from_rgb(24, 28, 36));
                        painter.circle_stroke(
                            center,
                            outer_r,
                            Stroke::new(2.0, Color32::from_rgb(66, 74, 90)),
                        );
                        painter.circle_filled(
                            center,
                            outer_r * 0.65,
                            Color32::from_rgb(16, 19, 25),
                        );

                        let ring_count = st
                            .ring_leds
                            .max(st.ring_pixels.len())
                            .max(default_board_ring_leds(&board_slug))
                            .max(1);
                        let ring_r = if ring_count == 1 { 0.0 } else { outer_r * 0.85 };
                        let led_dot_r = match ring_count {
                            1..=6 => 6.4,
                            7..=16 => 5.2,
                            17..=24 => 4.2,
                            25..=36 => 3.3,
                            _ => 1.9,
                        };
                        for idx in 0..ring_count {
                            let rgb = st.ring_pixels.get(idx).copied().unwrap_or([0, 165, 210]);
                            let pos = led_ring_position(center, ring_r, idx, ring_count);
                            draw_ring_led(&painter, pos, led_dot_r, rgb);
                        }

                        let doa_pos =
                            led_ring_position(center, outer_r + 4.0, st.doa_angle as usize, 360);
                        painter.line_segment(
                            [center, doa_pos],
                            Stroke::new(1.6, Color32::from_rgba_unmultiplied(0, 229, 255, 140)),
                        );
                        painter.circle_filled(doa_pos, 5.2, Color32::from_rgb(0, 229, 255));

                        if board_slug == "yandexmini" && st.tof_mm < 400 {
                            painter.circle_filled(
                                center,
                                outer_r * 0.52,
                                Color32::from_rgba_unmultiplied(229, 165, 10, 55),
                            );
                            painter.circle_stroke(
                                center,
                                outer_r * 0.52,
                                Stroke::new(1.5, Color32::from_rgb(235, 175, 25)),
                            );
                            painter.text(
                                center,
                                egui::Align2::CENTER_CENTER,
                                format!("РУКА: {} мм\nDOA {} град", st.tof_mm, st.doa_angle),
                                egui::FontId::monospace(12.0),
                                Color32::from_rgb(255, 230, 110),
                            );
                        } else {
                            painter.text(
                                center,
                                egui::Align2::CENTER_CENTER,
                                format!(
                                    "DOA {} град\n{} LED | Громк {}/10",
                                    st.doa_angle, ring_count, st.volume_level
                                ),
                                egui::FontId::monospace(12.0),
                                Color32::from_rgb(195, 210, 230),
                            );
                        }
                    });

                    ui.add_space(6.0);

                    // 2. Dynamic Front Faceplate Display (Segment Clock OR 21x12 Matrix)
                    stage_box_frame(ui, |ui| {
                        if board_has_matrix_21x12(&board_slug, &st) {
                            render_matrix_21x12_faceplate(ui, app, &st, bridge_online);
                        } else if board_has_segment_clock(&board_slug, &st) {
                            render_segment_clock_faceplate(ui, app, &board_slug, &st, bridge_online);
                        } else if board_slug == "mango" {
                            ui.label(
                                RichText::new(
                                    "Индикация корпуса Станции Стрит (mango): 4-LED шкала заряда (/dev/fb0) + 3-LED логотип oknix (/dev/fb1)",
                                )
                                .small()
                                .strong()
                                .color(Color32::from_rgb(175, 190, 210)),
                            );
                            ui.label(
                                RichText::new(
                                    "• Верхний блок (4 LED, AW9110): индикатор заряда АКБ (/dev/fb0 и /sys/class/leds/mango:*:ring1..4, паттерны ledpatterns/battery/*.led)\n\
                                     • Логотип oknix ниже (3 RGBW LED, AW21012): фронтальный индикатор Алисы (/dev/fb1 и /sys/class/leds/mango:*:logo0..2, паттерны ledpatterns/oknix/*.led)",
                                )
                                .small()
                                .color(Color32::from_rgb(140, 150, 168)),
                            );
                            ui.add_space(4.0);
                            render_aux_logo_strip(ui, app, &board_slug, &st);
                        } else {
                            ui.label(
                                RichText::new(format!(
                                    "Фронтальный экран отсутствует на плате {}",
                                    board_slug
                                ))
                                .small()
                                .color(Color32::from_rgb(155, 165, 180)),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "Индикация выполняется через верхнее кольцо ({} LED).",
                                    st.ring_leds.max(default_board_ring_leds(&board_slug))
                                ))
                                .small()
                                .color(Color32::from_rgb(135, 145, 160)),
                            );
                            render_aux_logo_strip(ui, app, &board_slug, &st);
                        }
                    });

                    ui.add_space(6.0);

                    // 3. Physical Enclosure Buttons (Linux uinput)
                    stage_box_frame(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new("Кнопки корпуса (uinput):")
                                    .small()
                                    .color(Color32::from_rgb(165, 175, 190)),
                            );
                            egui::ComboBox::from_id_salt("enclosure_press_mode")
                                .selected_text(match app.button_press_action.as_str() {
                                    "double" => "Двойное (double)",
                                    "triple" => "Тройное (triple)",
                                    "long" => "Удержание (long)",
                                    _ => "Одиночное (single)",
                                })
                                .width(155.0)
                                .show_ui(ui, |ui| {
                                    for (val, lbl) in [
                                        ("single", "Одиночное (single)"),
                                        ("double", "Двойное (double)"),
                                        ("triple", "Тройное (triple)"),
                                        ("long", "Удержание (long)"),
                                    ] {
                                        ui.selectable_value(
                                            &mut app.button_press_action,
                                            val.to_string(),
                                            lbl,
                                        );
                                    }
                                });
                        });
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            let mute_lbl = if st.mics_muted {
                                "Вкл. Микрофоны"
                            } else {
                                "Mute (Микрофоны)"
                            };
                            for (label, btn) in [
                                ("Action (Алиса)", "action"),
                                (mute_lbl, "mute"),
                                ("Громк -", "volume_down"),
                                ("Громк +", "volume_up"),
                            ] {
                                if ui.button(label).clicked() {
                                    app.send_command(HostCommand::PressButton {
                                        button: btn.to_string(),
                                        action: app.button_press_action.clone(),
                                    });
                                }
                            }
                        });
                    });
                });
        });
}
