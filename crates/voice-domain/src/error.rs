use thiserror::Error;

/// 値の検査と `VoiceSession` の規則の誤り。
#[derive(Debug, Clone, PartialEq, Error)]
pub enum DomainError {
    #[error("ピッチは {min}〜{max} 半音の範囲で指定してください（{value}）")]
    InvalidPitchShift { value: f32, min: f32, max: f32 },
    #[error("フォルマントの倍率は {min}〜{max} の範囲で指定してください（{value}）")]
    InvalidFormantShift { value: f32, min: f32, max: f32 },
    #[error("フォルマントの上限は {min}〜{max} Hz の範囲で指定してください（{value}）")]
    InvalidFormantCeiling { value: f32, min: f32, max: f32 },
    #[error("倍音のしきい値は {min}〜{max} dB の範囲で指定してください（{value}）")]
    InvalidHarmonicThreshold { value: f32, min: f32, max: f32 },
    #[error("最も強い倍音からの差は {min}〜{max} dB の範囲で指定してください（{value}）")]
    InvalidHarmonicRelativeLimit { value: f32, min: f32, max: f32 },
    #[error("LPC の次数は {min}〜{max} の偶数で指定してください（{value}）")]
    InvalidLpcOrder { value: u32, min: u32, max: u32 },
    #[error("モニターの音量は 0〜1 の範囲で指定してください（{value}）")]
    InvalidMonitorVolume { value: f32 },
    #[error("値が数ではありません")]
    NotFinite,
    #[error("標本化の速さは 0 より大きくしてください")]
    InvalidSampleRate,
    #[error("周波数は 0 より大きくしてください（{value}）")]
    InvalidFrequency { value: f32 },
    #[error("一覧に無い機器です（{0}）")]
    UnknownDevice(String),
    #[error("出力の先（VB-CABLE）の録音側は入力に選べません")]
    CableRecordingAsInput,
    #[error("出力の先（VB-CABLE）の再生側はモニターに選べません")]
    CablePlaybackAsMonitor,
    #[error("出力の先（VB-CABLE）が見つからないので開始できません")]
    NoVirtualCable,
    #[error("入力の機器が選ばれていないので開始できません")]
    NoInputDevice,
    #[error("モニターを入れるには、モニターの機器を選んでください")]
    NoMonitorDevice,
    #[error("動いている間は入力の機器を替えられません")]
    InputChangeWhileRunning,
    #[error("既に動いています")]
    AlreadyRunning,
    #[error("止まっています")]
    NotRunning,
}
