import { useState } from "react";
import {
  AppStatus,
  Wallpaper,
  deleteWallpaper,
  setWallpaperById,
  toggleFavorite,
} from "../lib/api";
import { formatBytes, formatDate } from "../lib/format";
import WallpaperImage from "../components/WallpaperImage";

interface Props {
  title: string;
  items: Wallpaper[];
  currentId: string | null;
  refresh: () => Promise<void>;
  setStatus: (s: AppStatus) => void;
}

export default function HistoryView({
  title,
  items,
  currentId,
  refresh,
  setStatus,
}: Props) {
  const [error, setError] = useState<string | null>(null);

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
        <h1>{title}</h1>
        <div className="empty">
          Nothing here yet. Downloaded wallpapers appear here and stay available offline.
        </div>
      </>
    );
  }

  return (
    <>
      <h1>{title}</h1>
      {error && <div className="status-line error">{error}</div>}
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
                {formatDate(w.downloaded_at)}
                {w.source_width > 0 && ` · source ${w.source_width}×${w.source_height}`}
              </div>
              <div className="muted small">
                {w.provider}
                {w.author_name ? ` · ${w.author_name}` : ""}
              </div>
            </div>
            <div className="actions">
              <button
                className="btn icon"
                title={w.is_favorite ? "Remove from favorites" : "Add to favorites"}
                onClick={() => guard(() => toggleFavorite(w.id, !w.is_favorite))}
              >
                {w.is_favorite ? "★" : "☆"}
              </button>
              <button
                className="btn grow"
                disabled={w.id === currentId}
                onClick={() =>
                  guard(async () => {
                    setStatus(await setWallpaperById(w.id));
                  })
                }
              >
                {w.id === currentId ? "In use" : "Set"}
              </button>
              <button
                className="btn icon danger"
                title="Delete"
                disabled={w.id === currentId}
                onClick={() => guard(() => deleteWallpaper(w.id))}
              >
                ✕
              </button>
            </div>
          </div>
        ))}
      </div>
    </>
  );
}
