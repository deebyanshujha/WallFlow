import { AppStatus, changeNow, useDefaultWallpaper } from "../lib/api";
import { formatBytes, formatSchedule } from "../lib/format";
import WallpaperImage from "../components/WallpaperImage";

interface Props {
  status: AppStatus;
  busy: boolean;
  setBusy: (b: boolean) => void;
  setStatus: (s: AppStatus) => void;
  refresh: () => Promise<void>;
}

export default function HomeView({ status, busy, setBusy, setStatus, refresh }: Props) {
  const { settings, current } = status;
  const usedPct = Math.min(
    100,
    (status.storage_used_bytes / (settings.max_storage_mb * 1024 * 1024)) * 100
  );

  async function onChangeNow() {
    setBusy(true);
    try {
      setStatus(await changeNow());
      await refresh();
    } finally {
      setBusy(false);
    }
  }

  async function onUseDefault() {
    setStatus(await useDefaultWallpaper());
    await refresh();
  }

  const statusClass = status.last_error
    ? "status-line error"
    : busy || status.network_active
    ? "status-line busy"
    : "status-line";

  return (
    <>
      <h1>Overview</h1>

      <div className={statusClass}>
        {busy || status.network_active
          ? "Downloading one wallpaper…"
          : status.last_error ?? status.last_message ?? "Ready."}
      </div>

      <div className="card">
        <WallpaperImage
          className="preview"
          fileName={current?.file_name ?? null}
          alt="Current wallpaper"
        />
        <div className="row spread" style={{ marginTop: 12 }}>
          <div>
            <div style={{ fontWeight: 600 }}>
              {current ? current.category : "Default wallpaper"}
            </div>
            <div className="muted small">
              {current
                ? `${current.width}×${current.height} · ${current.provider}` +
                  (current.author_name ? ` · ${current.author_name}` : "")
                : "Your fallback image is in use."}
            </div>
          </div>
          <div className="row">
            <button className="btn" onClick={onUseDefault}>
              Use default
            </button>
            <button className="btn primary" onClick={onChangeNow} disabled={busy}>
              {busy ? "Working…" : "Change now"}
            </button>
          </div>
        </div>
      </div>

      <div className="card">
        <div className="row spread">
          <span className="muted">Next scheduled change</span>
          <strong>{formatSchedule(settings.next_change_at)}</strong>
        </div>
        <div className="row spread" style={{ marginTop: 6 }}>
          <span className="muted">Source</span>
          <strong>
            {settings.provider}
            {!status.has_api_key && !["local", "wallhaven"].includes(settings.provider)
              ? " · no API key"
              : ""}
          </strong>
        </div>
        <div className="row spread" style={{ marginTop: 6 }}>
          <span className="muted">
            {settings.source_mode === "search" && settings.search_query.trim()
              ? "Search"
              : "Category"}
          </span>
          <strong>
            {settings.source_mode === "search" && settings.search_query.trim()
              ? `"${settings.search_query.trim()}"`
              : settings.category || "Anything"}
          </strong>
        </div>
        <div className="row spread" style={{ marginTop: 6 }}>
          <span className="muted">Downloading at</span>
          <strong>
            {status.screen_width}×{status.screen_height}
          </strong>
        </div>
      </div>

      <div className="card">
        <h2>Storage</h2>
        <div className="meter">
          <span
            className={usedPct >= 100 ? "full" : ""}
            style={{ width: `${usedPct}%` }}
          />
        </div>
        <div className="row spread small muted">
          <span>
            {formatBytes(status.storage_used_bytes)} of {settings.max_storage_mb} MB ·{" "}
            {status.wallpaper_count} wallpapers
          </span>
          <span>keeping {settings.keep_recent} recent</span>
        </div>
      </div>
    </>
  );
}
