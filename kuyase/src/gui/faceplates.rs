use crate::gui::canvas::draw_ring_led;
use crate::gui::display_geom::{
    build_local_segments_from_text, clock_segment_lines, hit_test_clock_segment,
    hit_test_matrix_pixel, render_text_to_matrix_21x12, segment_brightness,
};
use crate::gui::state::GuiAppState;
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use protocol::{BridgeStateSnapshot, HostCommand};
use std::collections::BTreeMap;

fn draw_matrix_dot(painter: &egui::Painter, pos: Pos2, radius: f32, val: u8) {
    if val > 0 {
        let alpha_glow = (val / 4).max(16);
        painter.circle_filled(
            pos,
            radius * 1.22,
            Color32::from_rgba_unmultiplied(140, 205, 255, alpha_glow),
        );
        let c = val.max(48);
        painter.circle_filled(
            pos,
            radius,
            Color32::from_rgb(c, c, (c as u16 + 15).min(255) as u8),
        );
    } else {
        painter.circle_filled(pos, radius * 0.88, Color32::from_rgb(26, 30, 38));
        painter.circle_stroke(
            pos,
            radius * 0.88,
            Stroke::new(0.8, Color32::from_rgb(44, 50, 62)),
        );
    }
}

fn draw_segment_clock(painter: &egui::Painter, rect: Rect, segments: &BTreeMap<String, u8>) {
    painter.rect_filled(rect, 6.0, Color32::from_rgb(13, 16, 21));
    painter.rect_stroke(rect, 6.0, Stroke::new(1.0, Color32::from_rgb(51, 59, 72)));

    let seg_color = |br: u8| {
        if br > 0 {
            Color32::from_rgba_unmultiplied(232, 246, 255, br.max(55))
        } else {
            Color32::from_rgb(37, 43, 54)
        }
    };

    for (name, p0, p1) in clock_segment_lines(rect) {
        let br = segment_brightness(segments, &name);
        painter.line_segment([p0, p1], Stroke::new(3.6, seg_color(br)));
    }

    let digit_h = 40.0;
    let start_x = rect.center().x - 66.0;
    let top_y = rect.center().y - digit_h * 0.5;
    let colon_x = start_x + 2.0 * 34.0 - 1.0;

    let p3 = segment_brightness(segments, "p3");
    let q3 = segment_brightness(segments, "q3");
    painter.circle_filled(Pos2::new(colon_x, top_y + 13.0), 2.8, seg_color(p3));
    painter.circle_filled(Pos2::new(colon_x, top_y + 27.0), 2.8, seg_color(q3));

    let deg_br = segment_brightness(segments, "deg");
    painter.circle_stroke(
        Pos2::new(start_x + 4.0 * 34.0 + 14.0, top_y + 6.0),
        4.2,
        Stroke::new(2.4, seg_color(deg_br)),
    );
}

