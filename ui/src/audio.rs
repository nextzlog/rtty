use std::collections::VecDeque;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::clock::Clock;

fn name(d: &cpal::Device) -> String {
    d.description()
        .map(|x| x.name().to_string())
        .unwrap_or_default()
}

pub fn input_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|ds| ds.map(|d| name(&d)).collect())
        .unwrap_or_default()
}

pub fn output_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.output_devices()
        .map(|ds| ds.map(|d| name(&d)).collect())
        .unwrap_or_default()
}

fn find(
    list: Result<impl Iterator<Item = cpal::Device>, cpal::Error>,
    default: Option<cpal::Device>,
    wanted: Option<&str>,
) -> Result<cpal::Device, String> {
    match wanted {
        Some(n) => list
            .map_err(|e| e.to_string())?
            .find(|d| name(d) == n)
            .ok_or_else(|| format!("no audio device {n}")),
        None => default.ok_or_else(|| "no audio device".to_string()),
    }
}

pub struct Input {
    _stream: cpal::Stream,
    rx: Receiver<Vec<f32>>,
    pub rate: u32,
    clock: Clock,
}

impl Input {
    pub fn open(wanted: Option<&str>) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = find(host.input_devices(), host.default_input_device(), wanted)?;
        let config = device.default_input_config().map_err(|e| e.to_string())?;
        let channels = config.channels() as usize;
        let rate = config.sample_rate();
        let format = config.sample_format();
        let (tx, rx) = channel();
        let err = |e| eprintln!("audio input: {e}");
        let stream = match format {
            cpal::SampleFormat::F32 => device.build_input_stream(
                config.into(),
                move |data: &[f32], _: &_| {
                    let _ = tx.send(data.chunks(channels).map(|f| f[0]).collect());
                },
                err,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                config.into(),
                move |data: &[i16], _: &_| {
                    let _ = tx.send(
                        data.chunks(channels)
                            .map(|f| f[0] as f32 / 32768.0)
                            .collect(),
                    );
                },
                err,
                None,
            ),
            other => return Err(format!("unsupported sample format {other:?}")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Input {
            _stream: stream,
            rx,
            rate,
            clock: Clock::default(),
        })
    }

    pub fn take(&mut self) -> Option<(Vec<f32>, SystemTime)> {
        let x: Vec<f32> = self.rx.try_iter().flatten().collect();
        if x.is_empty() {
            return None;
        }
        let at = self.clock.stamp(x.len(), self.rate, SystemTime::now());
        Some((x, at))
    }
}

pub struct Output {
    _stream: cpal::Stream,
    queue: Arc<Mutex<VecDeque<f32>>>,
    pub rate: u32,
}

impl Output {
    pub fn open(wanted: Option<&str>) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = find(host.output_devices(), host.default_output_device(), wanted)?;
        let config = device.default_output_config().map_err(|e| e.to_string())?;
        let channels = config.channels() as usize;
        let rate = config.sample_rate();
        let format = config.sample_format();
        let queue = Arc::new(Mutex::new(VecDeque::<f32>::new()));
        let q = queue.clone();
        let next = move || q.lock().ok().and_then(|mut q| q.pop_front()).unwrap_or(0.0);
        let err = |e| eprintln!("audio output: {e}");
        let stream = match format {
            cpal::SampleFormat::F32 => device.build_output_stream(
                config.into(),
                move |data: &mut [f32], _: &_| {
                    for f in data.chunks_mut(channels) {
                        f.fill(next());
                    }
                },
                err,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_output_stream(
                config.into(),
                move |data: &mut [i16], _: &_| {
                    for f in data.chunks_mut(channels) {
                        f.fill((next() * 32767.0) as i16);
                    }
                },
                err,
                None,
            ),
            other => return Err(format!("unsupported sample format {other:?}")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Output {
            _stream: stream,
            queue,
            rate,
        })
    }

    pub fn play(&self, samples: &[f32]) {
        if let Ok(mut q) = self.queue.lock() {
            q.extend(samples.iter().map(|v| v.clamp(-1.0, 1.0)));
        }
    }

    pub fn stop(&self) {
        if let Ok(mut q) = self.queue.lock() {
            q.clear();
        }
    }
}
