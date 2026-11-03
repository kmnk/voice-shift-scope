// The same shapes as dto.rs in Rust's voice-tauri.

export interface Formant {
  frequency: number;
  bandwidth: number;
}

export interface Harmonic {
  number: number;
  frequency: number;
  level: number;
  observed: boolean;
}

export interface Frame {
  f0: number | null;
  voiced: boolean;
  formants: (Formant | null)[];
  harmonics: Harmonic[];
  observedHarmonics: number;
  spectrum: number[];
}

export interface Settings {
  semitones: number;
  formant: number;
  /// Ceiling of the original analysis (Hz)
  ceiling: number;
  /// Ceiling of the shifted analysis (Hz), raised by the formant factor
  afterCeiling: number;
  threshold: number;
  relativeLimit: number;
}

export interface Pair {
  time: number;
  before: Frame;
  after: Frame;
  settings: Settings;
}

export interface Device {
  id: string;
  name: string;
  isDefault: boolean;
}

export interface Devices {
  inputs: Device[];
  outputs: Device[];
  monitors: Device[];
  cable: { playback: string; recording: string } | null;
  selectedInput: string | null;
  selectedOutput: string | null;
  selectedMonitor: string | null;
  monitorEnabled: boolean;
  monitorVolume: number;
  outputSource: "shifted" | "original";
  monitorSource: "shifted" | "original";
  running: boolean;
  analysis: { ceiling: number; threshold: number; relativeLimit: number };
  /** The current pitch (semitones) and formant (factor). At start, the remembered values */
  shift: { semitones: number; formant: number };
}

export type Side = "before" | "after";
