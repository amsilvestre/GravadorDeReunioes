#![allow(dead_code)]

use crate::audio::capture::{CaptureConfig, CaptureHandles};
use ringbuf::traits::{Consumer, Observer};

/// Resultado da mixagem: samples mixados + nivel de audio (RMS)
pub struct MixerOutput {
    pub samples: Vec<f32>,
    pub rms_level: f32,
}

/// Mixer que le de ambos os ring buffers (mic + loopback), mixa em mono e faz reamostragem.
pub struct Mixer {
    mic_consumer: ringbuf::HeapCons<f32>,
    loopback_consumer: ringbuf::HeapCons<f32>,
    mic_config: CaptureConfig,
    loopback_config: CaptureConfig,
    target_sample_rate: u32,
    source_sample_rate: u32,
    downsample_factor: usize,
}

impl Mixer {
    pub fn new(handles: CaptureHandles, target_sample_rate: u32, _target_channels: u16) -> Self {
        let source_rate = handles.loopback_config.sample_rate;

        let downsample_factor = if source_rate > target_sample_rate {
            (source_rate / target_sample_rate) as usize
        } else {
            1
        };

        Self {
            mic_config: handles.mic_config,
            loopback_config: handles.loopback_config,
            mic_consumer: handles.mic_consumer,
            loopback_consumer: handles.loopback_consumer,
            target_sample_rate,
            source_sample_rate: source_rate,
            downsample_factor,
        }
    }

    /// Le samples disponveis e retorna o bloco mixado.
    pub fn read_and_mix(&mut self) -> MixerOutput {
        let mic_raw = self.read_available(&mut ReadSource::Mic);
        let loopback_raw = self.read_available(&mut ReadSource::Loopback);

        // Converte para mono se necessario
        let mic_mono = Self::to_mono(&mic_raw, self.mic_config.channels);
        let loopback_mono = Self::to_mono(&loopback_raw, self.loopback_config.channels);

        // Mixa: soma ambas fontes
        let max_len = mic_mono.len().max(loopback_mono.len());
        let mut mixed = Vec::with_capacity(max_len);

        for i in 0..max_len {
            let mic_val = mic_mono.get(i).copied().unwrap_or(0.0);
            let loopback_val = loopback_mono.get(i).copied().unwrap_or(0.0);
            let val = (mic_val + loopback_val).clamp(-1.0, 1.0);
            mixed.push(val);
        }

        // Faz downsampling simples por média
        let output = if self.downsample_factor > 1 {
            Self::downsample(&mixed, self.downsample_factor)
        } else {
            mixed
        };

        // Calcula RMS para o medidor de nivel
        let rms_level = if output.is_empty() {
            0.0
        } else {
            let sum_sq: f32 = output.iter().map(|s| s * s).sum();
            (sum_sq / output.len() as f32).sqrt()
        };

        MixerOutput {
            samples: output,
            rms_level,
        }
    }

    fn read_available(&mut self, source: &mut ReadSource) -> Vec<f32> {
        let consumer = match source {
            ReadSource::Mic => &mut self.mic_consumer,
            ReadSource::Loopback => &mut self.loopback_consumer,
        };
        let available = consumer.occupied_len();
        if available == 0 {
            return Vec::new();
        }
        let mut buf = vec![0.0f32; available];
        let read = consumer.pop_slice(&mut buf);
        buf.truncate(read);
        buf
    }

    /// Converte samples interleaved multi-canal para mono (media dos canais)
    fn to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
        if channels == 1 {
            return samples.to_vec();
        }
        let ch = channels as usize;
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
    }

    /// Downsampling por média (anti-aliasing simples)
    fn downsample(samples: &[f32], factor: usize) -> Vec<f32> {
        if factor <= 1 || samples.is_empty() {
            return samples.to_vec();
        }
        let output_len = samples.len() / factor;
        let mut output = Vec::with_capacity(output_len);
        for i in 0..output_len {
            let chunk = &samples[i * factor..(i + 1) * factor];
            let sum: f32 = chunk.iter().sum();
            output.push(sum / factor as f32);
        }
        output
    }
}

enum ReadSource {
    Mic,
    Loopback,
}
