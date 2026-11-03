//! Values remembered after the app closes: the folder an image was last saved to, the screen language, the view (full or compact),
//! the input, output, and monitor devices, the analysis settings, and the pitch and formant (the user chose to remember devices and analysis values
//! on 2026-10-18, and the pitch and formant on 2026-11-02).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
pub struct Preferences {
    pub last_image_dir: Option<PathBuf>,
    /// "en" or "ja". Opens in English if absent.
    #[serde(default)]
    pub language: Option<String>,
    /// "full" (with plots) or "compact" (a voice changer only). Full if absent.
    #[serde(default)]
    pub view: Option<String>,
    #[serde(default)]
    pub devices: RememberedDevices,
    #[serde(default)]
    pub analysis: Option<RememberedAnalysis>,
    #[serde(default)]
    pub shift: Option<RememberedShift>,
}

/// Devices are remembered by their cpal id. If one is not in the list at start, the default device is used.
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct RememberedDevices {
    pub input: Option<String>,
    pub output: Option<String>,
    pub monitor: Option<String>,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub struct RememberedAnalysis {
    pub ceiling: f32,
    pub threshold: f32,
    pub relative_limit: f32,
    pub lpc_order: u32,
}

/// Pitch (semitones) and formant (factor). If out of range at start, the default (0 semitones, ×1.0) is kept.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct RememberedShift {
    pub semitones: f32,
    pub formant: f32,
}

pub struct PreferencesStore {
    path: PathBuf,
}

impl PreferencesStore {
    /// Uses `VOICE_SHIFT_SCOPE_CONFIG_DIR` if set, otherwise the per-app config directory Tauri provides.
    pub fn new(default_dir: PathBuf) -> Self {
        let dir = std::env::var_os("VOICE_SHIFT_SCOPE_CONFIG_DIR").map(PathBuf::from).unwrap_or(default_dir);
        Self { path: dir.join("preferences.json") }
    }

    pub fn load(&self) -> Preferences {
        std::fs::read(&self.path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn save(&self, prefs: &Preferences) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, serde_json::to_vec_pretty(prefs)?)
    }

    pub fn remember_image_dir(&self, dir: &Path) -> std::io::Result<()> {
        let mut prefs = self.load();
        prefs.last_image_dir = Some(dir.to_path_buf());
        self.save(&prefs)
    }

    /// The screen language. Accepts only "en" or "ja".
    pub fn language(&self) -> &'static str {
        match self.load().language.as_deref() {
            Some("ja") => "ja",
            _ => "en",
        }
    }

    pub fn remember_devices(&self, devices: RememberedDevices) -> std::io::Result<()> {
        let mut prefs = self.load();
        prefs.devices = devices;
        self.save(&prefs)
    }

    pub fn remember_analysis(&self, analysis: RememberedAnalysis) -> std::io::Result<()> {
        let mut prefs = self.load();
        prefs.analysis = Some(analysis);
        self.save(&prefs)
    }

    pub fn remember_shift(&self, shift: RememberedShift) -> std::io::Result<()> {
        let mut prefs = self.load();
        prefs.shift = Some(shift);
        self.save(&prefs)
    }

    pub fn view(&self) -> &'static str {
        match self.load().view.as_deref() {
            Some("compact") => "compact",
            _ => "full",
        }
    }

    pub fn remember_view(&self, view: &str) -> std::io::Result<()> {
        let mut prefs = self.load();
        prefs.view = Some(if view == "compact" { "compact" } else { "full" }.to_string());
        self.save(&prefs)
    }

    pub fn remember_language(&self, language: &str) -> std::io::Result<()> {
        let mut prefs = self.load();
        prefs.language = Some(if language == "ja" { "ja" } else { "en" }.to_string());
        self.save(&prefs)
    }

    /// The folder the save dialog opens first. `None` if the previous folder is gone (the caller opens Pictures).
    pub fn image_dir(&self) -> Option<PathBuf> {
        self.load().last_image_dir.filter(|d| d.is_dir())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_last_dir_and_forgets_missing_one() {
        let root = std::env::temp_dir().join(format!("vss-prefs-{}", std::process::id()));
        let images = root.join("images");
        std::fs::create_dir_all(&images).unwrap();
        let store = PreferencesStore { path: root.join("preferences.json") };
        assert!(store.image_dir().is_none());
        store.remember_image_dir(&images).unwrap();
        assert_eq!(store.image_dir().as_deref(), Some(images.as_path()));
        std::fs::remove_dir_all(&images).unwrap();
        assert!(store.image_dir().is_none());
        // The language starts as English and, once remembered, is kept along with the previous image folder
        assert_eq!(store.language(), "en");
        store.remember_language("ja").unwrap();
        assert_eq!(store.language(), "ja");
        assert_eq!(store.load().last_image_dir.as_deref(), Some(images.as_path()));
        assert_eq!(store.view(), "full");
        store.remember_view("compact").unwrap();
        assert_eq!(store.view(), "compact");
        assert_eq!(store.language(), "ja");
        store
            .remember_devices(RememberedDevices { input: Some("mic".into()), output: Some("cable".into()), monitor: None })
            .unwrap();
        store.remember_analysis(RememberedAnalysis { ceiling: 5000.0, threshold: 12.0, relative_limit: 35.0, lpc_order: 10 }).unwrap();
        let loaded = store.load();
        assert_eq!(loaded.devices.output.as_deref(), Some("cable"));
        assert_eq!(loaded.analysis.map(|a| a.ceiling), Some(5000.0));
        assert_eq!(loaded.view.as_deref(), Some("compact"));
        // The pitch and formant are absent at first and, once remembered, are kept along with the other values
        assert_eq!(loaded.shift, None);
        store.remember_shift(RememberedShift { semitones: -12.0, formant: 0.8 }).unwrap();
        let loaded = store.load();
        assert_eq!(loaded.shift, Some(RememberedShift { semitones: -12.0, formant: 0.8 }));
        assert_eq!(loaded.analysis.map(|a| a.ceiling), Some(5000.0));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
