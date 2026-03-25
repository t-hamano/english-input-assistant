use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;


/// Cache for audio data to avoid re-requesting the same text.
pub struct AudioCache {
    cache: Mutex<HashMap<String, Vec<u8>>>,
}

impl AudioCache {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn get(&self, text: &str) -> Option<Vec<u8>> {
        self.cache.lock().unwrap().get(text).cloned()
    }

    pub fn insert(&self, text: String, audio: Vec<u8>) {
        self.cache.lock().unwrap().insert(text, audio);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsVoice {
    pub value: &'static str,
    pub label: &'static str,
    pub is_default: bool,
}

pub const ALLOWED_VOICES: &[TtsVoice] = &[
    TtsVoice { value: "en-US-Standard-A", label: "Standard-A (Male)", is_default: true },
    TtsVoice { value: "en-US-Standard-B", label: "Standard-B (Male)", is_default: false },
    TtsVoice { value: "en-US-Standard-C", label: "Standard-C (Female)", is_default: false },
    TtsVoice { value: "en-US-Standard-D", label: "Standard-D (Male)", is_default: false },
    TtsVoice { value: "en-US-Standard-E", label: "Standard-E (Female)", is_default: false },
    TtsVoice { value: "en-US-Standard-F", label: "Standard-F (Female)", is_default: false },
    TtsVoice { value: "en-US-Standard-G", label: "Standard-G (Female)", is_default: false },
    TtsVoice { value: "en-US-Standard-H", label: "Standard-H (Female)", is_default: false },
    TtsVoice { value: "en-US-Standard-I", label: "Standard-I (Male)", is_default: false },
    TtsVoice { value: "en-US-Standard-J", label: "Standard-J (Male)", is_default: false },
];

pub fn default_voice() -> &'static str {
    ALLOWED_VOICES.iter().find(|v| v.is_default).map(|v| v.value).unwrap_or("en-US-Standard-A")
}

pub fn synthesize(api_key: &str, text: &str, voice: &str, speed: f64) -> Result<Vec<u8>, String> {
    if !ALLOWED_VOICES.iter().any(|v| v.value == voice) {
        return Err(format!("Invalid voice: {}", voice));
    }
    if !(0.5..=2.0).contains(&speed) {
        return Err(format!("Invalid speed: {}. Must be between 0.5 and 2.0", speed));
    }

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;
    let url = format!(
        "https://texttospeech.googleapis.com/v1/text:synthesize?key={}",
        api_key
    );

    // Extract language code: "en-US-Standard-C" -> "en-US"
    let language_code = voice.splitn(3, '-').take(2).collect::<Vec<_>>().join("-");

    let body = json!({
        "input": { "text": text },
        "voice": {
            "languageCode": language_code,
            "name": voice
        },
        "audioConfig": {
            "audioEncoding": "MP3",
            "speakingRate": speed
        }
    });

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| format!("Google TTS request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().unwrap_or_default();
        return Err(format!("Google TTS API error {}: {}", status, text));
    }

    let resp: serde_json::Value = response.json().map_err(|e| e.to_string())?;
    let audio_b64 = resp["audioContent"]
        .as_str()
        .ok_or("Missing audioContent in Google TTS response")?;

    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(audio_b64)
        .map_err(|e| format!("Base64 decode error: {}", e))
}

/// A stop signal that can be shared with the playback thread.
pub type StopSignal = Arc<AtomicBool>;

pub fn new_stop_signal() -> StopSignal {
    Arc::new(AtomicBool::new(false))
}

/// Play audio in the current thread, checking the stop signal periodically.
/// Returns when playback finishes or is stopped.
pub fn play_audio(audio_data: &[u8], stop: &StopSignal) -> Result<(), String> {
    use rodio::{Decoder, OutputStream, Sink};
    use std::io::Cursor;
    use std::time::Duration;

    let (_stream, stream_handle) =
        OutputStream::try_default().map_err(|e| format!("Audio output error: {}", e))?;
    let sink =
        Sink::try_new(&stream_handle).map_err(|e| format!("Audio sink error: {}", e))?;

    let cursor = Cursor::new(audio_data.to_vec());
    let source = Decoder::new(cursor).map_err(|e| format!("Audio decode error: {}", e))?;

    sink.append(source);

    while !sink.empty() {
        if stop.load(Ordering::Relaxed) {
            sink.stop();
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    Ok(())
}
