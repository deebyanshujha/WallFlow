import { useState } from "react";
import { Wallpaper, clearTrash, toggleFavorite } from "../lib/api";
import { formatBytes } from "../lib/format";
import WallpaperImage from "../components/WallpaperImage";

const RETENTION_DAYS = 30;

function daysLeftLabel(trashedAt: number): string {
  const elapsed = (Date.now() / 1000 - trashedAt) / 86400;
  const days = Math.max(1, Math.ceil(RETENTION_DAYS - elapsed));
  return days === 1 ? "1 day left" : `${days} days left`;
}

interface Props {
  items: Wallpaper[];
  currentId: string | null;
  refresh: () => Promise<void>;
}

export default function TrashView({ items, currentId, refresh }: Props) {
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);

  async function guard(fn: () => Promise<void>) {
    setError(null);
    try {
      await fn();
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  }

  if (items.length === 0) {
    return (
      <>
        <h1>Trash</h1>
        <div className="empty">
          Trash is empty. Removed favorites stay here for {RETENTION_DAYS} days.
        </div>
      </>
    );
  }

  const inUse = items.some((w) => w.id === currentId);
  const clearable = items.length - (inUse ? 1 : 0);

  return (
    <>
      <h1>Trash</h1>
      {error && <div className="status-line error">{error}</div>}
      <div className="field">
        {confirming ? (
          <div style={{ display: "flex", gap: 4 }}>
            <button
              className="btn danger"
              onClick={() => {
                setConfirming(false);
                void guard(clearTrash);
              }}
            >
              Permanently delete {clearable}
            </button>
            <button className="btn" onClick={() => setConfirming(false)}>
              Cancel
            </button>
          </div>
        ) : (
          <button
            className="btn danger"
            disabled={clearable === 0}
            onClick={() => setConfirming(true)}
          >
            Clear trash
          </button>
        )}
      </div>
      <div className="muted small field">
        Items are deleted automatically after {RETENTION_DAYS} days.
        {inUse && " The wallpaper on your desktop stays here until it is replaced."}
      </div>
      <div className="grid">
        {items.map((w) => (
          <div className="tile" key={w.id}>
            <WallpaperImage fileName={w.file_name} alt={w.category} />
            <div className="meta">
              <div style={{ fontWeight: 600 }}>{w.category}</div>
              <div className="muted small">
                {w.width}×{w.height} · {formatBytes(w.bytes)}
              </div>
              <div className="muted small">
                {w.id === currentId
                  ? "In use"
                  : daysLeftLabel(w.trashed_at ?? 0)}
              </div>
            </div>
            <div className="actions">
              <button
                className="btn grow"
                title="Move back to favorites"
                onClick={() => guard(() => toggleFavorite(w.id, true))}
              >
                Restore
              </button>
            </div>
          </div>
        ))}
      </div>
    </>
  );
}
