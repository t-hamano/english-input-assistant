use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

const SYSTEM_PROMPT: &str = r#"英語ライティングアシスタント。入力を自然な英語に変換せよ。日本語入力→英訳、英語入力→より自然に改善。JSON出力: {"translated":"自然な英文","grammar":"文法解説（日本語）","improvements":"改善点（日本語）","source_is_english":bool}"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationResult {
    pub translated: String,
    pub grammar: String,
    pub improvements: String,
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

fn parse_response(body: &str) -> Result<TranslationResult, String> {
    let json_str = if let Some(start) = body.find('{') {
        if let Some(end) = body.rfind('}') {
            &body[start..=end]
        } else {
            body
        }
    } else {
        body
    };

    serde_json::from_str::<TranslationResult>(json_str)
        .map_err(|e| format!("Failed to parse LLM response: {}. Raw: {}", e, body))
}

pub fn translate(api_key: &str, model: &str, input: &str, additional_prompt: &str) -> Result<TranslationResult, String> {
    let client = Client::new();
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model,
        api_key
    );

    let mut generation_config = json!({
        "temperature": 0,
        "responseMimeType": "application/json"
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
