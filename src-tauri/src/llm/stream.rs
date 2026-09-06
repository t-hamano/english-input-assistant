use super::{parse_response, TranslationResult};
use serde::Deserialize;
use serde_json::Value;
use std::io::BufRead;

#[derive(Deserialize)]
struct TranslationPrefix {
    source_is_english: bool,
    translated: String,
}

// Only accept complete top-level fields, never an unfinished JSON string.
// Serde decodes escaped quotes, backslashes and Unicode for us.
fn translation_prefix(text: &str) -> Option<TranslationResult> {
    let text = text.trim_start();
    if !text.starts_with('{') {
        return None;
    }
    let mut depth = 0;
    let mut in_string = false;
    let mut escaped = false;
    for (i, byte) in text.bytes().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth -= 1,
            _ => {}
        }
        if (byte == b',' && depth == 1) || (byte == b'}' && depth == 0) {
            let prefix = format!("{}}}", &text[..i]);
            if let Ok(prefix) = serde_json::from_str::<TranslationPrefix>(&prefix) {
                if !prefix.translated.trim().is_empty() {
                    return Some(TranslationResult {
                        translated: prefix.translated,
                        source_is_english: prefix.source_is_english,
                        explanation: String::new(),
                    });
                }
            }
        }
    }
    None
}

#[derive(Default)]
struct StreamResponse {
    text: String,
    preview: Option<TranslationResult>,
    finished: bool,
}

impl StreamResponse {
    fn push(
        &mut self,
        data: &str,
        on_translation: &mut impl FnMut(TranslationResult),
    ) -> Result<(), String> {
        if data.trim().is_empty() || data.trim() == "[DONE]" {
            return Ok(());
        }
        let chunk: Value = serde_json::from_str(data)
            .map_err(|e| format!("Invalid Gemini stream event: {}", e))?;
        if chunk.get("error").is_some() {
            return Err("Gemini returned a stream error.".to_string());
        }
        if chunk["promptFeedback"]["blockReason"].as_str().is_some() {
            return Err("Gemini blocked the request.".to_string());
        }
        let candidate = &chunk["candidates"][0];
        if let Some(reason) = candidate["finishReason"].as_str() {
            if reason != "STOP" {
                return Err("Gemini generation stopped before completion.".to_string());
            }
            self.finished = true;
        }
        if let Some(parts) = candidate["content"]["parts"].as_array() {
            for part in parts {
                if part["thought"].as_bool() != Some(true) {
                    if let Some(text) = part["text"].as_str() {
                        self.text.push_str(text);
                    }
                }
            }
        }
        if self.preview.is_none() {
            if let Some(preview) = translation_prefix(&self.text) {
                on_translation(preview.clone());
                self.preview = Some(preview);
            }
        }
        Ok(())
    }

    fn finish(self) -> Result<TranslationResult, String> {
        if !self.finished {
            return Err("Gemini stream ended before generation completed".to_string());
        }
        let result = parse_response(&self.text)?;
        if result.translated.trim().is_empty() {
            return Err("Gemini returned an empty translation".to_string());
        }
        if let Some(preview) = self.preview {
            if preview.translated != result.translated
                || preview.source_is_english != result.source_is_english
            {
                return Err("Gemini changed the translation after it was displayed".to_string());
            }
        }
        Ok(result)
    }
}

pub(super) fn read_response(
    mut reader: impl BufRead,
    mut on_translation: impl FnMut(TranslationResult),
    is_current: impl Fn() -> bool,
) -> Result<TranslationResult, String> {
    let mut response = StreamResponse::default();
    let mut line = String::new();
    let mut data = String::new();
    loop {
        if !is_current() {
            return Err("Translation cancelled".to_string());
        }
        line.clear();
        let count = reader
            .read_line(&mut line)
            .map_err(|_| "Gemini stream read failed.".to_string())?;
        if !is_current() {
            return Err("Translation cancelled".to_string());
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if count == 0 || trimmed.is_empty() {
            response.push(&data, &mut on_translation)?;
            data.clear();
            if count == 0 {
                break;
            }
        } else if let Some(value) = trimmed.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(value.strip_prefix(' ').unwrap_or(value));
        }
    }
    response.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_error_details_are_not_exposed() {
        for data in [
            r#"{"error":{"message":"dummy-secret-api-key"}}"#,
            r#"{"promptFeedback":{"blockReason":"dummy-secret-api-key"}}"#,
            r#"{"candidates":[{"finishReason":"dummy-secret-api-key"}]}"#,
        ] {
            let input = format!("data: {data}\n\n");
            let error = read_response(std::io::Cursor::new(input), |_| {}, || true).unwrap_err();
            assert!(!error.contains("dummy-secret-api-key"));
        }
    }

    #[test]
    fn malformed_translation_does_not_echo_raw_content() {
        let error = parse_response(r#"{"translated": "dummy-secret-api-key"}"#).unwrap_err();
        assert!(!error.contains("dummy-secret-api-key"));
    }
}