pub fn render_segment_clock_faceplate(
    ui: &mut egui::Ui,
    app: &mut GuiAppState,
    board_slug: &str,
    st: &BridgeStateSnapshot,
    bridge_online: bool,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new("7-сегментный дисплей (45 сегментов)")
                .small()
                .color(Color32::from_rgb(165, 175, 190)),
        );
        ui.separator();
        ui.label(
            RichText::new("ЛКМ: рисовать | ПКМ: стирать")
                .small()
                .color(Color32::from_rgb(130, 195, 255)),
        );
        ui.separator();
        ui.label(
            RichText::new(format!("PWM: {}", app.clock_brightness))
                .small()
                .monospace()
                .color(Color32::from_rgb(155, 175, 200)),
        );
    });

    if bridge_online && st.clock_segments.values().any(|&v| v > 0) {
        app.clock_active_mask.clear();
        for (k, &v) in &st.clock_segments {
            if v > 0 {
                app.clock_active_mask.insert(k.clone(), true);
            }
        }
    }

    let mut active_segs = BTreeMap::new();
    if bridge_online && !st.clock_segments.is_empty() {
        active_segs = st.clock_segments.clone();
    } else {
        for (k, &on) in &app.clock_active_mask {
            if on && app.clock_brightness > 0 {
                active_segs.insert(k.clone(), app.clock_brightness);
            }
        }
    }

    let (crect, cresp) = ui.allocate_exact_size(
        Vec2::new(ui.available_width().max(240.0), 64.0),
        Sense::click_and_drag(),
    );
    let cp = ui.painter_at(crect);
    draw_segment_clock(&cp, crect, &active_segs);

    let lmb_down = ui.input(|i| i.pointer.primary_down());
    let rmb_down = ui.input(|i| i.pointer.secondary_down());
    let lmb_clicked = cresp.clicked();
    let rmb_clicked = cresp.secondary_clicked();
    if (lmb_down || rmb_down || lmb_clicked || rmb_clicked)
        && (cresp.hovered() || cresp.dragged() || lmb_clicked || rmb_clicked)
    {
        if let Some(ptr) = ui
            .input(|i| i.pointer.interact_pos())
            .or_else(|| cresp.interact_pointer_pos())
        {
            if let Some(seg_key) = hit_test_clock_segment(crect, ptr) {
                let draw_mode = (lmb_down || lmb_clicked) && !(rmb_down || rmb_clicked);
                let is_on = app.clock_active_mask.get(&seg_key).copied().unwrap_or(false)
                    && segment_brightness(&active_segs, &seg_key) > 0;
                if draw_mode != is_on {
                    let next_val = if draw_mode {
                        if app.clock_brightness == 0 {
                            app.clock_brightness = 255;
                        }
                        app.clock_active_mask.insert(seg_key.clone(), true);
                        app.clock_brightness
                    } else {
                        app.clock_active_mask.remove(&seg_key);
                        0
                    };
                    if let Some(ref mut latest) = app.latest_state {
                        latest.clock_segments.insert(seg_key.clone(), next_val);
                    }
                    let mut map = BTreeMap::new();
                    map.insert(seg_key, next_val);
                    app.send_command(HostCommand::SetClockDisplay {
                        segments: Some(map),
                        replace: Some(false),
                        pixels: None,
                        led_frame: None,
                        brightness: Some(app.clock_brightness),
                        scale_brightness: None,
                        text: None,
                    });
                }
            }
        }
    }

    ui.horizontal_wrapped(|ui| {
        let resp = ui.add(
            egui::TextEdit::singleline(&mut app.clock_text_input)
                .desired_width(82.0)
                .hint_text("19:30"),
        );
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if enter || ui.button("Вывести").clicked() {
            let txt = app.clock_text_input.clone();
            if !txt.is_empty() && app.clock_brightness == 0 {
                app.clock_brightness = 255;
            }
            let built = build_local_segments_from_text(&txt, 255);
            app.clock_active_mask = built
                .into_iter()
                .filter(|(_, v)| *v > 0)
                .map(|(k, _)| (k, true))
                .collect();
            if let Some(ref mut latest) = app.latest_state {
                latest.clock_segments =
                    build_local_segments_from_text(&txt, app.clock_brightness);
            }
            app.send_command(HostCommand::SetClockDisplay {
                segments: None,
                replace: None,
                pixels: None,
                led_frame: None,
                brightness: Some(app.clock_brightness),
                scale_brightness: None,
                text: Some(txt),
            });
        }
        for preset in ["19:30", "+24°", "-05°", ""] {
            let lbl = if preset.is_empty() { "Погасить" } else { preset };
            if ui.button(lbl).clicked() {
                app.clock_text_input = preset.to_string();
                let br = if preset.is_empty() {
                    app.clock_active_mask.clear();
                    0
                } else {
                    if app.clock_brightness == 0 {
                        app.clock_brightness = 255;
                    }
                    let built = build_local_segments_from_text(preset, 255);
                    app.clock_active_mask = built
                        .into_iter()
                        .filter(|(_, v)| *v > 0)
                        .map(|(k, _)| (k, true))
                        .collect();
                    app.clock_brightness
                };
                if let Some(ref mut latest) = app.latest_state {
                    latest.clock_segments = build_local_segments_from_text(preset, br);
                }
                app.send_command(HostCommand::SetClockDisplay {
                    segments: None,
                    replace: None,
                    pixels: None,
                    led_frame: None,
                    brightness: Some(br),
                    scale_brightness: None,
                    text: Some(preset.to_string()),
                });
            }
        }
    });

    ui.horizontal(|ui| {
        ui.label(RichText::new("Яркость PWM:").small());
        if ui
            .add(egui::Slider::new(&mut app.clock_brightness, 0..=255))
            .changed()
        {
            if let Some(ref mut latest) = app.latest_state {
                for (k, v) in latest.clock_segments.iter_mut() {
                    if app.clock_active_mask.get(k).copied().unwrap_or(false) {
                        *v = app.clock_brightness;
                    } else {
                        *v = 0;
                    }
                }
            }
            app.send_command(HostCommand::SetClockDisplay {
                segments: None,
                replace: None,
                pixels: None,
                led_frame: None,
                brightness: None,
                scale_brightness: Some(app.clock_brightness),
                text: None,
            });
        }
    });

    render_aux_logo_strip(ui, app, board_slug, st);
}

