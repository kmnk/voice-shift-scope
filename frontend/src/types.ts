// Rust の voice-tauri の dto.rs と同じ形。

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
  ceiling: number;
  threshold: number;
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
  monitors: Device[];
  cable: { playback: string; recording: string } | null;
  selectedInput: string | null;
  selectedMonitor: string | null;
  monitorEnabled: boolean;
  monitorVolume: number;
  running: boolean;
}

export type Side = "before" | "after";
