import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

interface AppStatus {
  status: string;
  version: string;
  services: {
    audio: boolean;
    asr: boolean;
    llm: boolean;
  };
}

function App() {
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [greeting, setGreeting] = useState("");

  useEffect(() => {
    // Test Rust IPC on mount
    invoke<AppStatus>("get_app_status").then(setStatus);
    invoke<string>("greet", { name: "Dev" }).then(setGreeting);
  }, []);

  return (
    <div style={{ padding: "2rem", fontFamily: "monospace" }}>
      <h1>🖥 UnderScreen</h1>
      <p>{greeting}</p>
      {status && (
        <pre>{JSON.stringify(status, null, 2)}</pre>
      )}
    </div>
  );
}

export default App;