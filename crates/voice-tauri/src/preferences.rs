//! アプリを閉じた後まで覚える値。前に画像を保存したフォルダーだけを持つ。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
pub struct Preferences {
    pub last_image_dir: Option<PathBuf>,
}

pub struct PreferencesStore {
    path: PathBuf,
}

impl PreferencesStore {
    /// `VOICE_SHIFT_SCOPE_CONFIG_DIR` があればそこを、無ければ Tauri がアプリごとに用意する設定のディレクトリを使う。
    pub fn new(default_dir: PathBuf) -> Self {
        let dir = std::env::var_os("VOICE_SHIFT_SCOPE_CONFIG_DIR").map(PathBuf::from).unwrap_or(default_dir);
        Self { path: dir.join("preferences.json") }
    }

    pub fn load(&self) -> Preferences {
        std::fs::read(&self.path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn remember_image_dir(&self, dir: &Path) -> std::io::Result<()> {
        let mut prefs = self.load();
        prefs.last_image_dir = Some(dir.to_path_buf());
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, serde_json::to_vec_pretty(&prefs)?)
    }

    /// 保存先を選ぶ画面で最初に開くフォルダー。前のフォルダーが無くなっていれば `None`（呼ぶ側がピクチャを開く）。
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
        std::fs::remove_dir_all(&root).unwrap();
    }
}
