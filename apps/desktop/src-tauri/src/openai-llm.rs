use futures_util::StreamExt;
use tauri::Emitter;

// ─────────────────────────────────────────────────────────────────────────────
// System prompt — tuned for coding interview questions
// ─────────────────────────────────────────────────────────────────────────────

const SYSTEM_PROMPT: &str = "\
You are an expert competitive programmer. The user has captured text from their screen \
containing a coding interview problem.\n\
\n\
Respond in this exact plain-text format (no markdown, no asterisks, no backticks):\n\
\n\
APPROACH\n\
One or two sentences on the optimal strategy and data structure.\n\
\n\
SOLUTION\n\
Complete, working code. Use Python unless a different language is visible on screen.\n\
\n\
COMPLEXITY\n\
Time: O(?)   Space: O(?)\n\
\n\
Rules:\n\
- No filler text, no pleasantries\n\
- Code must be correct, clean, and idiomatic\n\
- If the captured text is not a coding problem, reply with one line explaining what you see\
";

// ─────────────────────────────────────────────────────────────────────────────
// Streaming query
// ─────────────────────────────────────────────────────────────────────────────

/// Call the OpenAI chat completions endpoint with `stream: true` and forward
/// every token to the React overlay as a `llm-token` event.
///
/// Emits:
///   `llm-start`        — signals React to clear the answer area and start streaming
///   `llm-token`        — one string fragment per SSE delta (N times)
///   (caller emits `llm-done` or `pipeline-error` after this returns)
///
/// Requires the `OPENAI_API_KEY` environment variable to be set.
pub async fn query_streaming(
    ocr_text: &str,
    window: &tauri::WebviewWindow,
) -> Result<(), String> {
    // ── API key ───────────────────────────────────────────────────────────────
    let api_key = std::env::var("OPENAI_API_KEY").map_err(|_| {
        "OPENAI_API_KEY not set.\n\
         Add this to your shell profile and relaunch the app:\n\
           export OPENAI_API_KEY=sk-..."
            .to_string()
    })?;

    let trimmed = ocr_text.trim();
    if trimmed.is_empty() {
        return Err(
            "No text was detected on screen.\n\
             Make sure a coding problem is fully visible, then press ⌘⇧S again."
                .into(),
        );
    }

    // ── HTTP client ───────────────────────────────────────────────────────────
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    let body = serde_json::json!({
        "model": "gpt-4o-mini",
        "stream": true,
        "temperature": 0.15,
        "max_tokens": 1800,
        "messages": [
            { "role": "system", "content": SYSTEM_PROMPT },
            {
                "role": "user",
                "content": format!("Text visible on my screen:\n\n{}", trimmed)
            }
        ]
    });

    // ── Send request ──────────────────────────────────────────────────────────
    let response = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Network error: {}", e))?;

    let http_status = response.status();
    if !http_status.is_success() {
        let body_text = response.text().await.unwrap_or_default();
        // Surface the common ones clearly
        return Err(match http_status.as_u16() {
            401 => format!("Invalid API key (401). Check your OPENAI_API_KEY."),
            429 => "Rate limit hit (429). Wait a moment and try again.".into(),
            503 => "OpenAI is temporarily unavailable (503). Try again shortly.".into(),
            _ => format!("OpenAI error {}: {}", http_status.as_u16(), body_text),
        });
    }

    // Signal React to enter streaming mode and clear any previous answer.
    let _ = window.emit("llm-start", ());

    // ── Read SSE stream ───────────────────────────────────────────────────────
    //
    // OpenAI sends newline-delimited SSE chunks:
    //   data: {"choices":[{"delta":{"content":"Hello"},...}],...}
    //   data: [DONE]
    //
    // Multiple SSE lines can arrive in a single TCP/HTTP chunk, and a single
    // SSE line can span multiple chunks.  We keep a line buffer to handle both.
    let mut byte_stream = response.bytes_stream();
    let mut line_buf = String::new();

    'stream: while let Some(chunk_result) = byte_stream.next().await {
        let bytes = chunk_result.map_err(|e| format!("Stream read error: {}", e))?;
        // Use lossy UTF-8 — token content is ASCII/BMP, so this is safe.
        line_buf.push_str(&String::from_utf8_lossy(&bytes));

        // Drain all complete lines from the buffer.
        loop {
            match line_buf.find('\n') {
                None => break, // incomplete line — wait for more bytes
                Some(pos) => {
                    let line = line_buf[..pos].trim().to_string();
                    line_buf = line_buf[pos + 1..].to_string();

                    if let Some(data) = line.strip_prefix("data: ") {
                        let data = data.trim();
                        if data == "[DONE]" {
                            break 'stream;
                        }
                        // Extract the text delta from the JSON payload.
                        if let Ok(json) =
                            serde_json::from_str::<serde_json::Value>(data)
                        {
                            let token = json
                                .pointer("/choices/0/delta/content")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            if !token.is_empty() {
                                let _ = window.emit("llm-token", token);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
