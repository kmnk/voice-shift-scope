//! `AudioPipeline` と `AudioDevices` の実装（cpal、WASAPI）。

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use voice_domain::ports::{AnalysisPublisher, Analyzer, AudioDevices, AudioPipeline, PipelineConfig, VoiceShifter};
use voice_domain::{
    AnalysisPair, AnalysisSettings, AudioSource, DeviceDirection as DomainDirection, DeviceInfo, DeviceList, MonitorSettings, Routing,
    SamplePosition, SampleRate, ShiftSettings,
};

use crate::engine::{AdaptiveReader, BLOCK, CORE_RATE, InputProcessor, Shared, StreamResampler};
use crate::{DeviceDirection, find_virtual_cable, probe_devices};

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("機器を列挙できませんでした: {0}")]
    Probe(#[from] cpal::Error),
    #[error("id が {0} の機器が見つかりません")]
    DeviceNotFound(String),
    #[error("{device} の既定の形式が f32 ではありません（{format:?}）")]
    UnsupportedFormat { device: String, format: cpal::SampleFormat },
    #[error("既に動いています")]
    AlreadyRunning,
}

/// cpal で機器を列挙する `AudioDevices`。
#[derive(Default, Clone)]
pub struct CpalDevices;

impl AudioDevices for CpalDevices {
    type Error = RuntimeError;

    fn list(&self) -> Result<DeviceList, RuntimeError> {
        let listing = probe_devices()?;
        let cable = find_virtual_cable(&listing.devices);
        let infos = listing
            .devices
            .iter()
            .map(|d| DeviceInfo {
                raw_id: d.id.clone(),
                name: d.name.clone(),
                direction: match d.direction {
                    DeviceDirection::Input => DomainDirection::Input,
                    DeviceDirection::Output => DomainDirection::Output,
                },
                is_default: d.is_default,
            })
            .collect();
        Ok(DeviceList::new(infos, cable.as_ref().map(|c| (c.playback.id.as_str(), c.recording.id.as_str()))))
    }
}

pub type ShifterFactory = Box<dyn Fn(u32) -> Box<dyn VoiceShifter> + Send>;
pub type AnalyzerFactory = Box<dyn Fn() -> Box<dyn Analyzer> + Send>;

/// 動いている間の状態と数。
#[derive(Debug, Clone, Default)]
pub struct PipelineStats {
    pub processed_samples: u64,
    pub output_underruns: u64,
    pub monitor_underruns: u64,
    pub output_xruns: u64,
    pub monitor_xruns: u64,
    pub output_underrun_events: u64,
    /// 最初と最後に出力が足りなくなったときの、処理していた入力の秒数。
    pub first_underrun_seconds: f64,
    pub last_underrun_seconds: f64,
    pub dropped_input: u64,
    /// Windows が録音の buffer を取りこぼした回数（cpal の Xrun）と、最後のときの秒数。
    pub input_xruns: u64,
    pub last_input_xrun_seconds: f64,
    pub output_fill_ms: f64,
    pub output_correction_ppm: i64,
    pub published_pairs: u64,
    pub shifter_latency: usize,
    pub errors: Vec<String>,
}

struct Running {
    shared: Arc<Shared>,
    _input: Option<cpal::Stream>,
    feeder: Option<JoinHandle<()>>,
    _output: cpal::Stream,
    monitor: Option<cpal::Stream>,
    monitor_device: Option<String>,
    monitor_reader: Arc<Mutex<Option<AdaptiveReader>>>,
    analysis: Option<JoinHandle<()>>,
    analysis_settings: Arc<Mutex<AnalysisSettings>>,
    published: Arc<std::sync::atomic::AtomicU64>,
    shifter_latency: usize,
    source_started: Arc<Mutex<Option<std::time::Instant>>>,
}

pub struct RuntimePipeline {
    shifters: ShifterFactory,
    analyzers: AnalyzerFactory,
    publisher: Arc<dyn AnalysisPublisher + Sync>,
    input_override: Option<Arc<Vec<f32>>>,
    errors: Arc<Mutex<Vec<String>>>,
    running: Option<Running>,
}

fn find_device(id: &str) -> Result<cpal::Device, RuntimeError> {
    for device in cpal::default_host().devices()? {
        if device.id().map(|i| i.to_string()).as_deref() == Ok(id) {
            return Ok(device);
        }
    }
    Err(RuntimeError::DeviceNotFound(id.to_string()))
}

