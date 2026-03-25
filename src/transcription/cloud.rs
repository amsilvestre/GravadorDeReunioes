use crate::transcription::{TranscriptionEngine, TranscriptionSegment};
use anyhow::{Context, Result};
use async_openai::{
    config::OpenAIConfig,
    types::{AudioResponseFormat, CreateTranscriptionRequestArgs},
    Client,
};
use std::path::Path;

pub struct CloudEngine {
    api_key: String,
}

impl CloudEngine {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }
}

impl TranscriptionEngine for CloudEngine {
    fn transcribe(
        &self,
        wav_path: &Path,
        language: Option<&str>,
        on_progress: Box<dyn Fn(f32) + Send>,
    ) -> Result<Vec<TranscriptionSegment>> {
        let api_key = self.api_key.clone();
        let wav_path = wav_path.to_path_buf();

        // Cria runtime tokio para rodar o async
        let rt = tokio::runtime::Runtime::new()?;

        rt.block_on(async move {
            let config = OpenAIConfig::new().with_api_key(&api_key);
            let client = Client::with_config(config);

            on_progress(0.1);

            // Verifica tamanho do arquivo — OpenAI aceita ate 25MB
            let file_size = std::fs::metadata(&wav_path)
                .with_context(|| format!("Arquivo nao encontrado: {:?}", wav_path))?
                .len();

            if file_size > 25 * 1024 * 1024 {
                // TODO: implementar split de arquivo para arquivos grandes
                anyhow::bail!(
                    "Arquivo WAV muito grande ({:.1} MB). Limite da API: 25 MB. Suporte a split sera adicionado em breve.",
                    file_size as f64 / (1024.0 * 1024.0)
                );
            }

            on_progress(0.2);

            // Define idioma
            let lang = language.unwrap_or("pt");

            let request = CreateTranscriptionRequestArgs::default()
                .file(wav_path)
                .model("whisper-1")
                .language(lang)
                .response_format(AudioResponseFormat::VerboseJson)
                .build()?;

            on_progress(0.3);

            let response = client
                .audio()
                .transcribe(request)
                .await
                .context("Falha na chamada da API OpenAI Whisper")?;

            on_progress(0.9);

            // VerboseJson retorna texto — fazemos parse basico por sentencas
            let mut segments = Vec::new();
            let text = response.text.trim();

            if text.is_empty() {
                anyhow::bail!("Transcricao retornou vazia");
            }

            // Divide por sentencas simples
            let sentences: Vec<&str> = text
                .split(|c: char| c == '.' || c == '!' || c == '?')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();

            for sentence in &sentences {
                segments.push(TranscriptionSegment {
                    text: sentence.to_string(),
                    start_ms: 0, // API basica nao retorna timestamps por segmento
                    end_ms: 0,
                });
            }

            if segments.is_empty() {
                segments.push(TranscriptionSegment {
                    text: text.to_string(),
                    start_ms: 0,
                    end_ms: 0,
                });
            }

            on_progress(1.0);
            Ok(segments)
        })
    }
}
