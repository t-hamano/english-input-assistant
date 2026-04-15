use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub google_api_key: String,
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
    String::new()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            google_api_key: String::new(),
            additional_prompt: String::new(),
            auto_start: false,
            gemini_model: default_gemini_model(),
            tts_voice: default_tts_voice(),
            tts_speed: default_tts_speed(),
            shortcut: default_shortcut(),
        }
    }
}

impl AppConfig {
    pub fn api_key(&self) -> &str {
        &self.google_api_key
    }
}

pub struct ConfigStore {
    config: Mutex<AppConfig>,
    path: PathBuf,
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
        }
    }

    pub fn get(&self) -> AppConfig {
        self.config.lock().unwrap().clone()
    }

    pub fn update(&self, config: AppConfig) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        let json = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
        fs::write(&self.path, json).map_err(|e| e.to_string())?;
        *self.config.lock().unwrap() = config;
        Ok(())
    }
}
