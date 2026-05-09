// Suppress cfg-related warnings that bubble up through the `objc` crate macros.
#![allow(unexpected_cfgs)]

use base64::{engine::general_purpose::STANDARD, Engine};
use std::io::Cursor;

// ─────────────────────────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────────────────────────

/// Everything the React frontend receives after a capture+OCR cycle.
#[derive(serde::Serialize, Clone)]
pub struct ScreenCaptureResult {
    /// Full-screen PNG encoded as standard Base64.
    /// Kept compact: the LLM vision call (Step 3) will consume this directly
    /// without any additional file I/O.
    pub image_base64: String,

    /// Raw text lines recognised by Apple Vision, joined with newlines.
    /// For a coding-question screen this will contain the problem statement,
    /// constraints, examples, and any visible code.
    pub ocr_text: String,

    /// Logical pixel dimensions of the captured display.
    pub width: u32,
    pub height: u32,
}

// ─────────────────────────────────────────────────────────────────────────────
// Entry point (called from the Cmd+Shift+S hotkey thread)
// ─────────────────────────────────────────────────────────────────────────────

/// Capture the primary display and run Apple Vision OCR on the result.
///
/// **Blocking** — always call from `std::thread::spawn` or
/// `tokio::task::spawn_blocking`, never from an async executor thread.
pub fn capture_and_ocr() -> Result<ScreenCaptureResult, String> {
    // 1. Screenshot → raw PNG bytes
    let (png_bytes, width, height) = capture_primary_screen()?;

    // 2. Base64-encode for the LLM vision call later
    let image_base64 = STANDARD.encode(&png_bytes);

    // 3. Apple Vision OCR
    let ocr_text = run_vision_ocr(&png_bytes)?;

    Ok(ScreenCaptureResult {
        image_base64,
        ocr_text,
        width,
        height,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Screenshot via xcap  (uses ScreenCaptureKit on macOS 12.3+)
// ─────────────────────────────────────────────────────────────────────────────

fn capture_primary_screen() -> Result<(Vec<u8>, u32, u32), String> {
    use xcap::Monitor;

    let monitors = Monitor::all().map_err(|e| e.to_string())?;
    if monitors.is_empty() {
        return Err("No monitors detected".into());
    }

    // Primary monitor heuristic: the one closest to origin (0, 0).
    // On macOS, the menu-bar display always lives at (0, 0) in AppKit coords.
    let monitor = monitors
        .iter()
        .min_by_key(|m| (m.x().unsigned_abs() + m.y().unsigned_abs()) as u64)
        .unwrap(); // safe: non-empty

    // capture_image() triggers the ScreenCaptureKit permission prompt on first
    // call if Screen Recording access has not yet been granted.
    let rgba = monitor.capture_image().map_err(|e| e.to_string())?;
    let (w, h) = (rgba.width(), rgba.height());

    // Encode RGBA pixels → PNG in memory (no temp-file I/O).
    let mut png_bytes: Vec<u8> = Vec::new();
    image::DynamicImage::from(rgba)
        .write_to(
            &mut Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .map_err(|e| e.to_string())?;

    Ok((png_bytes, w, h))
}

// ─────────────────────────────────────────────────────────────────────────────
// OCR via Apple Vision framework  (requires macOS 11+; accurate on 12.3+)
//
// Accuracy settings tuned for coding-question screens
// ─────────────────────────────────────────────────────────────────────────────

/// CGRect as defined by CoreGraphics.
/// Vision's `boundingBox` is in normalised coordinates:
///   origin (0,0) = BOTTOM-LEFT,  (1,1) = TOP-RIGHT
/// So a larger `origin.y` means the text is closer to the TOP of the image.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct CGSize {
    width: f64,
    height: f64,
}

#[cfg(target_os = "macos")]
pub fn run_vision_ocr(png_data: &[u8]) -> Result<String, String> {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};
    use std::ffi::CStr;
    use std::os::raw::c_char;

    unsafe {
        // ── NSData from raw PNG bytes ─────────────────────────────────────
        let ns_data: *mut Object = msg_send![
            class!(NSData),
            dataWithBytes: png_data.as_ptr() as *const std::ffi::c_void
            length: png_data.len()
        ];
        if ns_data.is_null() {
            return Err("Failed to create NSData from PNG bytes".into());
        }

        // ── VNImageRequestHandler ─────────────────────────────────────────
        let empty_dict: *mut Object = msg_send![class!(NSDictionary), dictionary];
        let handler: *mut Object = {
            let h: *mut Object = msg_send![class!(VNImageRequestHandler), alloc];
            msg_send![h, initWithData: ns_data options: empty_dict]
        };
        if handler.is_null() {
            return Err("Failed to init VNImageRequestHandler".into());
        }

        // ── VNRecognizeTextRequest ────────────────────────────────────────
        let request: *mut Object = {
            let r: *mut Object = msg_send![class!(VNRecognizeTextRequest), alloc];
            let nil: *mut Object = std::ptr::null_mut();
            msg_send![r, initWithCompletionHandler: nil]
        };
        if request.is_null() {
            return Err("Failed to init VNRecognizeTextRequest".into());
        }

        // ── Accuracy settings ─────────────────────────────────────────────

        // Level 1 = VNRequestTextRecognitionLevelAccurate
        // Uses the full neural-network pass, not the fast heuristic path.
        let _: () = msg_send![request, setRecognitionLevel: 1i64];

        // *** KEY FIX #1 — disable language correction ***
        // Language correction is designed for natural language prose and actively
        // mangles code: "codeVar" → "codeWar", "!=" → "!+", "nullptr" → "nudge",
        // etc.  Turning it off preserves the literal characters Vision sees.
        let _: () = msg_send![request, setUsesLanguageCorrection: objc::runtime::NO];

        // *** KEY FIX #2 — explicit language list ***
        // Auto-detection can misidentify code tokens as other languages and
        // silently switch the recognition model mid-image.  Pinning to English
        // removes that ambiguity.  For multi-language codebases this is still
        // correct because the identifiers and keywords are ASCII English.
        let en_us_cstr = b"en-US\0";
        let en_us_nsstr: *mut Object = msg_send![
            class!(NSString),
            stringWithUTF8String: en_us_cstr.as_ptr() as *const c_char
        ];
        let lang_array: *mut Object =
            msg_send![class!(NSArray), arrayWithObject: en_us_nsstr];
        let _: () = msg_send![request, setRecognitionLanguages: lang_array];

        // *** KEY FIX #3 — minimum text height ***
        // Default is 1/32 of image height (~45 px on a 1440p screen), which
        // silently drops small text in IDEs and terminal windows.
        // 0.0 tells Vision to attempt recognition on every glyph it finds.
        let _: () = msg_send![request, setMinimumTextHeight: 0.0f32];

        // ── Perform (synchronous) ─────────────────────────────────────────
        let requests_array: *mut Object =
            msg_send![class!(NSArray), arrayWithObject: request];
        let mut error: *mut Object = std::ptr::null_mut();
        let ok: bool = msg_send![
            handler,
            performRequests: requests_array
            error: &mut error
        ];

        if !ok {
            let msg = if !error.is_null() {
                let desc: *mut Object = msg_send![error, localizedDescription];
                let ptr: *const c_char = msg_send![desc, UTF8String];
                if ptr.is_null() {
                    "Vision error (no description)".into()
                } else {
                    CStr::from_ptr(ptr).to_string_lossy().into_owned()
                }
            } else {
                "Vision performRequests returned false".into()
            };
            return Err(msg);
        }

        // ── Collect and sort results ──────────────────────────────────────
        // results: NSArray<VNRecognizedTextObservation>
        // Each observation = one line of text + a normalised bounding box.
        let results: *mut Object = msg_send![request, results];
        if results.is_null() {
            return Ok(String::new());
        }

        let count: usize = msg_send![results, count];

        // Gather (y_origin, text, confidence) tuples for post-processing.
        let mut observations: Vec<(f64, String, f32)> = Vec::with_capacity(count);

        for i in 0..count {
            let obs: *mut Object = msg_send![results, objectAtIndex: i];

            // ── Bounding box (for reading-order sort) ─────────────────────
            // boundingBox is a CGRect in normalised coords (0–1).
            // Large structs are returned via a hidden pointer on arm64/x86-64;
            // msg_send! handles this correctly when the return type is declared.
            let bbox: CGRect = msg_send![obs, boundingBox];

            // topCandidates:1 — the single highest-confidence hypothesis.
            let candidates: *mut Object = msg_send![obs, topCandidates: 1usize];
            let n: usize = msg_send![candidates, count];
            if n == 0 {
                continue;
            }

            let candidate: *mut Object = msg_send![candidates, objectAtIndex: 0usize];

            // *** KEY FIX #4 — confidence filtering ***
            // Skip observations where Vision is less than 30% certain.
            // These are almost always hallucinated characters or background
            // noise that end up cluttering the output.
            let confidence: f32 = msg_send![candidate, confidence];
            if confidence < 0.30 {
                continue;
            }

            let ns_str: *mut Object = msg_send![candidate, string];
            let ptr: *const c_char = msg_send![ns_str, UTF8String];
            if ptr.is_null() {
                continue;
            }

            let text = CStr::from_ptr(ptr).to_string_lossy().into_owned();
            if text.trim().is_empty() {
                continue;
            }

            observations.push((bbox.origin.y, text, confidence));
        }

        // *** KEY FIX #5 — sort by bounding-box Y (descending) ***
        // Vision's coordinate system has (0,0) at the BOTTOM-LEFT, so a
        // higher Y value means the text is HIGHER on screen.
        // Sorting descending gives natural top-to-bottom reading order,
        // which is critical for multi-column layouts like LeetCode where
        // the problem statement and constraints sit in separate columns.
        observations.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let text = observations
            .into_iter()
            .map(|(_, s, _)| s)
            .collect::<Vec<_>>()
            .join("\n");

        Ok(text)
    }
}

/// Stub for non-macOS targets so the crate compiles everywhere.
#[cfg(not(target_os = "macos"))]
pub fn run_vision_ocr(_png_data: &[u8]) -> Result<String, String> {
    Err("Apple Vision OCR is only available on macOS".into())
}
