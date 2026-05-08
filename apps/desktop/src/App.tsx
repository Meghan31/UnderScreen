import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

// ─────────────────────────────────────────────────────────────────────────────
// Types
// ─────────────────────────────────────────────────────────────────────────────

interface AppStatus {
  status: string;
  version: string;
  stealth: boolean;
  services: {
    overlay: boolean;
    screenshot: boolean;
    llm: boolean;
  };
}

type OverlayMode = "hidden" | "peek" | "interactive";

// ─────────────────────────────────────────────────────────────────────────────
// Status dot
// ─────────────────────────────────────────────────────────────────────────────

function ServiceDot({ active, label }: { active: boolean; label: string }) {
  return (
    <div className="service-row">
      <span className={`dot ${active ? "dot--on" : "dot--off"}`} />
      <span className="service-label">{label}</span>
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Main overlay component
// ─────────────────────────────────────────────────────────────────────────────

function App() {
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [mode, setMode] = useState<OverlayMode>("peek");

  // ── Fetch initial app status from Rust ──────────────────────────────────
  useEffect(() => {
    invoke<AppStatus>("get_app_status").then(setStatus).catch(console.error);
  }, []);

  // ── Listen for hotkey-driven visibility changes from the Rust backend ───
  // When Cmd+Shift+Space is pressed the Rust layer hides/shows the window.
  // This event lets the React state stay in sync.
  useEffect(() => {
    const unlisten = listen<{ visible: boolean }>("overlay-toggle", (event) => {
      setMode(event.payload.visible ? "peek" : "hidden");
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  // ── Enter interactive mode when the panel is clicked ────────────────────
  // The window starts in click-through (ignoresMouseEvents = true).
  // Pressing Cmd+Shift+Space switches it to interactive mode via Rust.
  // This handler is for the frontend-side visual state only.
  const enterInteractive = useCallback(() => {
    setMode("interactive");
  }, []);

  const exitInteractive = useCallback(async () => {
    setMode("peek");
    // Restore click-through
    await invoke("set_overlay_visible", { visible: false }).catch(console.error);
  }, []);

  return (
    <div className="overlay-root">
      {/* ── Floating panel ─────────────────────────────────────────────── */}
      <div
        className={`panel ${mode === "interactive" ? "panel--interactive" : ""}`}
        onClick={mode === "peek" ? enterInteractive : undefined}
      >
        {/* Header */}
        <div className="panel-header">
          <div className="panel-logo">
            <span className="logo-icon">◈</span>
            <span className="logo-text">UnderScreen</span>
          </div>
          <div className="panel-badge">STEALTH</div>
        </div>

        {/* Divider */}
        <div className="divider" />

        {/* Status block */}
        {status && (
          <div className="status-block">
            <p className="status-section-label">SERVICES</p>
            <ServiceDot active={status.services.overlay} label="Overlay" />
            <ServiceDot active={status.services.screenshot} label="Screenshot" />
            <ServiceDot active={status.services.llm} label="AI Engine" />
          </div>
        )}

        {/* Divider */}
        <div className="divider" />

        {/* Hotkey hints */}
        <div className="hotkeys">
          <p className="status-section-label">SHORTCUTS</p>
          <HotkeyRow keys={["⌘", "⇧", "Space"]} label="Toggle overlay" />
          <HotkeyRow keys={["⌘", "⇧", "S"]} label="Screenshot + AI" dimmed />
          <HotkeyRow keys={["⌘", "⇧", "Q"]} label="Quit" dimmed />
        </div>

        {/* Divider */}
        <div className="divider" />

        {/* Footer */}
        <div className="panel-footer">
          <span className="footer-text">
            Invisible to Zoom · OBS · QuickTime
          </span>
          {mode === "interactive" && (
            <button className="close-btn" onClick={exitInteractive}>
              ✕ dismiss
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

// ── Hotkey badge row ─────────────────────────────────────────────────────────

function HotkeyRow({
  keys,
  label,
  dimmed = false,
}: {
  keys: string[];
  label: string;
  dimmed?: boolean;
}) {
  return (
    <div className={`hotkey-row ${dimmed ? "hotkey-row--dimmed" : ""}`}>
      <div className="key-group">
        {keys.map((k, i) => (
          <kbd key={i} className="key">
            {k}
          </kbd>
        ))}
      </div>
      <span className="hotkey-label">{label}</span>
    </div>
  );
}

export default App;
