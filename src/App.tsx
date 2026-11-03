import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { AppStatus, getFavorites, getHistory, getStatus, getTrash, Wallpaper } from "./lib/api";
import HomeView from "./views/HomeView";
import HistoryView from "./views/HistoryView";
import SettingsView from "./views/SettingsView";
import TrashView from "./views/TrashView";

type Tab = "home" | "history" | "favorites" | "trash" | "settings";

const TABS: { id: Tab; label: string }[] = [
  { id: "home", label: "Overview" },
  { id: "history", label: "History" },
  { id: "favorites", label: "Favorites" },
  { id: "trash", label: "Trash" },
  { id: "settings", label: "Settings" },
];

export default function App() {
  const [tab, setTab] = useState<Tab>("home");
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [history, setHistory] = useState<Wallpaper[]>([]);
  const [favorites, setFavorites] = useState<Wallpaper[]>([]);
  const [trash, setTrash] = useState<Wallpaper[]>([]);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    const [s, h, f, t] = await Promise.all([
      getStatus(),
      getHistory(),
      getFavorites(),
      getTrash(),
    ]);
    setStatus(s);
    setHistory(h);
    setFavorites(f);
    setTrash(t);
  }, []);

  useEffect(() => {
    void refresh();
    // The backend tells us when something changed; we never poll it.
    const unlistenStatus = listen("wallflow://status-changed", () => void refresh());
    const unlistenNav = listen<string>("wallflow://navigate", (e) => {
      if (e.payload === "settings") setTab("settings");
    });
    return () => {
      void unlistenStatus.then((f) => f());
      void unlistenNav.then((f) => f());
    };
  }, [refresh]);

  useEffect(() => {
    const theme = status?.settings.theme ?? "system";
    const resolved =
      theme === "system"
        ? window.matchMedia("(prefers-color-scheme: dark)").matches
          ? "dark"
          : "light"
        : theme;
    document.documentElement.dataset.theme = resolved;
  }, [status?.settings.theme]);

  if (!status) {
    return <div className="empty">Starting WallFlow…</div>;
  }

  const netLabel = busy || status.network_active ? "Using the internet" : "Idle · no network use";

  return (
    <div className="app">
      <nav className="sidebar">
        <div className="brand">WallFlow</div>
        {TABS.map((t) => (
          <button
            key={t.id}
            className="nav-item"
            aria-current={tab === t.id}
            onClick={() => setTab(t.id)}
          >
            {t.label}
          </button>
        ))}
        <div className="net-badge">
          <span
            className={`dot ${busy || status.network_active ? "active" : ""}`}
          />
          {netLabel}
        </div>
      </nav>

      <main className="main">
        {tab === "home" && (
          <HomeView
            status={status}
            busy={busy}
            setBusy={setBusy}
            setStatus={setStatus}
            refresh={refresh}
          />
        )}
        {(tab === "history" || tab === "favorites") && (
          <HistoryView
            key={tab}
            title={tab === "favorites" ? "Favorites" : "History"}
            items={tab === "favorites" ? favorites : history}
            currentId={status.settings.current_wallpaper_id}
            refresh={refresh}
            setStatus={setStatus}
          />
        )}
        {tab === "trash" && (
          <TrashView
            items={trash}
            currentId={status.settings.current_wallpaper_id}
            refresh={refresh}
          />
        )}
        {tab === "settings" && (
          <SettingsView status={status} setStatus={setStatus} refresh={refresh} />
        )}
      </main>
    </div>
  );
}
