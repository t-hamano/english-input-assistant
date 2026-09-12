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
    TtsVoice {
        value: "en-US-Standard-A",
        label: "Standard-A（男性）",
        is_default: true,
    },
    TtsVoice {
        value: "en-US-Standard-B",
        label: "Standard-B（男性）",
        is_default: false,
    },
    TtsVoice {
        value: "en-US-Standard-C",
        label: "Standard-C（女性）",
        is_default: false,
    },
    TtsVoice {
        value: "en-US-Standard-D",
        label: "Standard-D（男性）",
        is_default: false,
    },
    TtsVoice {
        value: "en-US-Standard-E",
        label: "Standard-E（女性）",
        is_default: false,
    },
    TtsVoice {
        value: "en-US-Standard-F",
        label: "Standard-F（女性）",
        is_default: false,
    },
    TtsVoice {
        value: "en-US-Standard-G",
        label: "Standard-G（女性）",
        is_default: false,
    },
    TtsVoice {
        value: "en-US-Standard-H",
        label: "Standard-H（女性）",
        is_default: false,
    },
    TtsVoice {
        value: "en-US-Standard-I",
        label: "Standard-I（男性）",
        is_default: false,
    },
    TtsVoice {
        value: "en-US-Standard-J",
        label: "Standard-J（男性）",
        is_default: false,
    },
    TtsVoice {
        value: "en-US-Chirp3-HD-Aoede",
        label: "Chirp 3 HD Aoede（女性）",
        is_default: false,
    },
    TtsVoice {
        value: "en-US-Chirp3-HD-Kore",
        label: "Chirp 3 HD Kore（女性）",
        is_default: false,
    },
    TtsVoice {
        value: "en-US-Chirp3-HD-Leda",
        label: "Chirp 3 HD Leda（女性）",
        is_default: false,
    },
    TtsVoice {
        value: "en-US-Chirp3-HD-Zephyr",
        label: "Chirp 3 HD Zephyr（女性）",
        is_default: false,
    },
    TtsVoice {
        value: "en-US-Chirp3-HD-Charon",
        label: "Chirp 3 HD Charon（男性）",
        is_default: false,
    },
    TtsVoice {
        value: "en-US-Chirp3-HD-Fenrir",
        label: "Chirp 3 HD Fenrir（男性）",
        is_default: false,
    },
    TtsVoice {
        value: "en-US-Chirp3-HD-Orus",
        label: "Chirp 3 HD Orus（男性）",
        is_default: false,
    },
    TtsVoice {
        value: "en-US-Chirp3-HD-Puck",
        label: "Chirp 3 HD Puck（男性）",
        is_default: false,
    },
];

pub fn default_voice() -> &'static str {
    ALLOWED_VOICES
        .iter()
        .find(|v| v.is_default)
        .map(|v| v.value)
        .unwrap_or("en-US-Standard-A")
}

pub fn synthesize(api_key: &str, text: &str, voice: &str, speed: f64) -> Result<Vec<u8>, String> {
    if !ALLOWED_VOICES.iter().any(|v| v.value == voice) {
        return Err(format!("無効な音声: {}", voice));
    }
    if !(0.5..=2.0).contains(&speed) {
        return Err(format!(
            "無効な速度: {}。0.5〜2.0 の範囲で指定してください",
            speed
        ));
    }

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| format!("HTTP クライアントエラー: {}", e))?;
    let url = "https://texttospeech.googleapis.com/v1/text:synthesize";

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
        .post(url)
        .header(
            "x-goog-api-key",
            crate::google_api::api_key_header(api_key)?,
        )
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| crate::google_api::request_error("Google TTS", e))?;

    if !response.status().is_success() {
        let status = response.status();
        return Err(format!("Google TTS API エラー: HTTP {status}"));
    }

    let resp: serde_json::Value = response
        .json()
        .map_err(|_| "Google TTS のレスポンスを読み取れませんでした。".to_string())?;
    let audio_b64 = resp["audioContent"]
        .as_str()
        .ok_or("Google TTS のレスポンスに audioContent がありません")?;

    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(audio_b64)
        .map_err(|e| format!("Base64 デコードエラー: {}", e))
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
        OutputStream::try_default().map_err(|e| format!("音声出力エラー: {}", e))?;
    let sink = Sink::try_new(&stream_handle).map_err(|e| format!("音声シンクエラー: {}", e))?;

    let cursor = Cursor::new(audio_data.to_vec());
    let source = Decoder::new(cursor).map_err(|e| format!("音声デコードエラー: {}", e))?;

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
