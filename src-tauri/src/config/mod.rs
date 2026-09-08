use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub additional_prompt: String,
    pub auto_start: bool,
    #[serde(default = "default_gemini_model")]
    pub gemini_model: String,
    #[serde(default = "default_tts_voice")]
    pub tts_voice: String,
    #[serde(default = "default_tts_speed")]
    pub tts_speed: f64,
    #[serde(default = "default_shortcut")]
    pub shortcut: String,
}

fn default_gemini_model() -> String {
    crate::llm::default_model().to_string()
}

fn default_tts_voice() -> String {
    crate::tts::default_voice().to_string()
}

fn default_tts_speed() -> f64 {
    1.0
}

pub fn default_shortcut() -> String {
    #[cfg(target_os = "macos")]
    {
        "Ctrl+Shift+Space".to_string()
    }
    #[cfg(not(target_os = "macos"))]
    {
        "Ctrl+Alt+Space".to_string()
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            additional_prompt: String::new(),
            auto_start: false,
            gemini_model: default_gemini_model(),
            tts_voice: default_tts_voice(),
            tts_speed: default_tts_speed(),
            shortcut: default_shortcut(),
        }
    }
}

// Only this IPC payload contains the key. AppConfig is safe to persist to disk.
#[derive(Serialize, Deserialize)]
pub struct SettingsConfig {
    #[serde(flatten)]
    pub preferences: AppConfig,
    pub google_api_key: String,
}

pub struct ConfigStore {
    config: Mutex<AppConfig>,
    path: PathBuf,
    credential_lock: Mutex<()>,
}

impl ConfigStore {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let path = app_data_dir.join("config.json");
        let config = if path.exists() {
            match fs::read_to_string(&path) {
                Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
                Err(_) => AppConfig::default(),
            }
        } else {
            AppConfig::default()
        };

        Self {
            config: Mutex::new(config),
            path,
            credential_lock: Mutex::new(()),
        }
    }

    pub fn get(&self) -> AppConfig {
        self.config.lock().unwrap().clone()
    }

    pub fn api_key(&self) -> Result<String, String> {
        let _guard = self.credential_lock.lock().unwrap();
        read_key(&credential()?)
    }

    pub fn settings(&self) -> Result<SettingsConfig, String> {
        let _guard = self.credential_lock.lock().unwrap();
        Ok(SettingsConfig {
            preferences: self.get(),
            google_api_key: read_key(&credential()?)?,
        })
    }

    pub fn update(&self, settings: SettingsConfig) -> Result<(), String> {
        let _guard = self.credential_lock.lock().unwrap();
        let config = settings.preferences;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        let json = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
        let entry = credential()?;
        write_key(&entry, &settings.google_api_key)?;
        fs::write(&self.path, json).map_err(|e| e.to_string())?;
        *self.config.lock().unwrap() = config;
        Ok(())
    }
}

fn credential() -> Result<keyring::Entry, String> {
    keyring::Entry::new("com.tetsu.english-input-assistant", "google-api-key")
        .map_err(|_| "システムの資格情報ストアにアクセスできませんでした。アクセス許可を確認して再試行してください。".into())
}

fn read_key(entry: &keyring::Entry) -> Result<String, String> {
    match entry.get_password() {
        Ok(key) => Ok(key),
        Err(keyring::Error::NoEntry) => Ok(String::new()),
        Err(_) => Err("システムの資格情報ストアから API キーを読み取れませんでした。ロックを解除するかアクセスを許可してから再試行してください。".into()),
    }
}

fn write_key(entry: &keyring::Entry, key: &str) -> Result<(), String> {
    if key.is_empty() {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("システムの資格情報ストアから API キーを削除できませんでした。".into()),
        }
    } else {
        entry.set_password(key)
            .map_err(|_| "システムの資格情報ストアに API キーを保存できませんでした。アクセス許可を確認して再試行してください。".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_payload_keeps_key_out_of_persisted_preferences() {
        let payload = serde_json::json!({
            "google_api_key": "dummy-test-key",
            "additional_prompt": "Use plain English",
            "auto_start": false
        });
        let settings: SettingsConfig = serde_json::from_value(payload).unwrap();
        assert_eq!(settings.google_api_key, "dummy-test-key");
        let persisted = serde_json::to_value(&settings.preferences).unwrap();
        assert!(persisted.get("google_api_key").is_none());
        assert_eq!(persisted["additional_prompt"], "Use plain English");
        let loaded: AppConfig = serde_json::from_value(persisted).unwrap();
        assert_eq!(loaded.gemini_model, default_gemini_model());
    }

    #[test]
    fn plaintext_key_is_not_loaded_from_old_config() {
        let config: AppConfig = serde_json::from_value(serde_json::json!({
            "google_api_key": "old-dummy-key",
            "additional_prompt": "Keep this preference",
            "auto_start": true
        })).unwrap();
        let serialized = serde_json::to_string(&config).unwrap();
        assert!(!serialized.contains("old-dummy-key"));
        assert!(!serialized.contains("google_api_key"));
        assert!(config.auto_start);
        assert_eq!(config.additional_prompt, "Keep this preference");
    }

    // Opt-in: touches only a uniquely named test entry in the real OS store.
    #[test]
    #[ignore = "requires access to the OS credential store"]
    fn native_credential_round_trip() {
        let user = format!("test-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        let entry = keyring::Entry::new("com.tetsu.english-input-assistant.tests", &user).unwrap();
        struct Cleanup(keyring::Entry);
        impl Drop for Cleanup {
            fn drop(&mut self) { let _ = self.0.delete_credential(); }
        }
        let cleanup = Cleanup(entry);
        let entry = &cleanup.0;
        assert!(read_key(entry).unwrap().is_empty());
        write_key(entry, "dummy-test-key").unwrap();
        let reopened = keyring::Entry::new("com.tetsu.english-input-assistant.tests", &user).unwrap();
        assert_eq!(read_key(&reopened).unwrap(), "dummy-test-key");
        write_key(&reopened, "replacement-dummy-key").unwrap();
        assert_eq!(read_key(entry).unwrap(), "replacement-dummy-key");
        write_key(entry, "").unwrap();
        assert!(read_key(entry).unwrap().is_empty());
        write_key(entry, "").unwrap();
    }
}
