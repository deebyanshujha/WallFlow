import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AppStatus,
  Settings,
  saveSettings,
  setApiKey,
  setDefaultWallpaper,
} from "../lib/api";
import { formatBytes } from "../lib/format";

interface Props {
  status: AppStatus;
  setStatus: (s: AppStatus) => void;
  refresh: () => Promise<void>;
}

const PROVIDERS = [
  { id: "wallhaven", label: "Wallhaven (no key needed)", keyOptional: true },
  { id: "unsplash", label: "Unsplash", keyOptional: false },
  { id: "pexels", label: "Pexels", keyOptional: false },
  { id: "local", label: "Local only (never downloads)", keyOptional: false },
];

// "" asks the provider for anything at all — its widest random pool.
const CATEGORIES = [
  "",
  "nature",
  "landscape",
  "mountains",
  "ocean",
  "forest",
  "city",
  "architecture",
  "minimal",
  "space",
  "abstract",
  "cars",
  "anime",
  "digital art",
  "dark",
];

const categoryLabel = (c: string) => (c ? c : "Anything (surprise me)");

const INTERVALS = [
  { hours: 24, label: "Once per day" },
  { hours: 12, label: "Every 12 hours" },
  { hours: 6, label: "Every 6 hours" },
  { hours: 168, label: "Once per week" },
];

