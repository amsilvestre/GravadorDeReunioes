#![allow(dead_code)]

pub mod cloud;
pub mod local;
pub mod model_downloader;

use anyhow::Result;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct TranscriptionSegment {
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

pub trait TranscriptionEngine: Send + Sync {
    fn transcribe(
        &self,
        wav_path: &Path,
        on_progress: Box<dyn Fn(f32) + Send>,
    ) -> Result<Vec<TranscriptionSegment>>;
}
