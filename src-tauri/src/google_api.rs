use reqwest::header::HeaderValue;

pub fn api_key_header(api_key: &str) -> Result<HeaderValue, String> {
    let mut value = HeaderValue::from_str(api_key)
        .map_err(|_| "API キーの形式が無効です。設定でキーを確認してください。".to_string())?;
    value.set_sensitive(true);
    Ok(value)
}

// Do not expose URLs, headers, or nested transport errors to the UI or logs.
pub fn request_error(service: &str, error: reqwest::Error) -> String {
    let reason = if error.is_timeout() {
        "リクエストがタイムアウトしました"
    } else if error.is_connect() {
        "接続できませんでした"
    } else {
        "リクエストを完了できませんでした"
    };
    format!("{service} リクエストに失敗しました: {reason}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_sent_in_sensitive_header_not_url_or_debug_output() {
        let key = "dummy-secret-api-key";
        let client = reqwest::blocking::Client::new();
        for url in [
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
            "https://texttospeech.googleapis.com/v1/text:synthesize",
        ] {
            let request = client.post(url)
                .header("x-goog-api-key", api_key_header(key).unwrap())
                .build().unwrap();
            assert_eq!(request.headers()["x-goog-api-key"], key);
            assert!(request.headers()["x-goog-api-key"].is_sensitive());
            assert!(!request.url().as_str().contains(key));
            assert!(!format!("{request:?}").contains(key));
        }
    }

    #[test]
    fn invalid_header_and_transport_errors_do_not_expose_secrets() {
        let key = "dummy-secret-api-key";
        let invalid = format!("{key}\r\nInjected: value");
        assert!(!api_key_header(&invalid).unwrap_err().contains(key));
        let url = format!("https://example.invalid/?key={key}");
        let error = reqwest::blocking::Client::new().post(&url)
            .header("x-goog-api-key", invalid).build().unwrap_err()
            .with_url(url.parse().unwrap());
        let message = request_error("Gemini", error);
        assert!(message.starts_with("Gemini リクエストに失敗しました:"));
        assert!(!message.contains(key));
        assert!(!message.contains("example.invalid"));
    }
}