export default function SettingsView({ status, setStatus, refresh }: Props) {
  const [draft, setDraft] = useState<Settings>(status.settings);
  const [apiKeyDraft, setApiKeyDraft] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => setDraft(status.settings), [status.settings]);

  const dirty = JSON.stringify(draft) !== JSON.stringify(status.settings);
  const searching = draft.source_mode === "search";
  const searchMissing = searching && !draft.search_query.trim();
  const keyOptional = PROVIDERS.find((p) => p.id === draft.provider)?.keyOptional ?? false;

  function update<K extends keyof Settings>(key: K, value: Settings[K]) {
    setDraft((d) => ({ ...d, [key]: value }));
  }

  async function guard(fn: () => Promise<string>) {
    setError(null);
    setMessage(null);
    try {
      setMessage(await fn());
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  }

  const onSave = () =>
    guard(async () => {
      setStatus(await saveSettings(draft));
      return "Settings saved.";
    });

  const onChooseDefault = () =>
    guard(async () => {
      const picked = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "Images", extensions: ["jpg", "jpeg", "png"] }],
      });
      if (typeof picked !== "string") return "No image chosen.";
      setStatus(await setDefaultWallpaper(picked));
      return "Default wallpaper updated.";
    });

  const onSaveKey = () =>
    guard(async () => {
      setStatus(await setApiKey(draft.provider, apiKeyDraft));
      setApiKeyDraft("");
      return "API key stored in Windows Credential Manager.";
    });

  const defaultName = draft.default_wallpaper_path
    ? draft.default_wallpaper_path.split(/[\\/]/).pop()
    : null;

  return (
    <>
      <h1>Settings</h1>
      {error && <div className="status-line error">{error}</div>}
      {message && <div className="status-line">{message}</div>}

      <div className="card">
        <h2>Wallpaper source</h2>
        <div className="field">
          <label>Provider</label>
          <select
            value={draft.provider}
            onChange={(e) => update("provider", e.target.value)}
          >
            {PROVIDERS.map((p) => (
              <option key={p.id} value={p.id}>
                {p.label}
              </option>
            ))}
          </select>
        </div>

        <div className="field">
          <label>Find wallpapers by</label>
          <div className="segmented" role="tablist">
            <button
              role="tab"
              aria-selected={!searching}
              className={searching ? "" : "active"}
              onClick={() => update("source_mode", "category")}
            >
              Category
            </button>
            <button
              role="tab"
              aria-selected={searching}
              className={searching ? "active" : ""}
              onClick={() => update("source_mode", "search")}
            >
              My own search
            </button>
          </div>

          {searching ? (
            <input
              type="text"
              placeholder="e.g. blue cars, rainy tokyo street, cyberpunk"
              maxLength={100}
              autoFocus
              value={draft.search_query}
              onChange={(e) => update("search_query", e.target.value)}
            />
          ) : (
            <select
              value={draft.category}
              onChange={(e) => update("category", e.target.value)}
            >
              {CATEGORIES.map((c) => (
                <option key={c} value={c}>
                  {categoryLabel(c)}
                </option>
              ))}
            </select>
          )}
          <div className="muted small" style={{ marginTop: 4 }}>
            {searching
              ? searchMissing
                ? "Type what you want to see."
                : `Next wallpaper will be searched for "${draft.search_query.trim()}".`
              : draft.category
                ? `Next wallpaper will come from the "${draft.category}" category.`
                : "Next wallpaper can be anything — the widest random mix."}{" "}
            Sent only when a wallpaper is actually fetched.
          </div>
        </div>

        {draft.provider !== "local" && (
          <div className="field">
            <label>
              API key{keyOptional ? " (optional)" : ""}{" "}
              {status.has_api_key
                ? "· a key is stored for this provider"
                : keyOptional
                  ? "· not needed for safe-for-work wallpapers"
                  : "· not set"}
            </label>
            <div className="row">
              <input
                type="password"
                placeholder="Paste your API key"
                value={apiKeyDraft}
                onChange={(e) => setApiKeyDraft(e.target.value)}
              />
              <button className="btn" onClick={onSaveKey} disabled={!apiKeyDraft}>
                Store
              </button>
            </div>
            <div className="muted small" style={{ marginTop: 4 }}>
              Stored in Windows Credential Manager. It is never shown again or sent anywhere
              except to the provider.
            </div>
          </div>
        )}
      </div>

      <div className="card">
        <h2>Schedule</h2>
        <div className="field-row">
          <div className="field">
            <label>Change wallpaper</label>
            <select
              value={draft.interval_hours}
              onChange={(e) => update("interval_hours", Number(e.target.value))}
            >
              {INTERVALS.map((i) => (
                <option key={i.hours} value={i.hours}>
                  {i.label}
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <label>Preferred hour (daily or longer)</label>
            <input
              type="number"
              min={0}
              max={23}
              value={draft.preferred_hour}
              onChange={(e) => update("preferred_hour", Number(e.target.value))}
            />
          </div>
        </div>
      </div>

      <div className="card">
        <h2>Default wallpaper</h2>
        <div className="row spread">
          <div>
            <div className="muted small">Used whenever nothing has been downloaded yet.</div>
            <div style={{ marginTop: 4 }}>
              {defaultName ?? "Built-in wallpaper"}
            </div>
          </div>
          <button className="btn" onClick={onChooseDefault}>
            {defaultName ? "Change default" : "Choose image"}
          </button>
        </div>
      </div>

      <div className="card">
        <h2>Storage</h2>
        <div className="field-row">
          <div className="field">
            <label>Keep recent wallpapers</label>
            <input
              type="number"
              min={1}
              max={200}
              value={draft.keep_recent}
              onChange={(e) => update("keep_recent", Number(e.target.value))}
            />
          </div>
          <div className="field">
            <label>Maximum storage (MB)</label>
            <input
              type="number"
              min={50}
              max={20000}
              step={50}
              value={draft.max_storage_mb}
              onChange={(e) => update("max_storage_mb", Number(e.target.value))}
            />
          </div>
        </div>
        <div className="muted small">
          Current usage: {formatBytes(status.storage_used_bytes)}. Favorites, the current
          wallpaper, and your default are never deleted.
        </div>
      </div>

      <div className="card">
        <h2>Application</h2>
        <label className="toggle field">
          <input
            type="checkbox"
            checked={draft.start_with_windows}
            onChange={(e) => update("start_with_windows", e.target.checked)}
            style={{ width: "auto" }}
          />
          Start WallFlow with Windows (minimised to the tray)
        </label>
        <div className="field">
          <label>Theme</label>
          <select value={draft.theme} onChange={(e) => update("theme", e.target.value)}>
            <option value="system">Match Windows</option>
            <option value="light">Light</option>
            <option value="dark">Dark</option>
          </select>
        </div>
      </div>

      <div className="row">
        <button className="btn primary" onClick={onSave} disabled={!dirty || searchMissing}>
          {dirty ? "Save settings" : "Saved"}
        </button>
        {dirty && (
          <span className="muted small">
            {searchMissing
              ? "Enter a search, or switch back to Category."
              : "You have unsaved changes."}
          </span>
        )}
      </div>
    </>
  );
}
