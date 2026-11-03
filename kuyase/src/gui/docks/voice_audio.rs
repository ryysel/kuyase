use super::{
    compute_hijack_window_remaining_ms, gnome_action_row, gnome_preferences_group,
    render_mini_oscilloscope,
};
use crate::gui::state::GuiAppState;
use eframe::egui::{self, Color32, RichText};
use protocol::HostCommand;
use std::time::{Duration, Instant};

pub fn render_tts_audio_voice_card(ui: &mut egui::Ui, app: &mut GuiAppState) {
    let (spk_rms, guest_cap_bytes, last_spoken, hijacks, says) =
        if let Some(ref st) = app.latest_state {
            (
                st.speaker_rms.max(app.audio_player.live_rms()),
                st.captured_audio_bytes.max(app.audio_player.captured_bytes_len()),
                st.last_spoken_text.clone(),
                st.aliced_hijack_count,
                st.aliced_say_count,
            )
        } else {
            (
                app.audio_player.live_rms(),
                app.audio_player.captured_bytes_len(),
                String::new(),
                0,
                0,
            )
        };

    gnome_preferences_group(
        ui,
        "1. Кастомный TTS, Прослушивание звука и Голосовая команда",
        "Синтез произвольной речи на колонке (:9875), живая осциллограмма потока 48 кГц и отправка фраз в микрофоны (:9850)",
        |ui| {
            let tts_sub = if last_spoken.is_empty() {
                format!(
                    "START_VOCALIZE (18) в aliced (:9875) -> поток 48 кГц на динамики ПК | Озвучено: {}",
                    says
                )
            } else {
                format!("Последняя фраза: \"{}\" | Озвучено: {}", last_spoken, says)
            };
            gnome_action_row(
                ui,
                "Озвучить кастомный текст (TTS)",
                &tts_sub,
                false,
                |ui| {
                    let text_w = (ui.available_width() * 0.42).clamp(170.0, 320.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut app.ipc_vocalize_text)
                            .desired_width(text_w)
                            .hint_text("Введите текст для озвучки..."),
                    );
                    let tts_btn = egui::Button::new(
                        RichText::new("Озвучить и Прослушать")
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .fill(Color32::from_rgb(53, 132, 228));
                    if ui.add(tts_btn).clicked() {
                        if app.config.audio_muted {
                            app.config.audio_muted = false;
                            app.audio_player.set_muted(false);
                            let _ = app.save_config();
                        }
                        let text = app.ipc_vocalize_text.clone();
                        app.send_command(HostCommand::IpcTest {
                            cmd: "vocalize".to_string(),
                            text: Some(text),
                        });
                    }
                    if ui.button("Стоп TTS").clicked() {
                        app.send_command(HostCommand::IpcTest {
                            cmd: "stop_tts".to_string(),
                            text: None,
                        });
                    }
                    for (label, phrase) in [
                        (
                            "Погода",
                            "Привет! Сегодня в Москве плюс 21 градус, без осадков.",
                        ),
                        (
                            "Панграмма",
                            "Съешь этих мягких французских булок да выпей чаю",
                        ),
                    ] {
                        if ui.button(label).on_hover_text(phrase).clicked() {
                            app.ipc_vocalize_text = phrase.to_string();
                        }
                    }
                },
            );

            let audio_sub = format!(
                "Поток S16_LE 2 канала | RMS: {} | Захвачено: {:.1} КБ ({:.1} с)",
                spk_rms,
                guest_cap_bytes as f64 / 1024.0,
                guest_cap_bytes as f64 / (48_000.0 * 4.0)
            );
            gnome_action_row(
                ui,
                "Звуковой выход колонки (snd-aloop 48 кГц)",
                &audio_sub,
                false,
                |ui| {
                    render_mini_oscilloscope(ui, app);
                    if ui.button("Тон 440 Гц").clicked() {
                        app.send_command(HostCommand::PlayTestTone);
                    }
                    if ui.button("Очистить").clicked() {
                        app.send_command(HostCommand::ClearAudio);
                        app.audio_player.clear_waveform();
                        if let Some(ref b) = app.bridge {
                            b.clear_captured_pcm();
                        }
                    }
                    if ui.button("Сохранить .WAV").clicked() {
                        let board = app
                            .latest_state
                            .as_ref()
                            .map(|s| s.board_slug.clone())
                            .unwrap_or_else(|| app.config.selected_board.clone());
                        if app.audio_player.captured_bytes_len() == 0 {
                            if let Some(ref b) = app.bridge {
                                let fallback = b.captured_pcm_bytes();
                                if !fallback.is_empty() {
                                    app.audio_player.feed_pcm_chunk(&fallback);
                                }
                            }
                        }
                        let wav_path = app
                            .root_dir
                            .join("work")
                            .join(format!("{}-captured.wav", board));
                        match app.audio_player.export_wav(&wav_path) {
                            Ok(bytes) => {
                                app.status_message = format!(
                                    "Сохранён файл {} ({} байт)",
                                    wav_path.display(),
                                    bytes
                                );
                            }
                            Err(e) => {
                                app.status_message = format!("Ошибка сохранения WAV: {}", e);
                            }
                        }
                    }
                },
            );

            let rem_ms =
                compute_hijack_window_remaining_ms(app.last_voice_inject_at, Instant::now());
            if rem_ms > 0 {
                ui.ctx().request_repaint_after(Duration::from_millis(16));
            }
            let voice_sub = if rem_ms > 0 {
                format!(
                    "Окно перехвата активно: осталось {} мс | Перехватов: {}",
                    rem_ms, hijacks
                )
            } else {
                format!(
                    "Инжекция фразы в yio_audio (:9850) и окно перехвата ответа 1680 мс | Перехватов: {}",
                    hijacks
                )
            };
            gnome_action_row(
                ui,
                "Сказать фразу Алисе (Voice Injection)",
                &voice_sub,
                true,
                |ui| {
                    let voice_w = (ui.available_width() * 0.34).clamp(150.0, 260.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut app.voice_phrase)
                            .desired_width(voice_w)
                            .hint_text("Фраза для Алисы..."),
                    );
                    let send_btn = egui::Button::new(
                        RichText::new("Послать фразу")
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .fill(Color32::from_rgb(53, 132, 228));
                    if ui.add(send_btn).clicked() {
                        app.last_voice_inject_at = Some(Instant::now());
                        let cmd = HostCommand::InjectVoice {
                            phrase: app.voice_phrase.clone(),
                            doa_angle: Some(app.doa_slider),
                            rms: Some(app.voice_rms),
                            wake_only: app.voice_wake_only,
                        };
                        app.send_command(cmd);
                    }
                    let hijack_lbl = if rem_ms > 0 {
                        format!("Перехватить ({} мс)", rem_ms)
                    } else {
                        "Перехватить (1680 мс)".to_string()
                    };
                    if ui.button(hijack_lbl).clicked() {
                        app.send_command(HostCommand::IpcTest {
                            cmd: "cancel_dialog".to_string(),
                            text: None,
                        });
                    }
                    for preset in ["включи ночной режим", "громкость 3", "какая погода"] {
                        if ui.button(preset).clicked() {
                            app.voice_phrase = preset.to_string();
                        }
                    }
                },
            );
        },
    );
}
