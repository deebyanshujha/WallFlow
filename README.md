# WallFlow

A lightweight Windows wallpaper manager. It changes your desktop wallpaper on a
schedule, uses as little bandwidth as possible, and keeps working when you are
offline.

## Prerequisites

The frontend builds already. The desktop app needs the Rust toolchain, which is
not yet installed on this machine:

1. **Visual Studio Build Tools** with the "Desktop development with C++" workload
   (provides the MSVC linker Tauri needs) — <https://visualstudio.microsoft.com/visual-cpp-build-tools/>
2. **Rust** via rustup — <https://rustup.rs> (choose the `x86_64-pc-windows-msvc` toolchain)
3. **WebView2 runtime** — already present on Windows 11.

Then:

```powershell
npm install
npm run tauri dev      # run the app
npm run tauri build    # produce dist/ + an NSIS .exe installer
```

## Getting an API key

WallFlow ships with no key and works without one (it will use the bundled and
cached wallpapers). To enable downloads:

- Unsplash: register an app at <https://unsplash.com/developers> and copy the
  *Access Key*.
- Pexels: request a key at <https://www.pexels.com/api/>.

Paste it into **Settings → Wallpaper source → API key**. It is written to the
**Windows Credential Manager** under the service name `WallFlow`. It is never
stored in the database, never written to a config file, and never sent back to
the frontend — the UI can only ever learn *whether* a key exists.

## How it behaves

### The change flow

Every wallpaper change — scheduled, manual, or at launch — runs the same path:

```
is a change due?
   → may we go online? (provider set, key present, no recent failure)
        yes → fetch one photo's metadata → download one resized image
            → decode and validate it → re-encode → save → set wallpaper
        no / failed → newest unused cached wallpaper → set wallpaper
                    → nothing cached → user's default → bundled default
   → record the change, compute the next slot, sleep
```

### Bandwidth

- **One image per change, ever.** No batches, no prefetch, no background polling.
  The scheduler wakes once a minute to compare two integers; it touches the
  network only when a change is actually due.
- **Right-sized downloads.** Screen resolution is read via `GetSystemMetrics` and
  rounded to a request bucket (1280/1600/1920/2560/3840), then passed to the
  provider's CDN so it resizes server-side. A 1080p screen pulls a ~1920px JPEG,
  not a 6000px original.
- **Never the same image twice.** Every downloaded photo's provider id is stored
  and sent as an exclusion list; a duplicate offer is rejected before the image
  body is requested.
- **One failure parks the network.** After a failed attempt, WallFlow will not
  try again until the next scheduled slot. There are no retry loops.

There is deliberately no HTTP response cache: the metadata endpoint is randomised
per request (so an `ETag` would never match) and images are fetched exactly once
and then kept on disk, which is a stronger form of the same saving.

### Offline

Losing the network is a normal state, not an error. WallFlow falls back through
cache → user default → bundled default, shows a plain message about what it did,
and carries on. The bundled fallback means a brand-new install with no internet
still puts something on the desktop.

### Storage

Cached images live in `%LOCALAPPDATA%\com.wallflow.app\wallpapers\`, metadata in
`wallflow.db` alongside it (SQLite). After each download, WallFlow deletes oldest-first until
both the size budget and the keep-recent count are satisfied. **Favorites, the
current wallpaper, and your default are never eligible for deletion** — the
default is stored in a separate directory that the cleanup query cannot reach.

### Security

- Downloads are treated strictly as image data: the payload is fully decoded with
  the `image` crate and re-encoded to JPEG, so what lands on disk is bytes we
  produced. A file that is not a decodable JPEG/PNG is rejected and the cache is
  used instead.
- Filenames are constructed by WallFlow, never taken from the provider, and are
  validated against traversal, separators, and non-ASCII characters.
- Downloads are capped at 25 MB and rejected unless the response is `image/*`.
- HTTPS is enforced at the HTTP client level (`https_only`), and non-HTTPS image
  URLs are refused even if a provider returns one.
- Requests carry only a category keyword. No file paths, machine identifiers, or
  personal data are sent. There is no telemetry or analytics of any kind.
- Nothing downloaded is ever executed.

## Project layout

```
src/                    React control panel (rendering only)
  lib/api.ts            typed wrappers over the Tauri commands
  views/                Overview, History/Favorites, Settings
  components/
src-tauri/
  src/
    engine.rs           the change flow, fallbacks, storage limits, scheduling maths
    scheduler.rs        the one background timer
    commands.rs         the IPC surface exposed to the frontend
    providers/          WallpaperProvider + Unsplash / Pexels / Local
    platform.rs         Windows: wallpaper, screen metrics, run-at-startup
    storage.rs          paths, filename safety, image validation
    secrets.rs          Credential Manager
    db.rs               SQLite schema and queries
  resources/            bundled fallback wallpaper
scripts/gen-assets.mjs  regenerates icons and the fallback wallpaper
```

## Windows notes and current limits

- The wallpaper is set with `SystemParametersInfoW(SPI_SETDESKWALLPAPER)`, which
  applies **one image to all monitors**. Per-monitor wallpapers need the
  `IDesktopWallpaper` COM interface and are not implemented.
- "Start with Windows" writes `HKCU\...\CurrentVersion\Run` with a `--minimized`
  flag. No elevation is required, and the entry is removed when you turn it off.
- Closing the panel hides it to the tray so the scheduler keeps running. Use
  **Quit** in the tray menu to actually exit; while the app is closed entirely,
  no wallpaper changes happen.
- The scheduler does not fire for slots that elapsed while the machine was
  asleep or shut down; it changes the wallpaper at the next launch instead.
