import { invoke } from "@tauri-apps/api/core";
import { convertFileSrc } from "@tauri-apps/api/core";
import { appLocalDataDir, join } from "@tauri-apps/api/path";

export interface Wallpaper {
  id: string;
  provider: string;
  remote_id: string;
  file_name: string;
  width: number;
  height: number;
  source_width: number;
  source_height: number;
  format: string;
  bytes: number;
  downloaded_at: number;
  category: string;
  author_name: string | null;
  author_url: string | null;
  source_url: string | null;
  is_favorite: boolean;
  last_used_at: number | null;
}

export interface Settings {
  provider: string;
  category: string;
  search_query: string;
  interval_hours: number;
  preferred_hour: number;
  keep_recent: number;
  max_storage_mb: number;
  start_with_windows: boolean;
  theme: string;
  default_wallpaper_path: string | null;
  next_change_at: number;
  current_wallpaper_id: string | null;
}

export interface AppStatus {
  settings: Settings;
  current: Wallpaper | null;
  network_active: boolean;
  last_message: string | null;
  last_error: string | null;
  storage_used_bytes: number;
  wallpaper_count: number;
  screen_width: number;
  screen_height: number;
  has_api_key: boolean;
}

export const getStatus = () => invoke<AppStatus>("get_status");
export const getHistory = () => invoke<Wallpaper[]>("get_history");
export const changeNow = () => invoke<AppStatus>("change_now");
export const setWallpaperById = (id: string) =>
  invoke<AppStatus>("set_wallpaper_by_id", { id });
export const toggleFavorite = (id: string, favorite: boolean) =>
  invoke<void>("toggle_favorite", { id, favorite });
export const deleteWallpaper = (id: string) =>
  invoke<void>("delete_wallpaper", { id });
export const saveSettings = (settings: Settings) =>
  invoke<AppStatus>("save_settings", { settings });
export const setDefaultWallpaper = (sourcePath: string) =>
  invoke<AppStatus>("set_default_wallpaper", { sourcePath });
export const useDefaultWallpaper = () => invoke<AppStatus>("use_default_wallpaper");
export const setApiKey = (provider: string, apiKey: string) =>
  invoke<AppStatus>("set_api_key", { provider, apiKey });

let cachedDir: string | null = null;

/** Images are served through Tauri's asset protocol, scoped to our data dir. */
export async function wallpaperSrc(fileName: string): Promise<string> {
  cachedDir ??= await appLocalDataDir();
  return convertFileSrc(await join(cachedDir, "wallpapers", fileName));
}

export async function pathSrc(absolutePath: string): Promise<string> {
  return convertFileSrc(absolutePath);
}
