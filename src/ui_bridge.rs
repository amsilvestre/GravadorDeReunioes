use arboard;
use crate::audio::capture::AudioCapture;
use crate::audio::mixer::Mixer;
use crate::audio::wav_writer::WavFileWriter;
use crate::config::AppConfig;
use crate::db::Database;
use crate::AppWindow;
use anyhow::Result;
use slint::ComponentHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub struct AppState {
    pub config: AppConfig,
    pub db: Database,
    pub current_recording_id: Option<i64>,
    pub current_recording_path: Option<std::path::PathBuf>,
    pub last_transcription_text: Option<String>,
}

pub fn setup(app: &AppWindow, db: Database, config: AppConfig) -> Result<()> {
    let state = Arc::new(Mutex::new(AppState {
        config: config.clone(),
        db,
        current_recording_id: None,
        current_recording_path: None,
        last_transcription_text: None,
    }));

    // Flag compartilhada para controle de gravacao
    let recording_flag = Arc::new(AtomicBool::new(false));

    // Carrega configuracoes na UI
    app.set_engine_index(config.engine);
    app.set_model_index(config.model_index);
    app.set_api_key(config.api_key.into());

    // === Callback: Iniciar gravacao ===
    let state_clone = state.clone();
    let app_weak = app.as_weak();
    let recording_flag_clone = recording_flag.clone();
    app.on_start_recording(move || {
        let state = state_clone.clone();
        let app_weak = app_weak.clone();
        let recording_flag = recording_flag_clone.clone();

        // Evita iniciar se ja esta gravando
        if recording_flag.load(Ordering::Relaxed) {
            return;
        }
        recording_flag.store(true, Ordering::Relaxed);

        std::thread::spawn(move || {
            // Gera nome do arquivo e registra no banco
            let (file_path, _recording_id) = {
                let mut s = state.lock().unwrap();
                let now = chrono::Local::now();
                let filename = format!("reuniao_{}.wav", now.format("%Y-%m-%d_%H-%M-%S"));
                let file_path = s.config.output_dir.join(&filename);
                s.current_recording_path = Some(file_path.clone());

                let created_at = now.to_rfc3339();
                let id = s.db.add_recording(&file_path.to_string_lossy(), &created_at).ok();
                s.current_recording_id = id;
                (file_path, id)
            };

            // Atualiza UI: gravando
            let _ = app_weak.upgrade_in_event_loop(|app: AppWindow| {
                app.set_is_recording(true);
                app.set_recording_status("Iniciando captura de audio...".into());
                app.set_has_recording(false);
            });

            // Inicia captura de audio
            let capture_result = AudioCapture::start();
            let (mut capture, handles) = match capture_result {
                Ok(result) => result,
                Err(e) => {
                    eprintln!("Erro ao iniciar captura de audio: {}", e);
                    recording_flag.store(false, Ordering::Relaxed);
                    let err_msg: slint::SharedString = format!("Erro: {}", e).into();
                    let _ = app_weak.upgrade_in_event_loop(move |app: AppWindow| {
                        app.set_is_recording(false);
                        app.set_recording_status(err_msg);
                    });
                    return;
                }
            };

            // Configura mixer — usa sample rate do loopback como referencia
            let target_sr = handles.loopback_config.sample_rate;
            let target_ch: u16 = 1; // mono para transcricao
            let mut mixer = Mixer::new(handles, target_sr, target_ch);

            // Cria WAV writer
            let mut wav_writer = match WavFileWriter::new(&file_path, target_sr, target_ch) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("Erro ao criar arquivo WAV: {}", e);
                    capture.stop();
                    recording_flag.store(false, Ordering::Relaxed);
                    return;
                }
            };

            let _ = app_weak.upgrade_in_event_loop(|app: AppWindow| {
                app.set_recording_status("Gravando...".into());
            });

            // Loop de gravacao
            let start = std::time::Instant::now();
            while recording_flag.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(100));

                // Le e mixa audio
                let output = mixer.read_and_mix();

                // Escreve no WAV
                if !output.samples.is_empty() {
                    if let Err(e) = wav_writer.write_samples(&output.samples) {
                        eprintln!("Erro ao escrever WAV: {}", e);
                        break;
                    }
                }

                // Atualiza UI a cada ~100ms
                let elapsed = start.elapsed();
                let secs = elapsed.as_secs();
                let h = secs / 3600;
                let m = (secs % 3600) / 60;
                let s = secs % 60;
                let duration_str: slint::SharedString =
                    format!("{:02}:{:02}:{:02}", h, m, s).into();
                let level = output.rms_level.min(1.0);

                let app_weak = app_weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = app_weak.upgrade() {
                        app.set_recording_duration(duration_str);
                        app.set_audio_level(level);
                    }
                });
            }

            // Finaliza gravacao
            capture.stop();
            let duration_secs = start.elapsed().as_secs() as i64;

            match wav_writer.finalize() {
                Ok(path) => {
                    eprintln!("Gravacao salva em: {:?}", path);
                    // Atualiza duracao no banco
                    let s = state.lock().unwrap();
                    if let Some(id) = s.current_recording_id {
                        let _ = s.db.update_recording_duration(id, duration_secs);
                    }
                }
                Err(e) => eprintln!("Erro ao finalizar WAV: {}", e),
            }

            let _ = app_weak.upgrade_in_event_loop(|app: AppWindow| {
                app.set_is_recording(false);
                app.set_recording_status("Gravacao finalizada".into());
                app.set_has_recording(true);
                app.set_audio_level(0.0);
            });
        });
    });

    // === Callback: Parar gravacao ===
    let recording_flag_clone = recording_flag.clone();
    app.on_stop_recording(move || {
        recording_flag_clone.store(false, Ordering::Relaxed);
    });

    // === Callback: Transcrever ===
    let state_clone = state.clone();
    let app_weak = app.as_weak();
    app.on_transcribe(move || {
        let state = state_clone.clone();
        let app_weak = app_weak.clone();

        std::thread::spawn(move || {
            let (wav_path, engine_type, api_key, models_dir, model_name) = {
                let s = state.lock().unwrap();
                let wav_path = match &s.current_recording_path {
                    Some(p) => p.clone(),
                    None => {
                        eprintln!("Nenhuma gravacao para transcrever");
                        return;
                    }
                };
                let engine = s.config.engine;
                let api_key = s.config.api_key.clone();
                let model_name = s.config.model_name().to_string();
                let models_dir = s.config.models_dir.clone();
                (wav_path, engine, api_key, models_dir, model_name)
            };

            // Atualiza UI: transcrevendo
            {
                let w = app_weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = w.upgrade() {
                        app.set_is_transcribing(true);
                        app.set_transcription_progress(0.0);
                        app.set_transcription_status("Iniciando...".into());
                    }
                });
            }

            // Seleciona engine e transcreve
            use crate::transcription::TranscriptionEngine;
            let result = if engine_type == 0 {
                // Cloud (OpenAI)
                if api_key.is_empty() {
                    Err(anyhow::anyhow!("Chave API OpenAI nao configurada. Va em Configuracoes e insira sua API key."))
                } else {
                    {
                        let w = app_weak.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = w.upgrade() {
                                app.set_transcription_status("Enviando para OpenAI...".into());
                            }
                        });
                    }
                    let progress_weak = app_weak.clone();
                    let on_progress: Box<dyn Fn(f32) + Send> = Box::new(move |p: f32| {
                        let status = format!("Transcrevendo... {}%", (p * 100.0) as u32);
                        let s: slint::SharedString = status.into();
                        let w = progress_weak.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = w.upgrade() {
                                app.set_transcription_progress(p);
                                app.set_transcription_status(s);
                            }
                        });
                    });
                    let engine = crate::transcription::cloud::CloudEngine::new(api_key);
                    engine.transcribe(&wav_path, on_progress)
                }
            } else {
                // Local (whisper-rs) — verifica/baixa modelo primeiro
                let model_path = {
                    let progress_weak = app_weak.clone();
                    let model_name_dl = model_name.clone();
                    let on_dl_progress = move |p: f32| {
                        let pct = (p * 100.0) as u32;
                        let status = format!("Baixando modelo ggml-{}.bin... {}%", model_name_dl, pct);
                        let s: slint::SharedString = status.into();
                        let w = progress_weak.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = w.upgrade() {
                                app.set_transcription_progress(p * 0.5); // download = 0..50%
                                app.set_transcription_status(s);
                            }
                        });
                    };
                    crate::transcription::model_downloader::ensure_model(
                        &models_dir,
                        &model_name,
                        &on_dl_progress,
                    )
                };

                match model_path {
                    Err(e) => Err(e),
                    Ok(path) => {
                        {
                            let w = app_weak.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(app) = w.upgrade() {
                                    app.set_transcription_progress(0.5);
                                    app.set_transcription_status("Carregando modelo...".into());
                                }
                            });
                        }
                        let progress_weak = app_weak.clone();
                        let on_progress: Box<dyn Fn(f32) + Send> = Box::new(move |p: f32| {
                            // transcricao = 50..100%
                            let overall = 0.5 + p * 0.5;
                            let status = format!("Transcrevendo... {}%", (overall * 100.0) as u32);
                            let s: slint::SharedString = status.into();
                            let w = progress_weak.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(app) = w.upgrade() {
                                    app.set_transcription_progress(overall);
                                    app.set_transcription_status(s);
                                }
                            });
                        });
                        let engine = crate::transcription::local::LocalEngine::new(path);
                        engine.transcribe(&wav_path, on_progress)
                    }
                }
            };

            match result {
                Ok(segments) => {
                    // Salva no banco e armazena texto para copy/export
                    let full_text: String = segments
                        .iter()
                        .map(|seg| seg.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" ");
                    {
                        let mut s = state.lock().unwrap();
                        if let Some(id) = s.current_recording_id {
                            let _ = s.db.update_recording_transcription(id, "done", Some(&full_text));
                        }
                        s.last_transcription_text = Some(full_text.clone());
                    }

                    // Converte para modelo Slint (Rc precisa ser criado na UI thread)
                    let slint_segments: Vec<(String, String)> = segments
                        .iter()
                        .map(|seg| {
                            let secs = seg.start_ms / 1000;
                            let mins = secs / 60;
                            let timestamp = format!("{:02}:{:02}", mins, secs % 60);
                            (timestamp, seg.text.clone())
                        })
                        .collect();

                    let _ = app_weak.upgrade_in_event_loop(move |app: AppWindow| {
                        let model_data: Vec<crate::TranscriptionSegment> = slint_segments
                            .iter()
                            .map(|(ts, text)| crate::TranscriptionSegment {
                                timestamp: ts.clone().into(),
                                text: text.clone().into(),
                            })
                            .collect();
                        let model = std::rc::Rc::new(slint::VecModel::from(model_data));
                        app.set_is_transcribing(false);
                        app.set_transcription_progress(1.0);
                        app.set_transcription_status("Transcricao concluida".into());
                        app.set_transcription_segments(model.into());
                    });
                }
                Err(e) => {
                    let err_msg: slint::SharedString = format!("Erro: {}", e).into();
                    let _ = app_weak.upgrade_in_event_loop(move |app: AppWindow| {
                        app.set_is_transcribing(false);
                        app.set_transcription_status(err_msg);
                    });
                }
            }
        });
    });

    // === Callback: Copiar para clipboard ===
    let state_clone = state.clone();
    let app_weak = app.as_weak();
    app.on_copy_to_clipboard(move || {
        let text = state_clone.lock().unwrap().last_transcription_text.clone();
        let app_weak = app_weak.clone();
        if let Some(text) = text {
            match arboard::Clipboard::new() {
                Ok(mut clipboard) => {
                    if clipboard.set_text(&text).is_ok() {
                        let _ = app_weak.upgrade_in_event_loop(|app: AppWindow| {
                            app.set_transcription_status("Texto copiado para o clipboard!".into());
                        });
                    }
                }
                Err(e) => eprintln!("Erro ao acessar clipboard: {}", e),
            }
        }
    });

    // === Callback: Exportar texto ===
    let state_clone = state.clone();
    let app_weak = app.as_weak();
    app.on_export_text(move || {
        let (text, wav_path) = {
            let s = state_clone.lock().unwrap();
            (s.last_transcription_text.clone(), s.current_recording_path.clone())
        };
        let app_weak = app_weak.clone();
        if let (Some(text), Some(wav_path)) = (text, wav_path) {
            let txt_path = wav_path.with_extension("txt");
            match std::fs::write(&txt_path, &text) {
                Ok(_) => {
                    let msg = format!("Exportado: {}", txt_path.display());
                    let _ = app_weak.upgrade_in_event_loop(move |app: AppWindow| {
                        app.set_transcription_status(msg.into());
                    });
                }
                Err(e) => {
                    let msg = format!("Erro ao exportar: {}", e);
                    let _ = app_weak.upgrade_in_event_loop(move |app: AppWindow| {
                        app.set_transcription_status(msg.into());
                    });
                }
            }
        }
    });

    // === Callback: Salvar configuracoes ===
    let state_clone = state.clone();
    let app_weak = app.as_weak();
    app.on_save_settings(move || {
        if let Some(app) = app_weak.upgrade() {
            let mut s = state_clone.lock().unwrap();
            s.config.engine = app.get_engine_index();
            s.config.model_index = app.get_model_index();
            s.config.api_key = app.get_api_key().to_string();

            if let Err(e) = s.config.save(&s.db) {
                eprintln!("Erro ao salvar configuracoes: {}", e);
            }
        }
    });

    // Callbacks de settings (salvos ao clicar "Salvar")
    app.on_api_key_changed(move |_key| {});
    app.on_engine_changed(move |_idx| {});
    app.on_model_changed(move |_idx| {});

    // === Callback: Carregar historico ===
    let state_clone = state.clone();
    let app_weak = app.as_weak();
    app.on_load_recordings(move || {
        let rows = {
            let s = state_clone.lock().unwrap();
            s.db.get_all_recordings().unwrap_or_default()
        };

        let items: Vec<crate::RecordingItem> = rows
            .into_iter()
            .map(|row| {
                let name = std::path::Path::new(&row.file_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&row.file_path)
                    .to_string();

                // Formata data: ISO8601 -> "DD/MM/YYYY HH:MM"
                let date = chrono::DateTime::parse_from_rfc3339(&row.created_at)
                    .map(|dt| dt.format("%d/%m/%Y %H:%M").to_string())
                    .unwrap_or(row.created_at.clone());

                // Formata duracao: segundos -> "MM:SS"
                let mins = row.duration_secs / 60;
                let secs = row.duration_secs % 60;
                let duration = format!("{:02}:{:02}", mins, secs);

                crate::RecordingItem {
                    id: row.id as i32,
                    name: name.into(),
                    date: date.into(),
                    duration: duration.into(),
                    status: row.transcription_status.into(),
                }
            })
            .collect();

        let _ = app_weak.upgrade_in_event_loop(move |app: AppWindow| {
            let model = std::rc::Rc::new(slint::VecModel::from(items));
            app.set_recordings(model.into());
        });
    });

    // === Callback: Selecionar gravacao do historico ===
    let state_clone = state.clone();
    let app_weak = app.as_weak();
    app.on_select_recording(move |id| {
        // Busca dados da gravacao no banco
        let rows = {
            let s = state_clone.lock().unwrap();
            s.db.get_all_recordings().unwrap_or_default()
        };
        let row = rows.into_iter().find(|r| r.id == id as i64);

        if let Some(row) = row {
            let has_transcription = row.transcription_status == "done"
                && row.transcription_text.is_some();
            let transcription_text = row.transcription_text.clone();

            {
                let mut s = state_clone.lock().unwrap();
                s.current_recording_id = Some(row.id);
                s.current_recording_path = Some(std::path::PathBuf::from(&row.file_path));
                s.last_transcription_text = transcription_text.clone();
            }

            // Converte texto salvo em segmentos para exibir na UI
            let slint_segments: Vec<(String, String)> = if let Some(text) = &transcription_text {
                text.split(|c: char| c == '.' || c == '!' || c == '?')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .map(|s| ("--:--".to_string(), s))
                    .collect()
            } else {
                vec![]
            };

            let status_msg = if has_transcription {
                "Transcricao anterior carregada".to_string()
            } else {
                String::new()
            };

            let _ = app_weak.upgrade_in_event_loop(move |app: AppWindow| {
                app.set_has_recording(true);
                app.set_active_page(0); // Navega para aba Gravar/Transcrever

                if !slint_segments.is_empty() {
                    let model_data: Vec<crate::TranscriptionSegment> = slint_segments
                        .iter()
                        .map(|(ts, text)| crate::TranscriptionSegment {
                            timestamp: ts.clone().into(),
                            text: text.clone().into(),
                        })
                        .collect();
                    let model = std::rc::Rc::new(slint::VecModel::from(model_data));
                    app.set_transcription_segments(model.into());
                } else {
                    let empty: Vec<crate::TranscriptionSegment> = vec![];
                    let model = std::rc::Rc::new(slint::VecModel::from(empty));
                    app.set_transcription_segments(model.into());
                }

                app.set_transcription_status(status_msg.into());
                app.set_transcription_progress(0.0);
            });
        }
    });

    Ok(())
}
