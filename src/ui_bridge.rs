use crate::audio::capture::AudioCapture;
use crate::audio::mixer::Mixer;
use crate::audio::wav_writer::WavFileWriter;
use crate::config::AppConfig;
use crate::db::Database;
use crate::AppStyle;
use crate::AppWindow;
use anyhow::Result;
use arboard;
use slint::ComponentHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

fn send_notification(title: &str, message: &str) {
    if let Ok(notification) = winrt_notification::Toast::new("AMS Gravador")
        .title(title)
        .text1(message)
        .show()
    {
        let _ = notification;
    }
}

fn format_duration(duration: std::time::Duration) -> slint::SharedString {
    let secs = duration.as_secs();
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s).into()
}

fn apply_transcription_search(app: &AppWindow, state: &Arc<Mutex<AppState>>) {
    let query = app.get_transcription_search().to_string().to_lowercase();
    let segments = {
        let s = state.lock().unwrap();
        s.last_transcription_segments.clone()
    };

    let filtered: Vec<crate::TranscriptionSegment> = if query.is_empty() {
        segments
    } else {
        segments
            .into_iter()
            .filter(|seg| seg.text.to_lowercase().contains(&query))
            .collect()
    };

    let model = std::rc::Rc::new(slint::VecModel::from(filtered));
    app.set_transcription_segments(model.into());
}