fn f32_config(device: &cpal::Device, input: bool) -> Result<cpal::SupportedStreamConfig, RuntimeError> {
    let config = if input { device.default_input_config()? } else { device.default_output_config()? };
    if config.sample_format() != cpal::SampleFormat::F32 {
        return Err(RuntimeError::UnsupportedFormat {
            device: device.to_string(),
            format: config.sample_format(),
        });
    }
    Ok(config)
}

/// 出力とモニターの ring buffer に溜めておく量の既定（30 ms）。
/// 試験で替えるときは `VOICE_SHIFT_SCOPE_OUTPUT_BUFFER_MS`（画面からは替えられない）。
const DEFAULT_OUTPUT_BUFFER_MS: usize = 30;

fn output_target() -> usize {
    let ms = std::env::var("VOICE_SHIFT_SCOPE_OUTPUT_BUFFER_MS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|ms| (5..=500).contains(ms))
        .unwrap_or(DEFAULT_OUTPUT_BUFFER_MS);
    ms * CORE_RATE as usize / 1000
}

impl RuntimePipeline {
    pub fn new(
        shifters: ShifterFactory,
        analyzers: AnalyzerFactory,
        publisher: Arc<dyn AnalysisPublisher + Sync>,
    ) -> Self {
        Self { shifters, analyzers, publisher, input_override: None, errors: Arc::default(), running: None }
    }

    /// 試験用: 入力の機器の代わりに、`CORE_RATE` の mono の標本を実時間の速さで流す。
    pub fn with_input_samples(mut self, samples: Vec<f32>) -> Self {
        self.input_override = Some(Arc::new(samples));
        self
    }

    /// 試験用: wav の音源を流し始めた時刻（入力の機器を使うときは `None`）。
    pub fn source_started(&self) -> Option<std::time::Instant> {
        self.running.as_ref().and_then(|r| *r.source_started.lock().expect("始めた時刻"))
    }

    pub fn stats(&self) -> PipelineStats {
        let errors = self.errors.lock().map(|e| e.clone()).unwrap_or_default();
        match &self.running {
            None => PipelineStats { errors, ..Default::default() },
            Some(r) => PipelineStats {
                processed_samples: r.shared.processed.load(Ordering::Relaxed),
                output_underruns: r.shared.output_underruns.load(Ordering::Relaxed),
                monitor_underruns: r.shared.monitor_underruns.load(Ordering::Relaxed),
                output_xruns: r.shared.output_xruns.load(Ordering::Relaxed),
                monitor_xruns: r.shared.monitor_xruns.load(Ordering::Relaxed),
                output_underrun_events: r.shared.output_underrun_events.load(Ordering::Relaxed),
                first_underrun_seconds: r.shared.first_underrun_at.load(Ordering::Relaxed) as f64 / CORE_RATE as f64,
                last_underrun_seconds: r.shared.last_underrun_at.load(Ordering::Relaxed) as f64 / CORE_RATE as f64,
                dropped_input: r.shared.dropped_input.load(Ordering::Relaxed),
                input_xruns: r.shared.input_xruns.load(Ordering::Relaxed),
                last_input_xrun_seconds: r.shared.last_input_xrun_at.load(Ordering::Relaxed) as f64 / CORE_RATE as f64,
                output_fill_ms: r.shared.output_fill.load(Ordering::Relaxed) as f64 * 1000.0 / CORE_RATE as f64,
                output_correction_ppm: r.shared.output_correction_ppm.load(Ordering::Relaxed),
                published_pairs: r.published.load(Ordering::Relaxed),
                shifter_latency: r.shifter_latency,
                errors,
            },
        }
    }

    fn open_output(
        &self,
        id: &str,
        reader: Arc<Mutex<Option<AdaptiveReader>>>,
        shared: Arc<Shared>,
        is_monitor: bool,
    ) -> Result<(cpal::Stream, u32), RuntimeError> {
        let device = find_device(id)?;
        let config = f32_config(&device, false)?;
        let rate = config.sample_rate();
        let channels = config.channels() as usize;
        let errors = self.errors.clone();
        let mut mono = vec![0.0f32; 16_384];
        let xrun_shared = shared.clone();
        let stream = device.build_output_stream(
            config.config(),
            move |data: &mut [f32], _| {
                let frames = data.len() / channels;
                let mono = &mut mono[..frames.min(16_384)];
                // 読み手の入れ替えのときだけロックがある。待たずに、取れなければ無音にする
                let missing = match reader.try_lock() {
                    Ok(mut guard) => match guard.as_mut() {
                        Some(r) => {
                            let missing = r.read(mono);
                            if !is_monitor {
                                shared.output_fill.store(r.fill() as u64, Ordering::Relaxed);
                                shared.output_correction_ppm.store((r.correction() * 1e6) as i64, Ordering::Relaxed);
                            }
                            missing
                        }
                        None => {
                            mono.fill(0.0);
                            0
                        }
                    },
                    Err(_) => {
                        mono.fill(0.0);
                        0
                    }
                };
                let gain = if is_monitor { f32::from_bits(shared.monitor_volume.load(Ordering::Relaxed)) } else { 1.0 };
                if missing > 0 {
                    let counter = if is_monitor { &shared.monitor_underruns } else { &shared.output_underruns };
                    counter.fetch_add(missing as u64, Ordering::Relaxed);
                    if !is_monitor {
                        let at = shared.processed.load(Ordering::Relaxed);
                        if shared.output_underrun_events.fetch_add(1, Ordering::Relaxed) == 0 {
                            shared.first_underrun_at.store(at, Ordering::Relaxed);
                        }
                        shared.last_underrun_at.store(at, Ordering::Relaxed);
                    }
                }
                for (frame, v) in data.chunks_mut(channels).zip(mono.iter()) {
                    frame.fill(v * gain);
                }
            },
            move |e| {
                if e.kind() == cpal::ErrorKind::Xrun {
                    let counter = if is_monitor { &xrun_shared.monitor_xruns } else { &xrun_shared.output_xruns };
                    counter.fetch_add(1, Ordering::Relaxed);
                } else if let Ok(mut errs) = errors.lock() {
                    errs.push(format!("{}: {e}", if is_monitor { "モニター" } else { "出力" }));
                }
            },
            None,
        )?;
        stream.play()?;
        Ok((stream, rate))
    }
}

impl RuntimePipeline {
    fn open_monitor(
        &self,
        id: &str,
        reader: &Arc<Mutex<Option<AdaptiveReader>>>,
        shared: &Arc<Shared>,
    ) -> Result<cpal::Stream, RuntimeError> {
        let (stream, rate) = self.open_output(id, reader.clone(), shared.clone(), true)?;
        if let Some(r) = reader.lock().expect("読み手").as_mut() {
            r.retarget(CORE_RATE, rate);
        }
        shared.monitor_enabled.store(true, Ordering::Relaxed);
        Ok(stream)
    }
}

impl AudioPipeline for RuntimePipeline {
    type Error = RuntimeError;

    fn start(&mut self, config: PipelineConfig) -> Result<(), RuntimeError> {
        if self.running.is_some() {
            return Err(RuntimeError::AlreadyRunning);
        }
        self.errors.lock().map(|mut e| e.clear()).ok();
        let shared = Shared::new(config.shift, config.monitor.volume.get(), config.monitor.enabled);
        let shifter = (self.shifters)(CORE_RATE);
        let shifter_latency = shifter.latency();

        let (out_prod, out_cons) = HeapRb::<f32>::new(CORE_RATE as usize).split();
        let (mon_prod, mon_cons) = HeapRb::<f32>::new(CORE_RATE as usize).split();
        let (before_prod, mut before_cons) = HeapRb::<f32>::new(CORE_RATE as usize * 2).split();
        let (after_prod, mut after_cons) = HeapRb::<f32>::new(CORE_RATE as usize * 2).split();

        // 出力（VB-CABLE の再生側）
        let out_reader = Arc::new(Mutex::new(None));
        let (output, out_rate) =
            self.open_output(config.output.playback.raw(), out_reader.clone(), shared.clone(), false)?;
        *out_reader.lock().expect("読み手") = Some(AdaptiveReader::new(out_cons, CORE_RATE, out_rate, output_target()));

        // モニター: 読み手は始めに作って残し、機器のストリームだけを開け閉めする
        let monitor_reader =
            Arc::new(Mutex::new(Some(AdaptiveReader::new(mon_cons, CORE_RATE, CORE_RATE, output_target()))));
        let mut monitor = None;
        let mut monitor_device = None;
        if let (true, Some(device)) = (config.monitor.enabled, config.monitor.device.as_ref()) {
            monitor = Some(self.open_monitor(device.raw(), &monitor_reader, &shared)?);
            monitor_device = Some(device.raw().to_string());
        }

        shared.output_original.store(config.routing.output == AudioSource::Original, Ordering::Relaxed);
        shared.monitor_original.store(config.routing.monitor == AudioSource::Original, Ordering::Relaxed);
        let mut processor = InputProcessor::new(shifter, shared.clone(), out_prod, mon_prod, before_prod, after_prod)
            .with_original_delay(shifter_latency);

        // 入力
        let source_started = Arc::new(Mutex::new(None));
        let (input, feeder) = match self.input_override.clone() {
            Some(samples) => {
                let shared = shared.clone();
                let started = source_started.clone();
                let handle = std::thread::spawn(move || {
                    let start = std::time::Instant::now();
                    *started.lock().expect("始めた時刻") = Some(start);
                    let silence = vec![0.0f32; BLOCK];
                    let mut sent = 0usize;
                    while !shared.stop.load(Ordering::Relaxed) {
                        let due = (start.elapsed().as_secs_f64() * CORE_RATE as f64) as usize;
                        while sent + BLOCK <= due {
                            let chunk = samples.get(sent..sent + BLOCK).unwrap_or(&silence);
                            processor.push(chunk);
                            sent += BLOCK;
                        }
                        std::thread::sleep(Duration::from_millis(2));
                    }
                });
                (None, Some(handle))
            }
            None => {
                // 入力のコールバックでは、mono にして速さをそろえ、ring buffer に入れるだけにする。
                // 声を変える処理は重さに波があり、コールバックの中で動かすと Windows が録音の buffer を
                // 取りこぼす（cpal の Xrun）ので、別の処理のスレッドで動かす
                let device = find_device(config.input.raw())?;
                let in_config = f32_config(&device, true)?;
                let channels = in_config.channels() as usize;
                let mut resampler = StreamResampler::new(in_config.sample_rate(), CORE_RATE, 16_384);
                let mut mono = vec![0.0f32; 16_384];
                let (mut in_prod, mut in_cons) = HeapRb::<f32>::new(CORE_RATE as usize).split();
                let errors = self.errors.clone();
                let xrun_shared = shared.clone();
                let callback_shared = shared.clone();
                let stream = device.build_input_stream(
                    in_config.config(),
                    move |data: &[f32], _| {
                        let frames = (data.len() / channels).min(mono.len());
                        for (m, frame) in mono[..frames].iter_mut().zip(data.chunks(channels)) {
                            *m = frame.iter().sum::<f32>() / channels as f32;
                        }
                        let block: &[f32] =
                            if resampler.is_identity() { &mono[..frames] } else { resampler.process(&mono[..frames]) };
                        let pushed = in_prod.push_slice(block);
                        if pushed < block.len() {
                            callback_shared.dropped_input.fetch_add((block.len() - pushed) as u64, Ordering::Relaxed);
                        }
                    },
                    move |e| {
                        if e.kind() == cpal::ErrorKind::Xrun {
                            // 録音の取りこぼしは数えて、画面には回数として出す。
                            // 録音を始めた瞬間（まだ 1 ブロックも処理していない間）の印は、音の途切れではないので数えない
                            let at = xrun_shared.processed.load(Ordering::Relaxed);
                            if at == 0 {
                                return;
                            }
                            xrun_shared.input_xruns.fetch_add(1, Ordering::Relaxed);
                            xrun_shared.last_input_xrun_at.store(at, Ordering::Relaxed);
                        } else if let Ok(mut errs) = errors.lock() {
                            errs.push(format!("入力: {e}"));
                        }
                    },
                    None,
                )?;
                stream.play()?;
                let worker_shared = shared.clone();
                let worker = std::thread::spawn(move || {
                    let mut scratch = vec![0.0f32; 4096];
                    while !worker_shared.stop.load(Ordering::Relaxed) {
                        let n = in_cons.pop_slice(&mut scratch);
                        if n > 0 {
                            processor.push(&scratch[..n]);
                        } else {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                    }
                });
                (Some(stream), Some(worker))
            }
        };

        // 分析のスレッド: 変えた後は声を変える処理の遅れだけ遅れて届くので、その分を捨てて前と後の時刻をそろえる
        let analysis_settings = Arc::new(Mutex::new(config.analysis));
        let published = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let analysis = {
            let shared = shared.clone();
            let settings = analysis_settings.clone();
            let publisher = self.publisher.clone();
            let published = published.clone();
            let mut before_an = (self.analyzers)();
            let mut after_an = (self.analyzers)();
            std::thread::spawn(move || {
                let window = 4096usize;
                let rate = SampleRate::new(CORE_RATE).expect("0 より大きい");
                let mut before = vec![0.0f32; window];
                let mut after = vec![0.0f32; window];
                let mut chunk_b = vec![0.0f32; BLOCK];
                let mut chunk_a = vec![0.0f32; BLOCK];
                let mut skip_after = shifter_latency;
                let mut position = 0u64;
                while !shared.stop.load(Ordering::Relaxed) {
                    while skip_after > 0 && after_cons.occupied_len() > 0 {
                        let n = skip_after.min(after_cons.occupied_len()).min(BLOCK);
                        after_cons.pop_slice(&mut chunk_a[..n]);
                        skip_after -= n;
                    }
                    if skip_after > 0 || before_cons.occupied_len() < BLOCK || after_cons.occupied_len() < BLOCK {
                        std::thread::sleep(Duration::from_millis(3));
                        continue;
                    }
                    // 分析が追いつかないときは、溜まったブロックを窓に流し込むだけにして、最新のブロックで分析する。
                    // 前と後は同じ数ずつ取り出すので、時刻はそろったまま
                    while before_cons.occupied_len() >= BLOCK && after_cons.occupied_len() >= BLOCK {
                        before_cons.pop_slice(&mut chunk_b);
                        after_cons.pop_slice(&mut chunk_a);
                        before.copy_within(BLOCK.., 0);
                        after.copy_within(BLOCK.., 0);
                        before[window - BLOCK..].copy_from_slice(&chunk_b);
                        after[window - BLOCK..].copy_from_slice(&chunk_a);
                        position += BLOCK as u64;
                    }
                    let analysis = *settings.lock().expect("分析の設定");
                    let shift = shared_shift(&shared);
                    let b = before_an.analyze(&before, rate, &analysis);
                    // 変えた後は、フォルマントを上げた分だけ上限も上げて分析する
                    let a = after_an.analyze(&after, rate, &analysis.for_shifted(shift.formant));
                    let hint = b.f0.map(|f| f.hz()).unwrap_or(0.0);
                    shared.pitch_hint.store(hint.to_bits(), Ordering::Relaxed);
                    publisher.publish(&AnalysisPair {
                        position: SamplePosition(position),
                        before: b,
                        after: a,
                        shift,
                        analysis,
                    });
                    published.fetch_add(1, Ordering::Relaxed);
                }
            })
        };

        self.running = Some(Running {
            shared,
            _input: input,
            feeder,
            _output: output,
            monitor,
            monitor_device,
            monitor_reader,
            analysis: Some(analysis),
            analysis_settings,
            published,
            shifter_latency,
            source_started,
        });
        Ok(())
    }

    fn stop(&mut self) -> Result<(), RuntimeError> {
        if let Some(mut r) = self.running.take() {
            r.shared.stop.store(true, Ordering::Relaxed);
            if let Some(h) = r.feeder.take() {
                let _ = h.join();
            }
            if let Some(h) = r.analysis.take() {
                let _ = h.join();
            }
        }
        Ok(())
    }

    fn set_shift(&mut self, shift: ShiftSettings) -> Result<(), RuntimeError> {
        if let Some(r) = &self.running {
            r.shared.set_shift(shift);
        }
        Ok(())
    }

    fn set_analysis(&mut self, analysis: AnalysisSettings) -> Result<(), RuntimeError> {
        if let Some(r) = &self.running {
            *r.analysis_settings.lock().expect("分析の設定") = analysis;
        }
        Ok(())
    }

    fn set_routing(&mut self, routing: Routing) -> Result<(), RuntimeError> {
        if let Some(r) = &self.running {
            r.shared.output_original.store(routing.output == AudioSource::Original, Ordering::Relaxed);
            r.shared.monitor_original.store(routing.monitor == AudioSource::Original, Ordering::Relaxed);
        }
        Ok(())
    }

    fn set_monitor(&mut self, monitor: MonitorSettings) -> Result<(), RuntimeError> {
        let Some(r) = &mut self.running else { return Ok(()) };
        r.shared.monitor_volume.store(monitor.volume.get().to_bits(), Ordering::Relaxed);
        let wanted = if monitor.enabled { monitor.device.as_ref().map(|d| d.raw().to_string()) } else { None };
        if wanted != r.monitor_device {
            // モニターのストリームだけを閉じて開き直す。出力と入力は止めない
            r.shared.monitor_enabled.store(false, Ordering::Relaxed);
            r.monitor = None;
            r.monitor_device = None;
            if let Some(id) = wanted {
                let (reader, shared) = (r.monitor_reader.clone(), r.shared.clone());
                let stream = self.open_monitor(&id, &reader, &shared)?;
                let r = self.running.as_mut().expect("動いている");
                r.monitor = Some(stream);
                r.monitor_device = Some(id);
            }
        }
        if let Some(r) = &self.running {
            r.shared.monitor_enabled.store(r.monitor_device.is_some(), Ordering::Relaxed);
        }
        Ok(())
    }
}

fn shared_shift(shared: &Shared) -> ShiftSettings {
    shared.current_shift().unwrap_or(ShiftSettings::NONE)
}
