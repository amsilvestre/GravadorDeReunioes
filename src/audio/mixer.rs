use crate::audio::capture::{CaptureConfig, CaptureHandles};
use ringbuf::traits::{Consumer, Observer};

/// Resultado da mixagem: samples mixados + nivel de audio (RMS)
pub struct MixerOutput {
    pub samples: Vec<f32>,
    pub rms_level: f32,
}

/// Mixer que le de ambos os ring buffers (mic + loopback) e mixa em mono.
pub struct Mixer {
    mic_consumer: ringbuf::HeapCons<f32>,
    loopback_consumer: ringbuf::HeapCons<f32>,
    mic_config: CaptureConfig,
    loopback_config: CaptureConfig,
    target_sample_rate: u32,
    target_channels: u16,
}

impl Mixer {
    pub fn new(handles: CaptureHandles, target_sample_rate: u32, target_channels: u16) -> Self {
        Self {
            mic_config: handles.mic_config,
            loopback_config: handles.loopback_config,
            mic_consumer: handles.mic_consumer,
            loopback_consumer: handles.loopback_consumer,
            target_sample_rate,
            target_channels,
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

        // Calcula RMS para o medidor de nivel
        let rms_level = if mixed.is_empty() {
            0.0
        } else {
            let sum_sq: f32 = mixed.iter().map(|s| s * s).sum();
            (sum_sq / mixed.len() as f32).sqrt()
        };

        // Se target_channels == 2, duplica para stereo
        let output = if self.target_channels == 2 {
            let mut stereo = Vec::with_capacity(mixed.len() * 2);
            for s in &mixed {
                stereo.push(*s);
                stereo.push(*s);
            }
            stereo
        } else {
            mixed
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

    pub fn target_sample_rate(&self) -> u32 {
        self.target_sample_rate
    }

    pub fn target_channels(&self) -> u16 {
        self.target_channels
    }
}

enum ReadSource {
    Mic,
    Loopback,
}
