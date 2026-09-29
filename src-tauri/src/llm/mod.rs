use serde::{Deserialize, Serialize};
use serde_json::json;
mod stream;

const SYSTEM_PROMPT: &str = r#"英語ライティングアシスタント。入力を自然な英語に変換せよ。日本語入力→英訳、英語入力→より自然に改善。JSON出力: {"source_is_english":bool,"translated":"自然な英文","explanation":"必ず日本語で記述。入力が日本語の場合は推奨英文の文法解説、入力が英語の場合は入力英文からの改善点と推奨英文の文法解説"}。出力順序は source_is_english、translated、explanation。英文を確定してから解説を生成すること。explanation内で語句を引用する際は必ず日本語の「」を使用し、半角ダブルクォート(")で囲わないこと（JSONが壊れるため）。"#;

const EXPLAIN_PROMPT: &str = r#"英文読解アシスタント。入力された英文を自然な日本語に翻訳し、解説せよ。JSON出力: {"translated":"自然な日本語訳","explanation":"必ず日本語で記述。英文の構文、重要な語句・イディオムの意味と使い方の解説"}。出力順序は translated、explanation。日本語訳を確定してから解説を生成すること。explanation内で語句を引用する際は必ず日本語の「」を使用し、半角ダブルクォート(")で囲わないこと（JSONが壊れるため）。"#;

/// What to do with the captured text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    /// Turn editable text into natural English that can replace it.
    Rewrite,
    /// Translate text that cannot be replaced into Japanese and explain it.
    Explain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationResult {
    pub translated: String,
    pub explanation: String,
    #[serde(default)]
    pub source_is_english: bool,
}

/// A previous result and the user's feedback on it, sent as follow-up turns.
#[derive(Debug, Clone, Deserialize)]
pub struct Refinement {
    pub previous: TranslationResult,
    pub feedback: String,
}

fn build_user_message(input: &str, additional_prompt: &str) -> String {
    if additional_prompt.is_empty() {
        input.to_string()
    } else {
        format!(
            "{}\n\n[Additional instructions: {}]",
            input, additional_prompt
        )
    }
}

fn build_refinement_message(feedback: &str) -> String {
    format!(
        "上記の推奨英文と解説に対するフィードバック:\n{}\n\nこのフィードバックに基づいて推奨英文を改善し、改善後の推奨英文に対する解説を新たに生成せよ。source_is_english は最初の入力に基づいて判定すること。",
        feedback
    )
}

fn build_contents(
    input: &str,
    additional_prompt: &str,
    refinement: Option<&Refinement>,
) -> Result<serde_json::Value, String> {
    let mut contents = vec![json!({
        "role": "user",
        "parts": [{ "text": build_user_message(input, additional_prompt) }]
    })];
    if let Some(refinement) = refinement {
        let previous = serde_json::to_string(&refinement.previous)
            .map_err(|_| "前回の翻訳結果を送信できませんでした。".to_string())?;
        contents.push(json!({ "role": "model", "parts": [{ "text": previous }] }));
        contents.push(json!({
            "role": "user",
            "parts": [{ "text": build_refinement_message(&refinement.feedback) }]
        }));
    }
    Ok(json!(contents))
}

/// Tracks whether a byte-by-byte JSON scan is inside a string, consuming
/// escapes and closing quotes so callers only see structural bytes that lie
/// outside strings. Shared by `extract_first_json_object` and
/// `stream::translation_prefix`, which each track bracket depth differently.
#[derive(Default)]
struct JsonStringTracker {
    in_string: bool,
    escaped: bool,
}

impl JsonStringTracker {
    /// Feeds one byte. Returns `true` if the byte is inside a string (or is
    /// the quote that opened/closed one) and is therefore not structural.
    fn consume(&mut self, byte: u8) -> bool {
        if self.in_string {
            if self.escaped {
                self.escaped = false;
            } else if byte == b'\\' {
                self.escaped = true;
            } else if byte == b'"' {
                self.in_string = false;
            }
            return true;
        }
        if byte == b'"' {
            self.in_string = true;
            return true;
        }
        false
    }
}

