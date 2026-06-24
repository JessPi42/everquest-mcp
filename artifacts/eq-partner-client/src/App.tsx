import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

interface Settings {
  apiKey: string;
  logFilePath: string;
  apiBaseUrl: string;
}

interface AppStatus {
  watching: boolean;
  lastSync: string | null;
  pendingEvents: number;
  lastError: string | null;
  connected: boolean;
}

const DEFAULT_SETTINGS: Settings = {
  apiKey: "",
  logFilePath: "",
  apiBaseUrl: "https://your-compendium.replit.app/api",
};

export default function App() {
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [status, setStatus] = useState<AppStatus>({
    watching: false,
    lastSync: null,
    pendingEvents: 0,
    lastError: null,
    connected: false,
  });
  const [testing, setTesting] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [updateVersion, setUpdateVersion] = useState<string | null>(null);
  const [installing, setInstalling] = useState(false);
  const [applyingUpdate, setApplyingUpdate] = useState(false);
  const [downloadProgress, setDownloadProgress] = useState<{ downloaded: number; total: number | null } | null>(null);

  useEffect(() => {
    invoke<Settings>("load_settings").then(s => {
      if (s) setSettings(s);
    }).catch(() => {});

    const interval = setInterval(() => {
      invoke<AppStatus>("get_status").then(s => setStatus(s)).catch(() => {});
    }, 2000);

    const unlistenUpdateAvailable = listen<string>("update-available", event => {
      setUpdateVersion(event.payload);
    });

    const unlistenProgress = listen<{ downloaded: number; total: number | null }>("update-progress", event => {
      setDownloadProgress(event.payload);
    });

    const unlistenInstalling = listen("update-installing", () => {
      setApplyingUpdate(true);
    });

    return () => {
      clearInterval(interval);
      unlistenUpdateAvailable.then(unlisten => unlisten());
      unlistenProgress.then(unlisten => unlisten());
      unlistenInstalling.then(unlisten => unlisten());
    };
  }, []);

  const handleBrowseLog = async () => {
    const selected = await open({
      multiple: false,
      filters: [{ name: "EQ Log", extensions: ["txt", "log"] }],
    });
    if (typeof selected === "string") {
      setSettings(prev => ({ ...prev, logFilePath: selected }));
    }
  };

  const handleSave = async () => {
    setSaving(true);
    try {
      await invoke("save_settings", { settings });
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      console.error(e);
    } finally {
      setSaving(false);
    }
  };

  const handleTestConnection = async () => {
    setTesting(true);
    try {
      const result = await invoke<{ ok: boolean; username?: string; error?: string }>("test_connection", {
        apiKey: settings.apiKey,
        apiBaseUrl: settings.apiBaseUrl,
      });
      if (result.ok) {
        setStatus(prev => ({ ...prev, connected: true, lastError: null }));
      } else {
        setStatus(prev => ({ ...prev, connected: false, lastError: result.error ?? "Connection failed" }));
      }
    } catch (e) {
      setStatus(prev => ({ ...prev, connected: false, lastError: String(e) }));
    } finally {
      setTesting(false);
    }
  };

  const handleToggleWatch = async () => {
    try {
      if (status.watching) {
        await invoke("stop_watching");
        setStatus(prev => ({ ...prev, watching: false }));
      } else {
        await invoke("start_watching", { logPath: settings.logFilePath });
        setStatus(prev => ({ ...prev, watching: true }));
      }
    } catch (e) {
      setStatus(prev => ({ ...prev, lastError: String(e) }));
    }
  };

  const statusColor = status.lastError ? "#f85149" : status.watching ? "#3fb950" : "#8b949e";
  const statusText = status.lastError ? "Error" : status.watching ? "Watching" : "Idle";

  return (
    <div style={{ padding: "20px", maxWidth: "480px", margin: "0 auto" }}>
      {updateVersion && (
        <div style={{ marginBottom: "12px", padding: "10px 12px", background: "#0d2137", border: "1px solid #1f6feb", borderRadius: "6px" }}>
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: "6px" }}>
            <p style={{ fontSize: "12px", color: "#58a6ff" }}>
              Update available: <strong>v{updateVersion}</strong>
            </p>
            {!installing && (
              <button
                onClick={() => setUpdateVersion(null)}
                style={{ background: "none", border: "none", color: "#8b949e", cursor: "pointer", fontSize: "14px", padding: "0 4px", lineHeight: 1 }}
              >
                ✕
              </button>
            )}
          </div>
          {applyingUpdate ? (
            <div style={{ marginTop: "4px", fontSize: "12px", color: "#58a6ff" }}>Installing…</div>
          ) : installing && downloadProgress ? (
            <div style={{ marginTop: "4px" }}>
              <div style={{ display: "flex", justifyContent: "space-between", marginBottom: "4px" }}>
                <span style={{ fontSize: "11px", color: "#58a6ff" }}>Downloading…</span>
                <span style={{ fontSize: "11px", color: "#8b949e" }}>
                  {downloadProgress.total
                    ? `${Math.round((downloadProgress.downloaded / downloadProgress.total) * 100)}%`
                    : `${(downloadProgress.downloaded / 1024 / 1024).toFixed(1)} MB`}
                </span>
              </div>
              <div style={{ height: "4px", background: "#21262d", borderRadius: "2px", overflow: "hidden" }}>
                <div
                  style={{
                    height: "100%",
                    borderRadius: "2px",
                    background: "#1f6feb",
                    width: downloadProgress.total
                      ? `${Math.round((downloadProgress.downloaded / downloadProgress.total) * 100)}%`
                      : "100%",
                    transition: "width 0.15s ease",
                  }}
                />
              </div>
            </div>
          ) : (
            <button
              onClick={async () => {
                setInstalling(true);
                setDownloadProgress(null);
                try {
                  await invoke("install_update");
                } catch (e) {
                  setInstalling(false);
                  setDownloadProgress(null);
                  setStatus(prev => ({ ...prev, lastError: `Update failed: ${e}` }));
                }
              }}
              disabled={installing}
              style={{ ...btnStyle, background: "#1f6feb", color: "#fff", border: "none", width: "100%" }}
            >
              {installing ? "Installing…" : "Install now"}
            </button>
          )}
        </div>
      )}
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: "20px" }}>
        <div>
          <h1 style={{ fontSize: "18px", fontWeight: 700, color: "#e8b84b", fontFamily: "serif" }}>EQ Partner</h1>
          <p style={{ fontSize: "11px", color: "#8b949e", marginTop: "2px" }}>EverQuest Log Monitor</p>
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: "6px" }}>
          <div style={{ width: "8px", height: "8px", borderRadius: "50%", background: statusColor }} />
          <span style={{ fontSize: "12px", color: statusColor }}>{statusText}</span>
        </div>
      </div>

      <section style={sectionStyle}>
        <h2 style={sectionTitleStyle}>API Settings</h2>
        <div style={fieldStyle}>
          <label style={labelStyle}>Partner API Key</label>
          <input
            type="password"
            placeholder="eqp_..."
            value={settings.apiKey}
            onChange={e => setSettings(prev => ({ ...prev, apiKey: e.target.value }))}
            style={inputStyle}
          />
        </div>
        <div style={fieldStyle}>
          <label style={labelStyle}>Compendium API URL</label>
          <input
            type="text"
            placeholder="https://your-compendium.replit.app/api"
            value={settings.apiBaseUrl}
            onChange={e => setSettings(prev => ({ ...prev, apiBaseUrl: e.target.value }))}
            style={inputStyle}
          />
        </div>
        <div style={{ display: "flex", gap: "8px", marginTop: "8px" }}>
          <button
            onClick={handleTestConnection}
            disabled={testing || !settings.apiKey}
            style={{ ...btnStyle, background: "#21262d", color: "#58a6ff", border: "1px solid #30363d" }}
          >
            {testing ? "Testing…" : "Test Connection"}
          </button>
          {status.connected && <span style={{ fontSize: "11px", color: "#3fb950", alignSelf: "center" }}>✓ Connected</span>}
        </div>
      </section>

      <section style={sectionStyle}>
        <h2 style={sectionTitleStyle}>Log File</h2>
        <div style={fieldStyle}>
          <label style={labelStyle}>EverQuest Log File Path</label>
          <div style={{ display: "flex", gap: "8px" }}>
            <input
              type="text"
              placeholder="C:\\EverQuest\\Logs\\eqlog_Yourchar_server.txt"
              value={settings.logFilePath}
              onChange={e => setSettings(prev => ({ ...prev, logFilePath: e.target.value }))}
              style={{ ...inputStyle, flex: 1 }}
            />
            <button onClick={handleBrowseLog} style={{ ...btnStyle, background: "#21262d", color: "#8b949e", border: "1px solid #30363d", flexShrink: 0 }}>
              Browse…
            </button>
          </div>
          <p style={{ fontSize: "11px", color: "#8b949e", marginTop: "4px" }}>
            Found in your EverQuest folder under Logs/. Enable logging in EQ with /log on.
          </p>
        </div>
      </section>

      <div style={{ display: "flex", gap: "8px", marginBottom: "16px" }}>
        <button
          onClick={handleSave}
          disabled={saving}
          style={{ ...btnStyle, flex: 1, background: "#21262d", color: "#e6edf3", border: "1px solid #30363d" }}
        >
          {saved ? "Saved!" : saving ? "Saving…" : "Save Settings"}
        </button>
        <button
          onClick={handleToggleWatch}
          disabled={!settings.logFilePath || !settings.apiKey}
          style={{
            ...btnStyle, flex: 1,
            background: status.watching ? "#8b1a1a" : "#1a4a1a",
            color: status.watching ? "#f85149" : "#3fb950",
            border: `1px solid ${status.watching ? "#f8514940" : "#3fb95040"}`,
          }}
        >
          {status.watching ? "Stop Watching" : "Start Watching"}
        </button>
      </div>

      {(status.watching || status.pendingEvents > 0 || status.lastSync) && (
        <section style={sectionStyle}>
          <h2 style={sectionTitleStyle}>Activity</h2>
          {status.pendingEvents > 0 && (
            <p style={{ fontSize: "12px", color: "#8b949e" }}>
              <span style={{ color: "#e6edf3" }}>{status.pendingEvents}</span> events queued for sync
            </p>
          )}
          {status.lastSync && (
            <p style={{ fontSize: "12px", color: "#8b949e", marginTop: "4px" }}>
              Last sync: <span style={{ color: "#e6edf3" }}>{status.lastSync}</span>
            </p>
          )}
        </section>
      )}

      {status.lastError && (
        <div style={{ marginTop: "12px", padding: "10px 12px", background: "#2d1111", border: "1px solid #8b1a1a", borderRadius: "6px" }}>
          <p style={{ fontSize: "11px", color: "#f85149" }}>{status.lastError}</p>
        </div>
      )}
    </div>
  );
}

const sectionStyle: React.CSSProperties = {
  background: "#161b22",
  border: "1px solid #30363d",
  borderRadius: "8px",
  padding: "14px",
  marginBottom: "12px",
};

const sectionTitleStyle: React.CSSProperties = {
  fontSize: "12px",
  fontWeight: 600,
  color: "#8b949e",
  textTransform: "uppercase",
  letterSpacing: "0.06em",
  marginBottom: "10px",
};

const fieldStyle: React.CSSProperties = {
  marginBottom: "10px",
};

const labelStyle: React.CSSProperties = {
  display: "block",
  fontSize: "12px",
  color: "#8b949e",
  marginBottom: "4px",
};

const inputStyle: React.CSSProperties = {
  width: "100%",
  padding: "6px 10px",
  background: "#0d1117",
  border: "1px solid #30363d",
  borderRadius: "6px",
  color: "#e6edf3",
  fontSize: "12px",
  outline: "none",
};

const btnStyle: React.CSSProperties = {
  padding: "7px 14px",
  borderRadius: "6px",
  cursor: "pointer",
  fontSize: "12px",
  fontWeight: 500,
  border: "none",
};
