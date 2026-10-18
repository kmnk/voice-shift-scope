//! domain が外の技術と話す口（port）。実装は adapter の crate が持つ。

use crate::{
    AnalysisFrame, AnalysisPair, AnalysisSettings, DeviceId, DeviceList, Formant, Frequency, Harmonic,
    MonitorSettings, SampleRate, ShiftSettings, VirtualCable,
};

/// 声を変える処理。音声のスレッドから呼ばれるので、`process` の中でメモリを確保しない。
pub trait VoiceShifter: Send {
    fn set(&mut self, settings: ShiftSettings);
    fn process(&mut self, input: &[f32], output: &mut [f32]);
    /// 入力から出力までの遅れ（標本の数）。
    fn latency(&self) -> usize;
    /// 入力の F0 の目安（Hz）。フォルマントの包絡を求めるのに使う方式だけが使う。
    /// 音声のスレッドから呼ばれるので、メモリを確保しない。
    fn set_pitch_hint(&mut self, _f0: f32) {}
}

pub trait PitchEstimator: Send {
    fn estimate(&mut self, window: &[f32], rate: SampleRate) -> Option<Frequency>;
}

pub trait FormantEstimator: Send {
    fn estimate(&mut self, window: &[f32], rate: SampleRate, settings: &AnalysisSettings) -> [Option<Formant>; 4];
}

pub trait HarmonicPicker: Send {
    /// 倍音と、1 番目から途切れずに観測できた倍音の数。
    fn pick(
        &mut self,
        window: &[f32],
        rate: SampleRate,
        f0: Frequency,
        settings: &AnalysisSettings,
    ) -> (Vec<Harmonic>, u32);
}

/// 窓の samples から `AnalysisFrame` を作る（domain service の口）。
pub trait Analyzer: Send {
    fn analyze(&mut self, window: &[f32], rate: SampleRate, settings: &AnalysisSettings) -> AnalysisFrame;
}

/// 機器の一覧。呼ぶたびに今の一覧を取り直す。
pub trait AudioDevices {
    type Error: std::error::Error + Send + Sync + 'static;
    fn list(&self) -> Result<DeviceList, Self::Error>;
}

/// 音声のスレッドと分析のスレッドを始めるときの設定。
#[derive(Debug, Clone, PartialEq)]
pub struct PipelineConfig {
    pub input: DeviceId,
    pub output: VirtualCable,
    pub monitor: MonitorSettings,
    pub shift: ShiftSettings,
    pub analysis: AnalysisSettings,
}

/// 音声のスレッドと分析のスレッドを始め、止め、動いている間の変更を受ける。
pub trait AudioPipeline {
    type Error: std::error::Error + Send + Sync + 'static;
    fn start(&mut self, config: PipelineConfig) -> Result<(), Self::Error>;
    fn stop(&mut self) -> Result<(), Self::Error>;
    fn set_shift(&mut self, shift: ShiftSettings) -> Result<(), Self::Error>;
    fn set_analysis(&mut self, analysis: AnalysisSettings) -> Result<(), Self::Error>;
    fn set_monitor(&mut self, monitor: MonitorSettings) -> Result<(), Self::Error>;
}

/// 分析の結果の行き先。
pub trait AnalysisPublisher: Send {
    fn publish(&self, pair: &AnalysisPair);
}

/// 変える前と後の音の行き先。今は分析だけが使う。
pub trait AudioTap: Send {
    fn push(&mut self, before: &[f32], after: &[f32]);
}