pub fn render_aux_logo_strip(
    ui: &mut egui::Ui,
    app: &mut GuiAppState,
    board_slug: &str,
    st: &BridgeStateSnapshot,
) {
    if st.aux_light_kind == "logo" || matches!(board_slug, "cucumber" | "pickle" | "mango") {
        let count = if !st.logo_pixels.is_empty() {
            st.logo_pixels.len()
        } else if board_slug == "mango" {
            3
        } else {
            6
        };
        let hover_desc = if board_slug == "mango" {
            "3-LED RGBW панель логотипа «oknix» на корпусе Яндекс Станции Стрит (mango):\n\
             • Аппаратный драйвер: Awinic AW21012 (leds-aw2101x.ko) + фреймбуфер quasar_led_ring.ko\n\
             • Запись через фреймбуфер /dev/fb1 (name=\"logo\" / \"oknix\"): 9 байт RGB24 (3×3) или 12 байт RGBW32 (3×4)\n\
             • Поканальное управление через sysfs: /sys/class/leds/mango:{red,green,blue}:logo{0..2}/brightness (0..255)\n\
             • Штатные анимации Алисы: /system/vendor/quasar/ledpatterns/oknix/*.led"
        } else {
            "Дополнительная адресная RGB-подсветка логотипа Алисы на фронтальной панели:\n\
             • Запись кадра через фреймбуфер /dev/fb1 (name=\"logo\"): 3 байта RGB на каждый светодиод\n\
             • Поканальное управление через sysfs: /sys/class/leds/<prefix>:{red,green,blue}:logo{0..N}/brightness (0..255)"
        };

        ui.horizontal_wrapped(|ui| {
            let lbl_text = if board_slug == "mango" {
                format!("Логотип oknix (/dev/fb1, {} LED): (?)", count)
            } else {
                format!("Логотип (/dev/fb1, {} LED): (?)", count)
            };
            ui.label(
                RichText::new(lbl_text)
                    .small()
                    .strong()
                    .color(Color32::from_rgb(160, 185, 215)),
            )
            .on_hover_text(hover_desc);

            let (lrect, lresp) =
                ui.allocate_exact_size(Vec2::new((count as f32) * 20.0 + 16.0, 20.0), Sense::hover());
            lresp.on_hover_text(hover_desc);
            let lp = ui.painter_at(lrect);
            for i in 0..count {
                let rgb = st
                    .logo_pixels
                    .get(i)
                    .copied()
                    .unwrap_or([0, 185, 235]);
                let x = lrect.left() + 10.0 + (i as f32) * 20.0;
                draw_ring_led(&lp, Pos2::new(x, lrect.center().y), 4.6, rgb);
            }

            if app.session.is_some() {
                if ui
                    .small_button("Фиолетовый")
                    .on_hover_text("Записать фиолетовый цвет Алисы (#6B0FFF) в /dev/fb1")
                    .clicked()
                {
                    app.send_serial_command(
                        "printf '\\x6b\\x0f\\xff\\x6b\\x0f\\xff\\x6b\\x0f\\xff\\x6b\\x0f\\xff\\x6b\\x0f\\xff\\x6b\\x0f\\xff' > /dev/fb1",
                    );
                }
                if ui
                    .small_button("Бирюзовый")
                    .on_hover_text("Записать бирюзовый цвет (#22CCFF) в /dev/fb1")
                    .clicked()
                {
                    app.send_serial_command(
                        "printf '\\x22\\xcc\\xff\\x22\\xcc\\xff\\x22\\xcc\\xff\\x22\\xcc\\xff\\x22\\xcc\\xff\\x22\\xcc\\xff' > /dev/fb1",
                    );
                }
                if ui
                    .small_button("Погасить")
                    .on_hover_text("Очистить /dev/fb1 и сбросить яркость logo0..N в sysfs в 0")
                    .clicked()
                {
                    app.send_serial_command(
                        "for d in /sys/class/leds/*:logo*; do [ -d \"$d\" ] && echo 0 > \"$d/brightness\"; done; dd if=/dev/zero of=/dev/fb1 bs=18 count=1 2>/dev/null || true",
                    );
                }
            }
        });
    }
}

