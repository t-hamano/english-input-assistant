use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::OnceLock;

static HTTP_CLIENT: OnceLock<Client> = OnceLock::new();

fn http_client() -> Result<&'static Client, String> {
    if let Some(client) = HTTP_CLIENT.get() {
        return Ok(client);
    }

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    // Keep the connection pool alive across translations and retries.
    // Failed initialization remains retryable.
    Ok(HTTP_CLIENT.get_or_init(|| client))
}

const SYSTEM_PROMPT: &str = r#"英語ライティングアシスタント。入力を自然な英語に変換せよ。日本語入力→英訳、英語入力→より自然に改善。JSON出力: {"translated":"自然な英文","explanation":"必ず日本語で記述。入力が日本語の場合は推奨英文の文法解説、入力が英語の場合は入力英文からの改善点と推奨英文の文法解説","source_is_english":bool}。explanation内で語句を引用する際は必ず日本語の「」を使用し、半角ダブルクォート(")で囲わないこと（JSONが壊れるため）。"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationResult {
    pub translated: String,
    pub explanation: String,
    #[serde(default)]
    pub source_is_english: bool,
}

fn build_user_message(input: &str, additional_prompt: &str) -> String {
    if additional_prompt.is_empty() {
        input.to_string()
    } else {
        format!("{}\n\n[Additional instructions: {}]", input, additional_prompt)
    }
}

fn extract_first_json_object(body: &str) -> Option<&str> {
    let bytes = body.as_bytes();
    let start = bytes.iter().position(|&b| b == b'{')?;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_string {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
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
        Err(e) => {
            if let Some(slice) = extract_first_json_object(body) {
                if let Ok(result) = serde_json::from_str::<TranslationResult>(slice) {
                    return Ok(result);
                }
            }
            Err(format!("Failed to parse LLM response: {}. Raw: {}", e, body))
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
    GeminiModel { value: "gemini-2.5-flash-lite", label: "Gemini 2.5 Flash-Lite", is_default: false },
    GeminiModel { value: "gemini-2.5-flash", label: "Gemini 2.5 Flash", is_default: true },
];

pub fn default_model() -> &'static str {
    ALLOWED_MODELS.iter().find(|m| m.is_default).map(|m| m.value).unwrap_or("gemini-2.5-flash")
}

pub fn translate(api_key: &str, model: &str, input: &str, additional_prompt: &str) -> Result<TranslationResult, String> {
    if !ALLOWED_MODELS.iter().any(|m| m.value == model) {
        return Err(format!("Invalid model: {}", model));
    }

    let client = http_client()?;
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model,
        api_key
    );

    let response_schema = json!({
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
        "required": ["translated", "explanation", "source_is_english"]
    });

    let mut generation_config = json!({
        "temperature": 0,
        "responseMimeType": "application/json",
        "responseSchema": response_schema
    });

    // thinkingConfig is only supported by Gemini 2.5 models
    if model.starts_with("gemini-2.5") {
        generation_config["thinkingConfig"] = json!({ "thinkingBudget": 0 });
    }

    let body = json!({
        "systemInstruction": {
            "parts": [{ "text": SYSTEM_PROMPT }]
        },
        "contents": [
            {
                "role": "user",
                "parts": [{ "text": build_user_message(input, additional_prompt) }]
            }
        ],
        "generationConfig": generation_config
    });

    let response = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| format!("Gemini request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().unwrap_or_default();
        return Err(format!("Gemini API error {}: {}", status, text));
    }

    let resp: serde_json::Value = response.json().map_err(|e| e.to_string())?;
    let content = resp["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .ok_or("Missing content in Gemini response")?;

    parse_response(content)
}
