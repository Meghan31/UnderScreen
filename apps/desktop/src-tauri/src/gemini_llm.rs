use futures_util::StreamExt;
use tauri::Emitter;
use tokio::time::sleep;
use std::sync::OnceLock;
use tokio::sync::Semaphore;

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
One or two sentences on the optimal strategy and data structure. also mention the time and space complexity.\n\
\n\
SOLUTION\n\
Complete, working code. Use cpp unless a different language is visible on screen.\n\
\n\
COMPLEXITY\n\
Time: O(?)   Space: O(?)\n\
\n\
Rules:\n\
- No filler text, no pleasantries\n\
- Just menion what approach you would use and then the code solution. Finally, give the time and space complexity of your solution. also what other approaches you could consider no code needed for other approaches\n\
- Code must be correct, clean, and idiomatic\n\
- If the captured text is not a coding problem, reply with one line explaining what you see\
";

// ─────────────────────────────────────────────────────────────────────────────
// API key — reads GEMINI_API_KEY from environment or a .env file
// ─────────────────────────────────────────────────────────────────────────────

/// Try to load a `.env` file and inject KEY=VALUE pairs into the process env.
/// Searches: current directory → user home directory.
/// Silently skips any path that doesn't exist or can't be read.
fn load_dotenv_if_present() {
    // Nothing to do if the key is already in the environment.
    if std::env::var("GEMINI_API_KEY").is_ok() {
        return;
    }

    let candidates = [
        // 1. .env next to the binary / working directory
        std::env::current_dir().ok().map(|d| d.join(".env")),
        // 2. ~/.env — reliable location for a macOS native app
        dirs_next_home().map(|h| h.join(".env")),
    ];

    for path_opt in candidates.iter().flatten() {
        if let Ok(contents) = std::fs::read_to_string(path_opt) {
            parse_and_inject_dotenv(&contents);
            // Re-check after each file; stop as soon as the key is found.
            if std::env::var("GEMINI_API_KEY").is_ok() {
                return;
            }
        }
    }
}

/// Parse a dotenv-style string and set found keys into the process env.
fn parse_and_inject_dotenv(contents: &str) {
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(eq) = line.find('=') {
            let key = line[..eq].trim();
            let mut val = line[eq + 1..].trim().to_string();
            // Strip surrounding quotes
            if val.len() >= 2
                && ((val.starts_with('"') && val.ends_with('"'))
                    || (val.starts_with('\'') && val.ends_with('\'')))
            {
                val = val[1..val.len() - 1].to_string();
            }
            // Safety: single-threaded setup; no other threads touch env at this point.
            #[allow(deprecated)]
            std::env::set_var(key, val);
        }
    }
}

/// Portable home-directory lookup without pulling in additional crates.
fn dirs_next_home() -> Option<std::path::PathBuf> {
    std::env::var("HOME").ok().map(std::path::PathBuf::from)
}

