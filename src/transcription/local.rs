use crate::transcription::{TranscriptionEngine, TranscriptionSegment};
use anyhow::{Context, Result};
use std::path::Path;

pub struct LocalEngine {
    model_path: std::path::PathBuf,
}

impl LocalEngine {
    pub fn new(model_path: std::path::PathBuf) -> Self {
        Self { model_path }
    }

    /// Carrega arquivo WAV e converte para 16kHz mono f32 (formato exigido pelo whisper)
    fn load_wav_as_16khz_mono(wav_path: &Path) -> Result<Vec<f32>> {
        let mut reader =
            hound::WavReader::open(wav_path).context("Falha ao abrir arquivo WAV")?;
        let spec = reader.spec();

        // Le todos os samples como f32
        let samples: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => reader
                .samples::<f32>()
                .collect::<std::result::Result<Vec<_>, _>>()?,
            hound::SampleFormat::Int => {
                let max_val = (1 << (spec.bits_per_sample - 1)) as f32;
                reader
                    .samples::<i32>()
                    .map(|s| s.map(|v| v as f32 / max_val))
                    .collect::<std::result::Result<Vec<_>, _>>()?
            }
        };

        // Converte para mono se stereo
        let mono = if spec.channels > 1 {
            let ch = spec.channels as usize;
            let num_frames = samples.len() / ch;
            let mut mono = Vec::with_capacity(num_frames);
            for frame in 0..num_frames {
                let mut sum = 0.0f32;
                for c in 0..ch {
                    sum += samples[frame * ch + c];
                }
                mono.push(sum / ch as f32);
            }
            mono
        } else {
            samples
        };

        // Resample para 16kHz se necessario
        if spec.sample_rate == 16000 {
            return Ok(mono);
        }

        // Resample simples por interpolacao linear
        let ratio = 16000.0 / spec.sample_rate as f64;
        let output_len = (mono.len() as f64 * ratio) as usize;
        let mut resampled = Vec::with_capacity(output_len);

        for i in 0..output_len {
            let src_pos = i as f64 / ratio;
            let idx = src_pos as usize;
            let frac = (src_pos - idx as f64) as f32;

            let s0 = mono.get(idx).copied().unwrap_or(0.0);
            let s1 = mono.get(idx + 1).copied().unwrap_or(s0);
            resampled.push(s0 + frac * (s1 - s0));
        }

        Ok(resampled)
    }
}

impl TranscriptionEngine for LocalEngine {
    fn transcribe(
        &self,
        wav_path: &Path,
        on_progress: Box<dyn Fn(f32) + Send>,
    ) -> Result<Vec<TranscriptionSegment>> {
        on_progress(0.05);

        // Verifica se o modelo existe
        if !self.model_path.exists() {
            anyhow::bail!(
                "Modelo Whisper nao encontrado em: {:?}\n\
                 Baixe um modelo GGML em https://huggingface.co/ggerganov/whisper.cpp/tree/main\n\
                 e coloque na pasta de modelos.",
                self.model_path
            );
        }

        on_progress(0.1);

        // Carrega o modelo whisper
        let ctx = whisper_rs::WhisperContext::new_with_params(
            self.model_path.to_str().unwrap_or_default(),
            whisper_rs::WhisperContextParameters::default(),
        )
        .context("Falha ao carregar modelo Whisper")?;

        on_progress(0.2);

        // Carrega e converte o audio
        let audio_data =
            Self::load_wav_as_16khz_mono(wav_path).context("Falha ao carregar audio WAV")?;

        on_progress(0.3);

        // Configura parametros de transcricao
        let mut params = whisper_rs::FullParams::new(whisper_rs::SamplingStrategy::Greedy {
            best_of: 1,
        });
        params.set_language(Some("pt"));
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        // Usa n-1 threads para deixar 1 para a UI
        let n_threads = (num_cpus::get() as i32 - 1).max(1);
        params.set_n_threads(n_threads);

        // Callback de progresso
        let progress_cb = on_progress;
        params.set_progress_callback_safe(move |progress| {
            // progress vai de 0 a 100
            let normalized = 0.3 + (progress as f32 / 100.0) * 0.65;
            progress_cb(normalized);
        });

        // Executa transcricao
        let mut state = ctx.create_state().context("Falha ao criar estado Whisper")?;
        state
            .full(params, &audio_data)
            .context("Falha durante transcricao Whisper")?;

        // Extrai segmentos
        let num_segments = state.full_n_segments();
        let mut segments = Vec::with_capacity(num_segments as usize);

        for i in 0..num_segments {
            if let Some(segment) = state.get_segment(i) {
                let text = segment.to_str_lossy().unwrap_or_default().trim().to_string();
                let start_cs = segment.start_timestamp(); // centiseconds
                let end_cs = segment.end_timestamp();
                segments.push(TranscriptionSegment {
                    text,
                    start_ms: (start_cs as u64) * 10,
                    end_ms: (end_cs as u64) * 10,
                });
            }
        }

        Ok(segments)
    }
}