fn detect_gpu_available() -> bool {
    let cuda_dlls = [
        "nvcuda.dll",
        "cudart64_80.dll",
        "cudart64_110.dll",
        "cudart64_120.dll",
        "cudart64_132.dll",
    ];

    for dll in cuda_dlls {
        if std::path::Path::new("C:\\Windows\\System32")
            .join(dll)
            .exists()
        {
            return true;
        }
    }

    if std::path::Path::new("C:\\Program Files\\NVIDIA GPU Computing Toolkit\\CUDA\\v13.2").exists()
    {
        return true;
    }

    std::process::Command::new("nvidia-smi")
        .arg("-L")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn compare_versions(new: &str, current: &str) -> i32 {
    let new_parts: Vec<u32> = new.split('.').filter_map(|s| s.parse().ok()).collect();
    let current_parts: Vec<u32> = current.split('.').filter_map(|s| s.parse().ok()).collect();

    for i in 0..std::cmp::max(new_parts.len(), current_parts.len()) {
        let new_part = new_parts.get(i).unwrap_or(&0);
        let current_part = current_parts.get(i).unwrap_or(&0);

        if new_part > current_part {
            return 1;
        } else if new_part < current_part {
            return -1;
        }
    }
    0
}

pub struct AppState {
    pub config: AppConfig,
    pub db: Database,
    pub current_recording_id: Option<i64>,
    pub current_recording_path: Option<std::path::PathBuf>,
    pub last_transcription_text: Option<String>,
    pub last_transcription_segments: Vec<crate::TranscriptionSegment>,
    pub update_url: String,
}

pub fn setup(app: &AppWindow, db: Database, config: AppConfig) -> Result<()> {
    let state = Arc::new(Mutex::new(AppState {
        config: config.clone(),
        db,
        current_recording_id: None,
        current_recording_path: None,
        last_transcription_text: None,
        last_transcription_segments: Vec::new(),
        update_url: String::new(),
    }));

    // Flag compartilhada para controle de gravacao
    let recording_flag = Arc::new(AtomicBool::new(false));
    let paused_flag = Arc::new(AtomicBool::new(false));

    // Clones para uso nos callbacks
    let paused_flag_for_start = paused_flag.clone();
    let paused_flag_for_pause = paused_flag.clone();
    let paused_flag_for_resume = paused_flag.clone();

    // Carrega configuracoes na UI
    let gpu_available = detect_gpu_available();
    app.set_engine_index(config.engine);
    app.set_theme_index(config.theme_index);
    app.set_model_index(config.model_index);
    app.set_language_index(config.language_index);
    app.set_hardware_index(if gpu_available {
        config.hardware_index
    } else {
        0
    });
    app.set_input_device_index(config.input_device_index);
    app.set_output_device_index(config.output_device_index);
    app.set_api_key(config.api_key.into());
    app.set_output_dir(config.output_dir.to_string_lossy().to_string().into());
    app.set_gpu_available(gpu_available);
    app.set_app_version(env!("CARGO_PKG_VERSION").into());
    app.global::<AppStyle>()
        .set_dark_theme(config.theme_index == 1);

    // Carrega lista de dispositivos
    let input_devices: Vec<slint::SharedString> = crate::audio::capture::list_input_devices()
        .iter()
        .map(|s| s.as_str().into())
        .collect();
    let output_devices: Vec<slint::SharedString> = crate::audio::capture::list_output_devices()
        .iter()
        .map(|s| s.as_str().into())
        .collect();

    let input_model = std::rc::Rc::new(slint::VecModel::from(input_devices));
    let output_model = std::rc::Rc::new(slint::VecModel::from(output_devices));
    app.set_input_devices(input_model.into());
    app.set_output_devices(output_model.into());

    // === Callback: Iniciar gravacao ===
    let state_clone = state.clone();
    let app_weak = app.as_weak();
    let recording_flag_clone = recording_flag.clone();
    app.on_start_recording(move || {
        let state = state_clone.clone();
        let app_weak = app_weak.clone();
        let recording_flag = recording_flag_clone.clone();
        let paused_flag = paused_flag_for_start.clone();
        let delay_index = if let Some(app) = app_weak.upgrade() {
            app.get_recording_delay_index()
        } else {
            0
        };
        let delay_secs = match delay_index {
            1 => 10,
            2 => 30,
            3 => 60,
            _ => 0,
        };

        // Evita iniciar se ja esta gravando
        if recording_flag.load(Ordering::Relaxed) {
            return;
        }
        recording_flag.store(true, Ordering::Relaxed);
        paused_flag.store(false, Ordering::Relaxed);

        std::thread::spawn(move || {
            // Gera nome do arquivo e registra no banco
            let (file_path, _recording_id) = {
                let mut s = state.lock().unwrap();
                let now = chrono::Local::now();
                let filename = format!("reuniao_{}.wav", now.format("%Y-%m-%d_%H-%M-%S"));
                let file_path = s.config.output_dir.join(&filename);
                s.current_recording_path = Some(file_path.clone());

                let created_at = now.to_rfc3339();
                let id =
                    s.db.add_recording(&file_path.to_string_lossy(), &created_at)
                        .ok();
                s.current_recording_id = id;
                (file_path, id)
            };

            // Atualiza UI: gravando
            let _ = app_weak.upgrade_in_event_loop(|app: AppWindow| {
                app.set_is_recording(true);
                app.set_is_paused(false);
                app.set_recording_duration("00:00:00".into());
                app.set_recording_status("Preparando gravação...".into());
                app.set_has_recording(false);
            });

            if delay_secs > 0 {
                for remaining in (1..=delay_secs).rev() {
                    if !recording_flag.load(Ordering::Relaxed) {
                        let _ = app_weak.upgrade_in_event_loop(|app: AppWindow| {
                            app.set_is_recording(false);
                            app.set_recording_status("Gravação cancelada".into());
                        });
                        return;
                    }

                    let status: slint::SharedString =
                        format!("Iniciando gravação em {}s...", remaining).into();
                    let _ = app_weak.upgrade_in_event_loop(move |app: AppWindow| {
                        app.set_recording_status(status);
                    });
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            }

            // Inicia captura de audio
            let input_idx = {
                let s = state.lock().unwrap();
                s.config.input_device_index as usize
            };
            let output_idx = {
                let s = state.lock().unwrap();
                s.config.output_device_index as usize
            };

            let capture_result = if input_idx > 0 || output_idx > 0 {
                AudioCapture::start_with_devices(Some(input_idx), Some(output_idx))
            } else {
                AudioCapture::start()
            };

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

            // Configura mixer — 16kHz mono (mesma qualidade do Whisper)
            let target_sr: u32 = 16000;
            let target_ch: u16 = 1;
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
            let paused = paused_flag.clone();
            let mut paused_total = std::time::Duration::from_secs(0);
            let mut pause_started: Option<std::time::Instant> = None;
            while recording_flag.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(100));

                let is_paused = paused.load(Ordering::Relaxed);
                if is_paused {
                    if pause_started.is_none() {
                        pause_started = Some(std::time::Instant::now());
                    }
                } else if let Some(ps) = pause_started.take() {
                    paused_total += ps.elapsed();
                }

                // Le e mixa audio
                let output = mixer.read_and_mix();

                // Escreve no WAV apenas se nao estiver pausado
                if !paused.load(Ordering::Relaxed) && !output.samples.is_empty() {
                    if let Err(e) = wav_writer.write_samples(&output.samples) {
                        eprintln!("Erro ao escrever WAV: {}", e);
                        break;
                    }
                }

                // Atualiza UI a cada ~100ms
                let mut elapsed = start.elapsed();
                if let Some(ps) = pause_started {
                    elapsed = elapsed.saturating_sub(paused_total + ps.elapsed());
                } else {
                    elapsed = elapsed.saturating_sub(paused_total);
                }
                let duration_str = format_duration(elapsed);
                let level = if is_paused {
                    0.0
                } else {
                    output.rms_level.min(1.0)
                };

                let app_weak = app_weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = app_weak.upgrade() {
                        app.set_recording_duration(duration_str);
                        app.set_audio_level(level);
                        app.set_is_paused(is_paused);
                    }
                });
            }

            // Finaliza gravacao
            capture.stop();
            let mut final_elapsed = start.elapsed();
            if let Some(ps) = pause_started {
                final_elapsed = final_elapsed.saturating_sub(paused_total + ps.elapsed());
            } else {
                final_elapsed = final_elapsed.saturating_sub(paused_total);
            }
            let duration_secs = final_elapsed.as_secs() as i64;

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

            // Notificacao
            send_notification("Gravação Finalizada", "A gravação foi salva com sucesso.");
        });
    });

    // === Callback: Parar gravacao ===
    let recording_flag_clone = recording_flag.clone();
    app.on_stop_recording(move || {
        recording_flag_clone.store(false, Ordering::Relaxed);
    });

    // === Callback: Pausar gravacao ===
    let paused_flag_pause = paused_flag_for_pause.clone();
    let app_weak_pause = app.as_weak();
    app.on_pause_recording(move || {
        paused_flag_pause.store(true, Ordering::Relaxed);
        let _ = app_weak_pause.upgrade_in_event_loop(|app: AppWindow| {
            app.set_is_paused(true);
            app.set_recording_status("Gravação pausada".into());
        });
    });

    // === Callback: Retomar gravacao ===
    let paused_flag_resume = paused_flag_for_resume.clone();
    let app_weak_resume = app.as_weak();
    app.on_resume_recording(move || {
        paused_flag_resume.store(false, Ordering::Relaxed);
        let _ = app_weak_resume.upgrade_in_event_loop(|app: AppWindow| {
            app.set_is_paused(false);
            app.set_recording_status("Gravando...".into());
        });
    });

    // === Callback: Transcrever ===
    let state_clone = state.clone();
    let app_weak = app.as_weak();
    app.on_transcribe(move || {
        let state = state_clone.clone();
        let app_weak = app_weak.clone();

        std::thread::spawn(move || {
            let (wav_path, engine_type, api_key, models_dir, model_name, language_code, use_gpu) = {
                let s = state.lock().unwrap();
                let wav_path = match &s.current_recording_path {
                    Some(p) => p.clone(),
                    None => {
                        let _ = app_weak.upgrade_in_event_loop(|app: AppWindow| {
                            app.set_transcription_status("Nenhuma gravação para transcrever".into());
                        });
                        return;
                    }
                };
                
                if !wav_path.exists() {
                    let _ = app_weak.upgrade_in_event_loop(|app: AppWindow| {
                        app.set_transcription_status("Arquivo de áudio não encontrado".into());
                    });
                    return;
                }
                
                if let Some(ext) = wav_path.extension() {
                    let ext = ext.to_string_lossy().to_lowercase();
                    if ext != "wav" && ext != "mp3" && ext != "m4a" && ext != "ogg" && ext != "flac" {
                        let _ = app_weak.upgrade_in_event_loop(|app: AppWindow| {
                            app.set_transcription_status("Arquivo inválido. Use arquivos de áudio (WAV, MP3, etc)".into());
                        });
                        return;
                    }
                }
                
                let engine = s.config.engine;
                let api_key = s.config.api_key.clone();
                let model_name = s.config.model_name().to_string();
                let models_dir = s.config.models_dir.clone();
                let language_code = s.config.language_code();
                let use_gpu = s.config.hardware_index == 1;
                (wav_path, engine, api_key, models_dir, model_name, language_code, use_gpu)
            };

            // Flag para parar o timer quando transcricao terminar
            let timer_stop_flag = Arc::new(AtomicBool::new(false));
            let timer_stop_flag_clone = timer_stop_flag.clone();

            // Atualiza UI: transcrevendo
            {
                let w = app_weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = w.upgrade() {
                        app.set_is_transcribing(true);
                        app.set_transcription_progress(0.0);
                        app.set_transcription_status("Iniciando...".into());
                        app.set_transcription_duration("Iniciando...".into());
                    }
                });
            }

            // Inicia thread do timer
            let timer_app_weak = app_weak.clone();
            std::thread::spawn(move || {
                let start = std::time::Instant::now();
                while !timer_stop_flag_clone.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    if timer_stop_flag_clone.load(Ordering::Relaxed) {
                        break;
                    }
                    let elapsed = start.elapsed();
                    let duration_text = format!(
                        "{:02}:{:02}:{:02}",
                        elapsed.as_secs() / 3600,
                        (elapsed.as_secs() % 3600) / 60,
                        elapsed.as_secs() % 60
                    );
                    let w = timer_app_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = w.upgrade() {
                            app.set_transcription_duration(duration_text.into());
                        }
                    });
                }
            });

            // Seleciona engine e transcreve
            use crate::transcription::TranscriptionEngine;
            let transcription_start = std::time::Instant::now();
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
                    engine.transcribe(&wav_path, language_code.as_deref(), on_progress)
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
                        let engine = crate::transcription::local::LocalEngine::new(path, use_gpu);
                        engine.transcribe(&wav_path, language_code.as_deref(), on_progress)
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
                    let model_segments: Vec<crate::TranscriptionSegment> = segments
                        .iter()
                        .map(|seg| {
                            let secs = seg.start_ms / 1000;
                            let mins = secs / 60;
                            let timestamp = format!("{:02}:{:02}", mins, secs % 60);
                            crate::TranscriptionSegment {
                                timestamp: timestamp.into(),
                                text: seg.text.clone().into(),
                            }
                        })
                        .collect();
                    {
                        let mut s = state.lock().unwrap();
                        if let Some(id) = s.current_recording_id {
                            let _ = s.db.update_recording_transcription(id, "done", Some(&full_text));
                        }
                        s.last_transcription_text = Some(full_text.clone());
                        s.last_transcription_segments = model_segments.clone();
                    }

                    timer_stop_flag.store(true, Ordering::Relaxed);
                    let _ = app_weak.upgrade_in_event_loop(move |app: AppWindow| {
                        app.set_is_transcribing(false);
                        app.set_transcription_progress(1.0);
                        let elapsed = transcription_start.elapsed();
                        let duration_text = format!(
                            "Transcrição concluída em {:02}:{:02}:{:02}",
                            elapsed.as_secs() / 3600,
                            (elapsed.as_secs() % 3600) / 60,
                            elapsed.as_secs() % 60
                        );
                        app.set_transcription_status(duration_text.clone().into());
                        app.set_transcription_duration(duration_text.into());
                        app.set_transcription_segments(std::rc::Rc::new(slint::VecModel::from(model_segments.clone())).into());
                    });

                    let app_for_filter = app_weak.clone();
                    let state_for_filter = state.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = app_for_filter.upgrade() {
                            apply_transcription_search(&app, &state_for_filter);
                        }
                    });
                    
                    // Notificacao
                    send_notification("Transcrição Concluída", "A transcrição do áudio foi concluída.");
                }
                Err(e) => {
                    timer_stop_flag.store(true, Ordering::Relaxed);
                    let err_msg: slint::SharedString = format!("Erro: {}", e).into();
                    let err_msg_clone = err_msg.clone();
                    let _ = app_weak.upgrade_in_event_loop(move |app: AppWindow| {
                        app.set_is_transcribing(false);
                        app.set_transcription_status(err_msg_clone);
                    });
                    
                    // Notificacao de erro
                    let err_str = format!("Erro: {}", e);
                    send_notification("Erro na Transcrição", &err_str);
                }
            }
        });
    });

    // === Callback: Abrir arquivo de audio externo e transcrever ===
    let state_clone = state.clone();
    let app_weak = app.as_weak();
    app.on_open_audio_file(move || {
        let state = state_clone.clone();
        let app_weak = app_weak.clone();

        if let Some(file_path) = rfd::FileDialog::new()
            .add_filter("Audio", &["wav", "mp3", "m4a", "ogg", "flac"])
            .pick_file()
        {
            {
                let mut s = state.lock().unwrap();
                s.current_recording_path = Some(file_path.clone());
                s.current_recording_id = None;
            }

            let _ = app_weak.upgrade_in_event_loop(|app: AppWindow| {
                app.set_has_recording(true);
                app.set_transcription_segments(
                    std::rc::Rc::new(slint::VecModel::from(Vec::<crate::TranscriptionSegment>::new())).into(),
                );
            });

            let (wav_path, engine_type, api_key, models_dir, model_name, language_code, use_gpu) = {
                let s = state.lock().unwrap();
                let wav_path = file_path.clone();
                let engine = s.config.engine;
                let api_key = s.config.api_key.clone();
                let model_name = s.config.model_name().to_string();
                let models_dir = s.config.models_dir.clone();
                let language_code = s.config.language_code();
                let use_gpu = s.config.hardware_index == 1;
                (wav_path, engine, api_key, models_dir, model_name, language_code, use_gpu)
            };

            std::thread::spawn(move || {
                let timer_stop_flag = Arc::new(AtomicBool::new(false));
                let timer_stop_flag_clone = timer_stop_flag.clone();

                // Atualiza UI: transcrevendo
                {
                    let w = app_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = w.upgrade() {
                            app.set_is_transcribing(true);
                            app.set_transcription_progress(0.0);
                            app.set_transcription_status("Iniciando...".into());
                            app.set_transcription_duration("Iniciando...".into());
                        }
                    });
                }

                let transcription_start = std::time::Instant::now();
                let timer_app_weak = app_weak.clone();
                std::thread::spawn(move || {
                    while !timer_stop_flag_clone.load(Ordering::Relaxed) {
                        std::thread::sleep(std::time::Duration::from_secs(1));
                        if timer_stop_flag_clone.load(Ordering::Relaxed) {
                            break;
                        }
                        let elapsed = transcription_start.elapsed();
                        let duration_text = format!(
                            "{:02}:{:02}:{:02}",
                            elapsed.as_secs() / 3600,
                            (elapsed.as_secs() % 3600) / 60,
                            elapsed.as_secs() % 60
                        );
                        let w = timer_app_weak.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = w.upgrade() {
                                app.set_transcription_duration(duration_text.into());
                            }
                        });
                    }
                });

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

                use crate::transcription::TranscriptionEngine;
                let result = if engine_type == 0 {
                    if api_key.is_empty() {
                        Err(anyhow::anyhow!("Chave API OpenAI nao configurada. Va em Configuracoes e insira sua API key."))
                    } else {
                        let w = app_weak.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = w.upgrade() {
                                app.set_transcription_status("Enviando para OpenAI...".into());
                            }
                        });
                        let engine = crate::transcription::cloud::CloudEngine::new(api_key);
                        engine.transcribe(&wav_path, language_code.as_deref(), on_progress)
                    }
                } else {
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
                                    app.set_transcription_progress(p * 0.5);
                                    app.set_transcription_status(s);
                                }
                            });
                        };
                        crate::transcription::model_downloader::ensure_model(&models_dir, &model_name, &on_dl_progress)
                    };

                    match model_path {
                        Err(e) => Err(e),
                        Ok(path) => {
                            let w = app_weak.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(app) = w.upgrade() {
                                    app.set_transcription_progress(0.5);
                                    app.set_transcription_status("Carregando modelo...".into());
                                }
                            });
                            let progress_weak = app_weak.clone();
                            let on_progress: Box<dyn Fn(f32) + Send> = Box::new(move |p: f32| {
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
                            let engine = crate::transcription::local::LocalEngine::new(path, use_gpu);
                            engine.transcribe(&wav_path, language_code.as_deref(), on_progress)
                        }
                    }
                };

                match result {
                    Ok(segments) => {
                        timer_stop_flag.store(true, Ordering::Relaxed);
                        let full_text: String = segments
                            .iter()
                            .map(|seg| seg.text.as_str())
                            .collect::<Vec<_>>()
                            .join(" ");
                        let model_segments: Vec<crate::TranscriptionSegment> = segments
                            .iter()
                            .map(|seg| {
                                let secs = seg.start_ms / 1000;
                                let mins = secs / 60;
                                let timestamp = format!("{:02}:{:02}", mins, secs % 60);
                                crate::TranscriptionSegment {
                                    timestamp: timestamp.into(),
                                    text: seg.text.clone().into(),
                                }
                            })
                            .collect();
                        {
                            let mut s = state.lock().unwrap();
                            s.last_transcription_text = Some(full_text.clone());
                            s.last_transcription_segments = model_segments.clone();
                        }

                        let _ = app_weak.upgrade_in_event_loop(move |app: AppWindow| {
                            app.set_is_transcribing(false);
                            app.set_transcription_progress(1.0);
                            let elapsed = transcription_start.elapsed();
                            let duration_text = format!(
                                "Transcrição concluída em {:02}:{:02}:{:02}",
                                elapsed.as_secs() / 3600,
                                (elapsed.as_secs() % 3600) / 60,
                                elapsed.as_secs() % 60
                            );
                            app.set_transcription_status(duration_text.clone().into());
                            app.set_transcription_duration(duration_text.into());
                            app.set_transcription_segments(std::rc::Rc::new(slint::VecModel::from(model_segments.clone())).into());
                        });

                        let app_for_filter = app_weak.clone();
                        let state_for_filter = state.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(app) = app_for_filter.upgrade() {
                                apply_transcription_search(&app, &state_for_filter);
                            }
                        });

                        send_notification("Transcrição Concluída", "A transcrição do áudio foi concluída.");
                    }
                    Err(e) => {
                        timer_stop_flag.store(true, Ordering::Relaxed);
                        let err_msg: slint::SharedString = format!("Erro: {}", e).into();
                        let err_msg_clone = err_msg.clone();
                        let _ = app_weak.upgrade_in_event_loop(move |app: AppWindow| {
                            app.set_is_transcribing(false);
                            app.set_transcription_status(err_msg_clone);
                        });

                        let err_str = format!("Erro: {}", e);
                        send_notification("Erro na Transcrição", &err_str);
                    }
                }
            });
        }
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
            (
                s.last_transcription_text.clone(),
                s.current_recording_path.clone(),
            )
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
            s.config.theme_index = app.get_theme_index();
            s.config.model_index = app.get_model_index();
            s.config.language_index = app.get_language_index();
            s.config.hardware_index = app.get_hardware_index();
            s.config.input_device_index = app.get_input_device_index();
            s.config.output_device_index = app.get_output_device_index();
            s.config.api_key = app.get_api_key().to_string();
            s.config.output_dir = std::path::PathBuf::from(app.get_output_dir().to_string());

            if let Err(e) = s.config.save(&s.db) {
                eprintln!("Erro ao salvar configuracoes: {}", e);
            }
        }
    });

    // Callbacks de settings (salvos ao clicar "Salvar")
    app.on_api_key_changed(move |_key| {});
    app.on_engine_changed(move |_idx| {});
    let app_weak_theme = app.as_weak();
    app.on_theme_changed(move |idx| {
        if let Some(app) = app_weak_theme.upgrade() {
            app.global::<AppStyle>().set_dark_theme(idx == 1);
        }
    });
    app.on_model_changed(move |_idx| {});
    app.on_language_changed(move |_idx| {});
    app.on_hardware_changed(move |_idx| {});
    app.on_input_device_changed(move |_idx| {});
    app.on_output_device_changed(move |_idx| {});

    let state_clone = state.clone();
    let app_weak = app.as_weak();
    app.on_transcription_search_changed(move |_text| {
        if let Some(app) = app_weak.upgrade() {
            apply_transcription_search(&app, &state_clone);
        }
    });

    let state_clone = state.clone();
    let app_weak = app.as_weak();
    app.on_select_output_folder(move || {
        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
            let mut s = state_clone.lock().unwrap();
            s.config.output_dir = folder.clone();
            let _ = s.config.save(&s.db);
            let _ = app_weak.upgrade_in_event_loop(move |app: AppWindow| {
                app.set_output_dir(folder.to_string_lossy().to_string().into());
            });
        }
    });

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
                let name = row.display_name.clone().unwrap_or_else(|| {
                    std::path::Path::new(&row.file_path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&row.file_path)
                        .to_string()
                });

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
            let has_transcription =
                row.transcription_status == "done" && row.transcription_text.is_some();
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

    // === Callback: Excluir gravacao ===
    let state_clone = state.clone();
    let app_weak = app.as_weak();
    app.on_delete_recording(move |id| {
        let file_path = {
            let s = state_clone.lock().unwrap();
            s.db.delete_recording(id as i64).ok().flatten()
        };

        if let Some(wav_path) = file_path {
            let wav_path = std::path::PathBuf::from(&wav_path);
            let _ = std::fs::remove_file(&wav_path);
            let txt_path = wav_path.with_extension("txt");
            let _ = std::fs::remove_file(txt_path);
        }

        let _ = app_weak.upgrade_in_event_loop(|app: AppWindow| {
            app.invoke_load_recordings();
        });
    });

    // === Callback: Renomear gravacao ===
    let state_clone = state.clone();
    let app_weak = app.as_weak();
    app.on_rename_recording(move |id, name| {
        {
            let s = state_clone.lock().unwrap();
            let _ = s.db.rename_recording(id as i64, name.as_str());
        }
        let _ = app_weak.upgrade_in_event_loop(|app: AppWindow| {
            app.invoke_load_recordings();
        });
    });

    // === Callback: Verificar atualizacoes ===
    let app_weak_update = app.as_weak();
    let state_for_update = state.clone();
    app.on_check_for_updates(move || {
        let app_weak = app_weak_update.clone();
        let state = state_for_update.clone();
        std::thread::spawn(move || {
            let _ = app_weak.upgrade_in_event_loop(|app: AppWindow| {
                app.set_is_checking_update(true);
            });

            let current_version = env!("CARGO_PKG_VERSION");

            match reqwest::blocking::Client::new()
                .get("https://api.github.com/repos/amsilvestre/GravadorDeReunioes/releases/latest")
                .header("User-Agent", "AMS-Gravador-de-Reunioes")
                .header("Accept", "application/vnd.github.v3+json")
                .send()
            {
                Ok(response) => {
                    if let Ok(json) = response.json::<serde_json::Value>() {
                        let tag_name = json["tag_name"]
                            .as_str()
                            .unwrap_or("")
                            .trim_start_matches('v')
                            .to_string();
                        let body = json["body"].as_str().unwrap_or("").to_string();
                        let download_url = json["assets"]
                            .as_array()
                            .and_then(|assets| {
                                assets
                                    .iter()
                                    .find(|a| a["name"].as_str().unwrap_or("").contains("Setup"))
                            })
                            .and_then(|asset| asset["browser_download_url"].as_str())
                            .unwrap_or("")
                            .to_string();

                        let version_compare = compare_versions(&tag_name, current_version);
                        if version_compare > 0 {
                            let tag_name_owned = tag_name;
                            let body_owned = body;
                            let download_url_owned = download_url.clone();
                            {
                                let mut s = state.lock().unwrap();
                                s.update_url = download_url.clone();
                            }
                            let _ = app_weak.upgrade_in_event_loop(move |app: AppWindow| {
                                app.set_update_version(tag_name_owned.into());
                                app.set_update_body(body_owned.into());
                                app.set_update_url(download_url_owned.into());
                                app.set_show_update_dialog(true);
                                app.set_is_checking_update(false);
                            });
                        } else {
                            let _ = app_weak.upgrade_in_event_loop(|app: AppWindow| {
                                app.set_is_checking_update(false);
                            });
                        }
                    } else {
                        let _ = app_weak.upgrade_in_event_loop(|app: AppWindow| {
                            app.set_is_checking_update(false);
                        });
                    }
                }
                Err(_) => {
                    let _ = app_weak.upgrade_in_event_loop(|app: AppWindow| {
                        app.set_is_checking_update(false);
                    });
                }
            }
        });
    });

    // === Callback: Baixar e instalar atualizacao ===
    let app_weak_install = app.as_weak();
    let state_for_install = state.clone();
    app.on_download_and_install_update(move || {
        let state = state_for_install.clone();
        // Get the URL from the shared state set during check_for_updates
        let url = {
            let s = state.lock().unwrap();
            s.update_url.clone()
        };

        if !url.is_empty() {
            let app_weak = app_weak_install.clone();
            let url_copy = url.clone();
            std::thread::spawn(move || {
                let temp_dir = std::env::temp_dir();
                let installer_path = temp_dir.join("AMS_Gravador_Update_Setup.exe");

                match reqwest::blocking::Client::new().get(&url_copy).send() {
                    Ok(mut response) => {
                        if let Ok(mut file) = std::fs::File::create(&installer_path) {
                            if std::io::copy(&mut response, &mut file).is_ok() {
                                let _ = app_weak.upgrade_in_event_loop(|app: AppWindow| {
                                    app.set_show_update_dialog(false);
                                });

                                std::thread::sleep(std::time::Duration::from_millis(500));

                                std::process::Command::new(&installer_path).spawn().ok();
                                std::process::exit(0);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Erro ao baixar atualizacao: {}", e);
                    }
                }
            });
        }
    });

    Ok(())
}