fn extract_first_json_object(body: &str) -> Option<&str> {
    let bytes = body.as_bytes();
    let start = bytes.iter().position(|&b| b == b'{')?;
    let mut depth = 0i32;
    let mut tracker = JsonStringTracker::default();
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if tracker.consume(b) {
            continue;
        }
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&body[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_response(body: &str) -> Result<TranslationResult, String> {
    match serde_json::from_str::<TranslationResult>(body.trim()) {
        Ok(result) => Ok(result),
        Err(_) => {
            if let Some(slice) = extract_first_json_object(body) {
                if let Ok(result) = serde_json::from_str::<TranslationResult>(slice) {
                    return Ok(result);
                }
            }
            Err("LLM のレスポンスを解析できませんでした。".to_string())
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiModel {
    pub value: &'static str,
    pub label: &'static str,
    pub is_default: bool,
}

pub const ALLOWED_MODELS: &[GeminiModel] = &[
    GeminiModel {
        value: "gemini-3.5-flash-lite",
        label: "Gemini 3.5 Flash-Lite",
        is_default: false,
    },
    GeminiModel {
        value: "gemini-3.8-flash",
        label: "Gemini 3.8 Flash",
        is_default: true,
    },
];

pub fn default_model() -> &'static str {
    ALLOWED_MODELS
        .iter()
        .find(|m| m.is_default)
        .map(|m| m.value)
        .unwrap_or("gemini-3.8-flash")
}

#[allow(clippy::too_many_arguments)]
pub fn translate(
    api_key: &str,
    model: &str,
    mode: Mode,
    input: &str,
    additional_prompt: &str,
    refinement: Option<&Refinement>,
    on_translation: impl FnMut(TranslationResult),
    is_current: impl Fn() -> bool,
) -> Result<TranslationResult, String> {
    if !ALLOWED_MODELS.iter().any(|m| m.value == model) {
        return Err(format!("無効なモデル: {}", model));
    }

    let client = crate::google_api::http_client()?;
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse",
        model
    );

    let (system_prompt, response_schema, additional_prompt) = match mode {
        Mode::Rewrite => (
            SYSTEM_PROMPT,
            json!({
                "type": "object",
                "properties": {
                    "translated": {
                        "type": "string",
                        "description": "自然な英文"
                    },
                    "explanation": {
                        "type": "string",
                        "description": "日本語での解説"
                    },
                    "source_is_english": {
                        "type": "boolean",
                        "description": "入力が英語であるかどうか"
                    }
                },
                "required": ["translated", "explanation", "source_is_english"],
                "propertyOrdering": ["source_is_english", "translated", "explanation"]
            }),
            additional_prompt,
        ),
        // Additional instructions are about the English the user writes.
        Mode::Explain => (
            EXPLAIN_PROMPT,
            json!({
                "type": "object",
                "properties": {
                    "translated": {
                        "type": "string",
                        "description": "自然な日本語訳"
                    },
                    "explanation": {
                        "type": "string",
                        "description": "日本語での解説"
                    }
                },
                "required": ["translated", "explanation"],
                "propertyOrdering": ["translated", "explanation"]
            }),
            "",
        ),
    };

    // Keep latency low. Gemini 3.x cannot disable thinking entirely, only lower it.
    let generation_config = json!({
        "temperature": 0,
        "responseMimeType": "application/json",
        "responseSchema": response_schema,
        "thinkingConfig": { "thinkingLevel": "low" }
    });

    let body = json!({
        "systemInstruction": {
            "parts": [{ "text": system_prompt }]
        },
        "contents": build_contents(input, additional_prompt, refinement)?,
        "generationConfig": generation_config
    });

    let response = client
        .post(&url)
        .header(
            "x-goog-api-key",
            crate::google_api::api_key_header(api_key)?,
        )
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| crate::google_api::request_error("Gemini", e))?;

    if !response.status().is_success() {
        let status = response.status();
        return Err(format!("Gemini API エラー: HTTP {status}"));
    }

    stream::read_response(
        std::io::BufReader::new(response),
        on_translation,
        is_current,
    )
}

const WORD_PROMPT: &str =
    "英文中の語句を日本語に訳せ。文脈に合った訳だけを簡潔に出力し、説明や引用符は付けないこと。";

/// Translates a phrase selected from the suggested English, using the sentence as context.
pub fn translate_word(
    api_key: &str,
    model: &str,
    word: &str,
    sentence: &str,
) -> Result<String, String> {
    if !ALLOWED_MODELS.iter().any(|m| m.value == model) {
        return Err(format!("無効なモデル: {}", model));
    }

    let client = crate::google_api::http_client()?;
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
        model
    );
    let body = json!({
        "systemInstruction": {
            "parts": [{ "text": WORD_PROMPT }]
        },
        "contents": [
            {
                "role": "user",
                "parts": [{ "text": format!("英文: {}\n語句: {}", sentence, word) }]
            }
        ],
        "generationConfig": {
            "temperature": 0,
            "thinkingConfig": { "thinkingLevel": "low" }
        }
    });

    let response = client
        .post(&url)
        .header(
            "x-goog-api-key",
            crate::google_api::api_key_header(api_key)?,
        )
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| crate::google_api::request_error("Gemini", e))?;

    if !response.status().is_success() {
        let status = response.status();
        return Err(format!("Gemini API エラー: HTTP {status}"));
    }

    let response: serde_json::Value = response
        .json()
        .map_err(|_| "LLM のレスポンスを解析できませんでした。".to_string())?;
    let text = response["candidates"][0]["content"]["parts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|part| part["thought"].as_bool() != Some(true))
        .filter_map(|part| part["text"].as_str())
        .collect::<String>();
    let text = text.trim();
    if text.is_empty() {
        return Err("Gemini が空の翻訳を返しました".to_string());
    }
    Ok(text.to_string())
}