/// Return the Gemini API key, loading `.env` files first if needed.
pub fn get_gemini_api_key() -> Result<String, String> {
    load_dotenv_if_present();
    std::env::var("GEMINI_API_KEY").map_err(|_| {
        "GEMINI_API_KEY not set.\n\
         Option 1 — shell profile (persistent):\n\
           echo 'export GEMINI_API_KEY=your_key' >> ~/.zshrc && source ~/.zshrc\n\
         Option 2 — .env file in your home directory:\n\
           echo 'GEMINI_API_KEY=your_key' >> ~/.env"
            .to_string()
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Streaming query — Gemini 2.0 Flash via Server-Sent Events
// ─────────────────────────────────────────────────────────────────────────────

/// Call the Gemini `streamGenerateContent` endpoint and forward every text
/// delta to the React overlay as an `llm-token` event.
///
/// Emits:
///   `llm-start`       — clears the answer area and enters streaming mode
///   `llm-token` × N  — individual text fragments
///   (caller emits `llm-done` or `pipeline-error` after this returns)
pub async fn query_streaming(
    ocr_text: &str,
    window: &tauri::WebviewWindow,
) -> Result<(), String> {
    // ── API key ───────────────────────────────────────────────────────────────
    let api_key = get_gemini_api_key()?;

    let trimmed = ocr_text.trim();
    if trimmed.is_empty() {
        return Err(
            "No text was detected on screen.\n\
             Make sure a coding problem is fully visible, then press ⌘⇧S again."
                .into(),
        );
    }

    // ── HTTP client (reused) and concurrency control ──────────────────────────
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    static SEM: OnceLock<Semaphore> = OnceLock::new();

    let client = CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .pool_idle_timeout(std::time::Duration::from_secs(15))
            .build()
            .expect("Failed to build HTTP client")
    })
    .clone();

    // Limit concurrent Gemini requests to avoid hitting project-level concurrency limits.
    let sem = SEM.get_or_init(|| Semaphore::new(1));
    let _permit = sem.acquire().await.map_err(|e| format!("Semaphore closed: {}", e))?;

    // Gemini streaming endpoint — ?alt=sse gives Server-Sent Events format.
    // Using gemini-2.0-flash: fastest model with best quality for coding tasks.
    // Use a newer flash-preview model for lower latency; keep SSE streaming.
    let url = "https://generativelanguage.googleapis.com/v1beta/models/\
         gemini-3-flash-preview:streamGenerateContent?alt=sse"
        .to_string();

    let body = serde_json::json!({
        // System instruction sets the assistant persona
        "system_instruction": {
            "parts": [{ "text": SYSTEM_PROMPT }]
        },
        "contents": [
            {
                "role": "user",
                "parts": [{ "text": format!("Text visible on my screen:\n\n{}", trimmed) }]
            }
        ],
        "generationConfig": {
            "temperature": 0.15,
            // Allow full detailed responses
            "maxOutputTokens": 4096
        }
    });

    // Retry loop: handle 429/503 with exponential backoff and respect Retry-After.
    let mut attempt: u32 = 0;
    let max_retries: u32 = 4;
    let response = loop {
        let req = client
            .post(&url)
            .header("Content-Type", "application/json")
            // Prefer header auth to avoid accidental leakage in logs/urls
            .header("x-goog-api-key", api_key.clone())
            .json(&body);

        match req.send().await {
            Err(e) => {
                if attempt >= max_retries {
                    return Err(format!("Network error: {}", e));
                }
                let backoff = std::time::Duration::from_secs(1 << attempt.min(6));
                sleep(backoff).await;
                attempt += 1;
                continue;
            }
            Ok(resp) => {
                let status = resp.status();
                if status.as_u16() == 429 || status.as_u16() == 503 {
                    // Extract Retry-After header and compute wait before consuming resp
                    let hdr_opt = resp
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok());
                    let hdr = hdr_opt.map(|s| s.to_string()).unwrap_or_else(|| "(none)".to_string());
                    let wait_secs = hdr_opt
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or_else(|| 1 << attempt);

                    // Now consume the response body for logging
                    let body_text = resp.text().await.unwrap_or_default();
                    eprintln!(
                        "Gemini {} response on attempt {}: Retry-After={} body={}",
                        status.as_u16(),
                        attempt,
                        hdr,
                        body_text
                    );

                    if attempt >= max_retries {
                        return Err(match status.as_u16() {
                            429 => format!("Rate limit hit (429). Response: {}", body_text),
                            503 => format!("Gemini is temporarily unavailable (503). Response: {}", body_text),
                            _ => format!("Gemini error {}: {}", status.as_u16(), body_text),
                        });
                    }

                    sleep(std::time::Duration::from_secs(wait_secs)).await;
                    attempt += 1;
                    continue;
                }

                break resp;
            }
        }
    };

    let http_status = response.status();
    if !http_status.is_success() {
        let body_text = response.text().await.unwrap_or_default();
        return Err(match http_status.as_u16() {
            400 => format!("Bad request (400): {}", body_text),
            403 => "API key is invalid or restricted (403). Check your GEMINI_API_KEY.".into(),
            429 => "Rate limit hit (429). Wait a moment and try again.".into(),
            503 => "Gemini is temporarily unavailable (503). Try again shortly.".into(),
            _ => format!("Gemini error {}: {}", http_status.as_u16(), body_text),
        });
    }

    // Signal React to clear the previous answer and start receiving tokens.
    let _ = window.emit("llm-start", ());

    // ── Parse SSE stream ──────────────────────────────────────────────────────
    //
    // Each SSE data line is a complete JSON object:
    //   data: {"candidates":[{"content":{"parts":[{"text":"Hello"}],...},...}],...}
    //
    // Unlike OpenAI, Gemini does NOT send a `data: [DONE]` sentinel.
    // The stream simply ends when all candidates have a finishReason set.
    // IMPORTANT: Continue reading until stream fully closes, even after finishReason.
    let mut byte_stream = response.bytes_stream();
    let mut line_buf = String::new();
    let mut token_count = 0;
    let mut seen_finish_reason = false;

    'stream: while let Some(chunk_result) = byte_stream.next().await {
        let bytes = chunk_result.map_err(|e| format!("Stream read error: {}", e))?;
        line_buf.push_str(&String::from_utf8_lossy(&bytes));

        loop {
            match line_buf.find('\n') {
                None => break,
                Some(pos) => {
                    let line = line_buf[..pos].trim().to_string();
                    line_buf = line_buf[pos + 1..].to_string();

                    if let Some(data) = line.strip_prefix("data: ") {
                        let data = data.trim();

                        // OpenAI-style sentinel — handle defensively.
                        if data == "[DONE]" {
                            break 'stream;
                        }

                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                            // Extract text delta: candidates[0].content.parts[0].text
                            let token = json
                                .pointer("/candidates/0/content/parts/0/text")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");

                            if !token.is_empty() {
                                token_count += 1;
                                let _ = window.emit("llm-token", token);
                            }

                            // Note when model signals done, but continue reading entire stream.
                            let finish = json
                                .pointer("/candidates/0/finishReason")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            if !finish.is_empty() && finish != "null" && !seen_finish_reason {
                                eprintln!("Gemini finishReason={} after {} tokens (continuing to read remaining chunks)", finish, token_count);
                                seen_finish_reason = true;
                                // Do NOT break — continue reading remaining stream chunks
                            }
                        }
                    }
                }
            }
        }
    }

    eprintln!("Gemini stream completed: {} total tokens, finishReason seen: {}", token_count, seen_finish_reason);
    Ok(())
}