pub fn render_matrix_21x12_faceplate(
    ui: &mut egui::Ui,
    app: &mut GuiAppState,
    st: &BridgeStateSnapshot,
    bridge_online: bool,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new("Матрица 21x12 LED (/dev/fb1)")
                .small()
                .color(Color32::from_rgb(165, 175, 190)),
        );
        ui.separator();
        ui.label(
            RichText::new("ЛКМ: рисовать | ПКМ: стирать")
                .small()
                .color(Color32::from_rgb(130, 195, 255)),
        );
        ui.separator();
        ui.label(
            RichText::new(format!("PWM: {}", app.clock_brightness))
                .small()
                .monospace()
                .color(Color32::from_rgb(155, 175, 200)),
        );
    });

    if app.matrix_custom_pixels.len() < 252 {
        app.matrix_custom_pixels.resize(252, 0);
    }
    if app.matrix_active_mask.len() < 252 {
        app.matrix_active_mask.resize(252, false);
    }

    if bridge_online && st.screen_pixels.len() == 252 && app.clock_brightness > 0 {
        app.matrix_custom_pixels = st.screen_pixels.clone();
        if st.screen_pixels.iter().any(|&v| v > 0) {
            app.matrix_active_mask = st.screen_pixels.iter().map(|&v| v > 0).collect();
        }
    }

    let cell = 13.0;
    let msize = Vec2::new(21.0 * cell + 10.0, 12.0 * cell + 10.0);
    let (mrect, mresp) = ui.allocate_exact_size(msize, Sense::click_and_drag());
    let mp = ui.painter_at(mrect);
    mp.rect_filled(mrect, 6.0, Color32::from_rgb(13, 15, 20));
    mp.rect_stroke(mrect, 6.0, Stroke::new(1.0, Color32::from_rgb(48, 56, 70)));

    let lmb_down = ui.input(|i| i.pointer.primary_down());
    let rmb_down = ui.input(|i| i.pointer.secondary_down());
    if (lmb_down || rmb_down) && (mresp.hovered() || mresp.dragged() || mresp.clicked()) {
        if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
            if let Some(idx) = hit_test_matrix_pixel(mrect, cell, p) {
                let draw_mode = lmb_down && !rmb_down;
                if draw_mode && app.clock_brightness == 0 {
                    app.clock_brightness = 255;
                }
                let next_v = if draw_mode {
                    app.clock_brightness.max(1)
                } else {
                    0
                };
                if app.matrix_custom_pixels[idx] != next_v
                    || app.matrix_active_mask[idx] != draw_mode
                {
                    app.matrix_active_mask[idx] = draw_mode;
                    app.matrix_custom_pixels[idx] = next_v;
                    if let Some(ref mut latest) = app.latest_state {
                        if latest.screen_pixels.len() < 252 {
                            latest.screen_pixels.resize(252, 0);
                        }
                        latest.screen_pixels[idx] = next_v;
                    }
                    app.send_command(HostCommand::SetClockDisplay {
                        segments: None,
                        replace: None,
                        pixels: Some(app.matrix_custom_pixels.clone()),
                        led_frame: None,
                        brightness: Some(app.clock_brightness),
                        scale_brightness: None,
                        text: None,
                    });
                }
            }
        }
    }

    for row in 0..12 {
        for col in 0..21 {
            let idx = row * 21 + col;
            let v = if app.clock_brightness == 0 {
                0
            } else {
                app.matrix_custom_pixels.get(idx).copied().unwrap_or(0)
            };
            let dot_pos = Pos2::new(
                mrect.left() + 5.0 + (col as f32 + 0.5) * cell,
                mrect.top() + 5.0 + (row as f32 + 0.5) * cell,
            );
            draw_matrix_dot(&mp, dot_pos, 4.2, v);
        }
    }

    ui.horizontal_wrapped(|ui| {
        let resp = ui.add(
            egui::TextEdit::singleline(&mut app.matrix_text_input)
                .desired_width(82.0)
                .hint_text("19:30"),
        );
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if enter || ui.button("Вывести").clicked() {
            let txt = app.matrix_text_input.clone();
            if !txt.is_empty() && app.clock_brightness == 0 {
                app.clock_brightness = 255;
            }
            let mask_px = render_text_to_matrix_21x12(&txt, 255);
            app.matrix_active_mask = mask_px.iter().map(|&v| v > 0).collect();
            app.matrix_custom_pixels = render_text_to_matrix_21x12(&txt, app.clock_brightness);
            if let Some(ref mut latest) = app.latest_state {
                latest.screen_pixels = app.matrix_custom_pixels.clone();
            }
            app.send_command(HostCommand::SetClockDisplay {
                segments: None,
                replace: None,
                pixels: None,
                led_frame: None,
                brightness: Some(app.clock_brightness),
                scale_brightness: None,
                text: Some(txt),
            });
        }
        for preset in ["19:30", "+24°", "-05°", ""] {
            let lbl = if preset.is_empty() { "Погасить" } else { preset };
            if ui.button(lbl).clicked() {
                app.matrix_text_input = preset.to_string();
                let br = if preset.is_empty() {
                    app.matrix_active_mask.fill(false);
                    app.matrix_custom_pixels.fill(0);
                    0
                } else {
                    if app.clock_brightness == 0 {
                        app.clock_brightness = 255;
                    }
                    let mask_px = render_text_to_matrix_21x12(preset, 255);
                    app.matrix_active_mask = mask_px.iter().map(|&v| v > 0).collect();
                    app.matrix_custom_pixels =
                        render_text_to_matrix_21x12(preset, app.clock_brightness);
                    app.clock_brightness
                };
                if let Some(ref mut latest) = app.latest_state {
                    latest.screen_pixels = app.matrix_custom_pixels.clone();
                }
                app.send_command(HostCommand::SetClockDisplay {
                    segments: None,
                    replace: None,
                    pixels: None,
                    led_frame: None,
                    brightness: Some(br),
                    scale_brightness: None,
                    text: Some(preset.to_string()),
                });
            }
        }
        if ui.button("Волна").clicked() {
            if app.clock_brightness == 0 {
                app.clock_brightness = 255;
            }
            app.matrix_active_mask.fill(false);
            app.matrix_custom_pixels.fill(0);
            for c in 0..21 {
                let h = (3.0 + ((c as f32) * 0.7).sin() * 3.0).round() as usize;
                for r in (11usize.saturating_sub(h))..11 {
                    let idx = r * 21 + c;
                    app.matrix_active_mask[idx] = true;
                    app.matrix_custom_pixels[idx] = app.clock_brightness;
                }
            }
            if let Some(ref mut latest) = app.latest_state {
                latest.screen_pixels = app.matrix_custom_pixels.clone();
            }
            app.send_command(HostCommand::SetClockDisplay {
                segments: None,
                replace: None,
                pixels: Some(app.matrix_custom_pixels.clone()),
                led_frame: None,
                brightness: Some(app.clock_brightness),
                scale_brightness: None,
                text: None,
            });
        }
    });

    ui.horizontal(|ui| {
        ui.label(RichText::new("Яркость PWM:").small());
        if ui
            .add(egui::Slider::new(&mut app.clock_brightness, 0..=255))
            .changed()
        {
            for idx in 0..252 {
                app.matrix_custom_pixels[idx] = if app.matrix_active_mask[idx] {
                    app.clock_brightness
                } else {
                    0
                };
            }
            if let Some(ref mut latest) = app.latest_state {
                latest.screen_pixels = app.matrix_custom_pixels.clone();
            }
            app.send_command(HostCommand::SetClockDisplay {
                segments: None,
                replace: None,
                pixels: None,
                led_frame: None,
                brightness: None,
                scale_brightness: Some(app.clock_brightness),
                text: None,
            });
        }
    });
}
