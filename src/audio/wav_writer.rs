#![allow(dead_code)]

use anyhow::{Context, Result};
use hound::{SampleFormat, WavSpec, WavWriter as HoundWriter};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

pub struct WavFileWriter {
    writer: HoundWriter<BufWriter<std::fs::File>>,
    path: PathBuf,
    samples_written: u64,
    sample_rate: u32,
}

impl WavFileWriter {
    pub fn new(path: &Path, sample_rate: u32, channels: u16) -> Result<Self> {
        // Garante que o diretorio existe
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let spec = WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };

        let writer = HoundWriter::create(path, spec)
            .with_context(|| format!("Falha ao criar arquivo WAV: {:?}", path))?;

        Ok(Self {
            writer,
            path: path.to_path_buf(),
            samples_written: 0,
            sample_rate,
        })
    }

    /// Escreve um bloco de samples f32 no arquivo WAV
    pub fn write_samples(&mut self, samples: &[f32]) -> Result<()> {
        for &sample in samples {
            self.writer.write_sample(sample)?;
        }
        self.samples_written += samples.len() as u64;
        Ok(())
    }

    /// Finaliza o arquivo WAV (escreve header correto)
    pub fn finalize(self) -> Result<PathBuf> {
        let path = self.path.clone();
        self.writer
            .finalize()
            .context("Falha ao finalizar arquivo WAV")?;
        Ok(path)
    }

    /// Retorna a duracao em segundos da gravacao ate o momento
    pub fn duration_secs(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.samples_written as f64 / self.sample_rate as f64
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
