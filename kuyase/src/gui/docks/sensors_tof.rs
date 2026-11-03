use super::{gnome_action_row, gnome_preferences_group, LED_PATTERNS};
use crate::gui::state::GuiAppState;
use eframe::egui::{self, Color32, RichText};
use protocol::HostCommand;

pub fn render_tof_and_sensors_card(ui: &mut egui::Ui, app: &mut GuiAppState) {
    let board_slug = app
        .latest_state
        .as_ref()
        .map(|s| s.board_slug.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| app.config.selected_board.clone());
    let desc = protocol::find_board_descriptor(&board_slug);
    let is_sn3236 = desc
        .map(|d| d.ring_driver == "i2c_sn3236")
        .unwrap_or(board_slug == "yandexmini");
    let has_ultrasound = desc
        .map(|d| d.has_ultrasound)
        .unwrap_or(board_slug == "orion");

    let (card_title, card_subtitle) = if is_sn3236 {
        (
            "2. Лазерный дальномер ToF (VL53L0X) и датчики среды",
            "Оптический датчик жестов руки VL53L0X (устанавливался только в Станцию Мини 1), освещённость ALS и температура SoC",
        )
    } else if has_ultrasound {
        (
            "2. Ультразвуковой тракт (38.4 кГц), освещённость ALS и датчики",
            "Дистанция акустического отражения для pcm.ultrasound (96 кГц / 38.4 кГц), датчик света light-sensor-top и температура SoC",
        )
    } else {
        (
            "2. Датчики среды (ALS, температура SoC) и световое кольцо",
            "Датчик внешней освещённости ltr308als01 (IIO), температура SoC и управление кольцом LED (оптического датчика VL53L0X на этой модели нет)",
        )
    };

    gnome_preferences_group(ui, card_title, card_subtitle, |ui| {
        if is_sn3236 {
            gnome_action_row(
                ui,
                "Импульс датчика ToF (STM VL53L0X)",
                "Кратковременное приближение ладони (140 мм на 0.9 с с возвратом к прежней дистанции)",
                false,
                |ui| {
                    let wave_btn = egui::Button::new(
                        RichText::new("Взмах руки (Импульс 140 мм)")
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .fill(Color32::from_rgb(53, 132, 228));
                    if ui.add(wave_btn).clicked() {
                        app.trigger_tof_wave();
                    }
                    if ui.button("Поднести ладонь (120 мм)").clicked() {
                        app.tof_wave_reset_at = None;
                        app.tof_slider = 120;
                        app.tof_restore_distance = 120;
                        app.send_command(HostCommand::SetSensor {
                            doa_angle: None,
                            mic_rms: None,
                            als_lux: None,
                            temp_c: None,
                            tof_mm: Some(120),
                            swipe: None,
                            swipe_direction: None,
                        });
                    }
                    if ui.button("Убрать руку (8190 мм)").clicked() {
                        app.tof_wave_reset_at = None;
                        app.tof_slider = 8190;
                        app.tof_restore_distance = 8190;
                        app.send_command(HostCommand::SetSensor {
                            doa_angle: None,
                            mic_rms: None,
                            als_lux: None,
                            temp_c: None,
                            tof_mm: Some(8190),
                            swipe: None,
                            swipe_direction: None,
                        });
                    }
                },
            );

            gnome_action_row(
                ui,
                "Точная дистанция ToF (VL53L0X)",
                "Ручная установка расстояния от ладони до крышки (40..8190 мм) для регулировки громкости жестом",
                false,
                |ui| {
                    if ui
                        .add(egui::Slider::new(&mut app.tof_slider, 40..=8190).suffix(" мм"))
                        .changed()
                    {
                        app.tof_wave_reset_at = None;
                        app.tof_restore_distance = app.tof_slider;
                        app.send_command(HostCommand::SetSensor {
                            doa_angle: None,
                            mic_rms: None,
                            als_lux: None,
                            temp_c: None,
                            tof_mm: Some(app.tof_slider),
                            swipe: None,
                            swipe_direction: None,
                        });
                    }
                },
            );
        } else if has_ultrasound {
            gnome_action_row(
                ui,
                "Акустическое отражение ультразвука (pcm.ultrasound, 38.4 кГц)",
                "Дистанция до препятствия (40..8190 мм), определяющая амплитуду отражённого эха на усилителе ACM8625S (2-002c)",
                false,
                |ui| {
                    if ui
                        .add(egui::Slider::new(&mut app.tof_slider, 40..=8190).suffix(" мм"))
                        .changed()
                    {
                        app.tof_wave_reset_at = None;
                        app.tof_restore_distance = app.tof_slider;
                        app.send_command(HostCommand::SetSensor {
                            doa_angle: None,
                            mic_rms: None,
                            als_lux: None,
                            temp_c: None,
                            tof_mm: Some(app.tof_slider),
                            swipe: None,
                            swipe_direction: None,
                        });
                    }
                    if ui.button("Ближнее эхо (180 мм)").clicked() {
                        app.tof_wave_reset_at = None;
                        app.tof_slider = 180;
                        app.tof_restore_distance = 180;
                        app.send_command(HostCommand::SetSensor {
                            doa_angle: None,
                            mic_rms: None,
                            als_lux: None,
                            temp_c: None,
                            tof_mm: Some(180),
                            swipe: None,
                            swipe_direction: None,
                        });
                    }
                    if ui.button("Свободное поле (8190 мм)").clicked() {
                        app.tof_wave_reset_at = None;
                        app.tof_slider = 8190;
                        app.tof_restore_distance = 8190;
                        app.send_command(HostCommand::SetSensor {
                            doa_angle: None,
                            mic_rms: None,
                            als_lux: None,
                            temp_c: None,
                            tof_mm: Some(8190),
                            swipe: None,
                            swipe_direction: None,
                        });
                    }
                },
            );
        }

        gnome_action_row(
            ui,
            "Громкость, датчики среды и паттерн LED",
            "Громкость (0..10), освещённость ALS (лк), температура SoC (C) и принудительный .led паттерн",
            true,
            |ui| {
                if ui
                    .add(egui::Slider::new(&mut app.volume_slider, 0..=10).text("Громк"))
                    .changed()
                {
                    app.send_command(HostCommand::SetVolume {
                        volume_level: app.volume_slider,
                    });
                }
                if ui
                    .add(egui::Slider::new(&mut app.als_slider, 0..=10000).text("лк"))
                    .changed()
                {
                    app.send_command(HostCommand::SetSensor {
                        doa_angle: None,
                        mic_rms: None,
                        als_lux: Some(app.als_slider),
                        temp_c: None,
                        tof_mm: None,
                        swipe: None,
                        swipe_direction: None,
                    });
                }
                if ui
                    .add(
                        egui::Slider::new(&mut app.temp_slider, 20.0..=85.0)
                            .text("C")
                            .fixed_decimals(1),
                    )
                    .changed()
                {
                    app.send_command(HostCommand::SetSensor {
                        doa_angle: None,
                        mic_rms: None,
                        als_lux: None,
                        temp_c: Some(app.temp_slider),
                        tof_mm: None,
                        swipe: None,
                        swipe_direction: None,
                    });
                }

                egui::ComboBox::from_id_salt("gnome_led_pattern_combo")
                    .selected_text(if app.selected_led_pattern.is_empty() {
                        "LED: Авто (Quasar)"
                    } else {
                        &app.selected_led_pattern
                    })
                    .width(185.0)
                    .show_ui(ui, |ui| {
                        for &(val, label) in LED_PATTERNS {
                            if ui
                                .selectable_label(app.selected_led_pattern == val, label)
                                .clicked()
                            {
                                app.selected_led_pattern = val.to_string();
                                app.send_command(HostCommand::SetLedPattern {
                                    pattern: app.selected_led_pattern.clone(),
                                });
                            }
                        }
                    });
            },
        );
    });
}
