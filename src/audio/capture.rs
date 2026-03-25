#![allow(dead_code)]

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};
use ringbuf::traits::{Producer, Split};
use ringbuf::HeapRb;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Tamanho do ring buffer em samples (5 segundos a 48kHz stereo)
const RING_BUFFER_SIZE: usize = 48000 * 2 * 5;

pub struct AudioCapture {
    mic_stream: Option<Stream>,
    loopback_stream: Option<Stream>,
    running: Arc<AtomicBool>,
}

pub struct CaptureConfig {
    pub sample_rate: u32,
    pub channels: u16,
}

pub struct CaptureHandles {
    pub mic_consumer: ringbuf::HeapCons<f32>,
    pub loopback_consumer: ringbuf::HeapCons<f32>,
    pub mic_config: CaptureConfig,
    pub loopback_config: CaptureConfig,
}

impl AudioCapture {
    /// Inicia captura de audio do microfone e do sistema (WASAPI loopback).
    /// Retorna os consumers dos ring buffers para o mixer ler.
    pub fn start() -> Result<(Self, CaptureHandles)> {
        let host = cpal::default_host();
        let running = Arc::new(AtomicBool::new(true));

        // === Microfone ===
        let mic_device = host
            .default_input_device()
            .context("Nenhum dispositivo de entrada (microfone) encontrado")?;

        let mic_supported_config = mic_device
            .default_input_config()
            .context("Nao foi possivel obter config do microfone")?;

        let mic_config = StreamConfig {
            channels: mic_supported_config.channels(),
            sample_rate: mic_supported_config.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };

        let mic_rb = HeapRb::<f32>::new(RING_BUFFER_SIZE);
        let (mic_producer, mic_consumer) = mic_rb.split();

        let mic_stream = Self::build_input_stream(
            &mic_device,
            &mic_config,
            mic_supported_config.sample_format(),
            mic_producer,
            running.clone(),
        )?;

        // === Loopback (audio do sistema via WASAPI) ===
        let loopback_device = host
            .default_output_device()
            .context("Nenhum dispositivo de saida encontrado para loopback")?;

        let loopback_supported_config = loopback_device
            .default_output_config()
            .context("Nao foi possivel obter config do loopback")?;

        let loopback_config = StreamConfig {
            channels: loopback_supported_config.channels(),
            sample_rate: loopback_supported_config.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };

        let loopback_rb = HeapRb::<f32>::new(RING_BUFFER_SIZE);
        let (loopback_producer, loopback_consumer) = loopback_rb.split();

        // No cpal, ao chamar build_input_stream no dispositivo de OUTPUT,
        // o backend WASAPI automaticamente ativa o modo loopback
        let loopback_stream = Self::build_input_stream(
            &loopback_device,
            &loopback_config,
            loopback_supported_config.sample_format(),
            loopback_producer,
            running.clone(),
        )?;

        // Inicia ambos os streams
        mic_stream
            .play()
            .context("Falha ao iniciar stream do microfone")?;
        loopback_stream
            .play()
            .context("Falha ao iniciar stream do loopback")?;

        let capture = Self {
            mic_stream: Some(mic_stream),
            loopback_stream: Some(loopback_stream),
            running,
        };

        let handles = CaptureHandles {
            mic_consumer,
            loopback_consumer,
            mic_config: CaptureConfig {
                sample_rate: mic_config.sample_rate.0,
                channels: mic_config.channels,
            },
            loopback_config: CaptureConfig {
                sample_rate: loopback_config.sample_rate.0,
                channels: loopback_config.channels,
            },
        };

        Ok((capture, handles))
    }

    fn build_input_stream(
        device: &Device,
        config: &StreamConfig,
        sample_format: SampleFormat,
        producer: ringbuf::HeapProd<f32>,
        running: Arc<AtomicBool>,
    ) -> Result<Stream> {
        let err_fn = |err| eprintln!("Erro no stream de audio: {}", err);

        let stream = match sample_format {
            SampleFormat::F32 => {
                let mut producer = producer;
                device.build_input_stream(
                    config,
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        if running.load(Ordering::Relaxed) {
                            let _ = producer.push_slice(data);
                        }
                    },
                    err_fn,
                    None,
                )?
            }
            SampleFormat::I16 => {
                let mut producer = producer;
                device.build_input_stream(
                    config,
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        if running.load(Ordering::Relaxed) {
                            for &sample in data {
                                let f = sample as f32 / i16::MAX as f32;
                                let _ = producer.try_push(f);
                            }
                        }
                    },
                    err_fn,
                    None,
                )?
            }
            SampleFormat::U16 => {
                let mut producer = producer;
                device.build_input_stream(
                    config,
                    move |data: &[u16], _: &cpal::InputCallbackInfo| {
                        if running.load(Ordering::Relaxed) {
                            for &sample in data {
                                let f = (sample as f32 / u16::MAX as f32) * 2.0 - 1.0;
                                let _ = producer.try_push(f);
                            }
                        }
                    },
                    err_fn,
                    None,
                )?
            }
            _ => anyhow::bail!("Formato de sample nao suportado: {:?}", sample_format),
        };

        Ok(stream)
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        // Drop dos streams para parar a captura
        self.mic_stream.take();
        self.loopback_stream.take();
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.stop();
    }
}
