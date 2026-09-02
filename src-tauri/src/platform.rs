//! Windows-native integration: wallpaper and screen metrics.
//!
//! Run-at-startup is deliberately NOT handled here — it goes through the
//! official Tauri autostart plugin instead of a hand-rolled registry write.

use anyhow::{anyhow, Result};
use std::path::Path;

#[cfg(windows)]
mod imp {
    use super::*;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{
        GetDC, GetDeviceCaps, ReleaseDC, DESKTOPHORZRES, DESKTOPVERTRES, HORZRES, VERTRES,
    };
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE,
        REG_OPTION_NON_VOLATILE, REG_SZ,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SystemParametersInfoW, SM_CXSCREEN, SM_CYSCREEN, SPIF_SENDCHANGE,
        SPIF_UPDATEINIFILE, SPI_SETDESKWALLPAPER,
    };

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    /// Windows re-encodes whatever we hand it into its own cached
    /// `TranscodedWallpaper` JPEG, at quality ~85 unless told otherwise, and
    /// stretches it according to WallpaperStyle. Left alone, that single step
    /// undoes all the care taken over the download. Setting these makes Windows
    /// keep the image essentially as-is and fill the screen without distorting.
    fn prepare_desktop_registry() -> Result<()> {
        let mut key = HKEY::default();
        let sub = wide(r"Control Panel\Desktop");
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(sub.as_ptr()),
                0,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                None,
                &mut key,
                None,
            )
            .ok()?;

            // Values under Control Panel\Desktop are REG_SZ even when numeric.
            for (name, value) in [
                ("JPEGImportQuality", "100"),
                ("WallpaperStyle", "10"), // Fill: covers the screen, crops, never stretches
                ("TileWallpaper", "0"),
            ] {
                let n = wide(name);
                let v = wide(value);
                let bytes =
                    std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 2);
                let _ = RegSetValueExW(key, PCWSTR(n.as_ptr()), 0, REG_SZ, Some(bytes));
            }
            let _ = RegCloseKey(key);
        }
        Ok(())
    }

    pub fn set_wallpaper(path: &Path) -> Result<()> {
        // Quality settings must be in place before the wallpaper is applied,
        // because Windows transcodes at that moment.
        let _ = prepare_desktop_registry();

        // SPI_SETDESKWALLPAPER needs a real absolute path; a relative one
        // silently clears the desktop instead of failing.
        let abs = path.canonicalize()?;
        let s = abs.to_string_lossy().replace(r"\\?\", "");
        let mut buf = wide(&s);
        unsafe {
            SystemParametersInfoW(
                SPI_SETDESKWALLPAPER,
                0,
                Some(buf.as_mut_ptr() as *mut _),
                SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
            )?;
        }
        Ok(())
    }

    /// True pixel size of the primary display.
    ///
    /// DESKTOPHORZRES/VERTRES report physical pixels even when the process is
    /// not DPI-aware, so a 4K screen at 150% scaling still reports 3840x2160
    /// rather than the 2560x1440 that GetSystemMetrics would report.
    pub fn primary_resolution() -> (u32, u32) {
        unsafe {
            let dc = GetDC(None);
            if !dc.is_invalid() {
                let w = GetDeviceCaps(dc, DESKTOPHORZRES);
                let h = GetDeviceCaps(dc, DESKTOPVERTRES);
                // Fall back to the scaled size if the physical query fails.
                let (w, h) = if w > 0 && h > 0 {
                    (w, h)
                } else {
                    (GetDeviceCaps(dc, HORZRES), GetDeviceCaps(dc, VERTRES))
                };
                ReleaseDC(None, dc);
                if w > 0 && h > 0 {
                    return (w as u32, h as u32);
                }
            }
            let w = GetSystemMetrics(SM_CXSCREEN);
            let h = GetSystemMetrics(SM_CYSCREEN);
            if w > 0 && h > 0 {
                (w as u32, h as u32)
            } else {
                (1920, 1080)
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub fn set_wallpaper(_path: &Path) -> Result<()> {
        Err(anyhow!("setting the wallpaper is only supported on Windows"))
    }
    pub fn primary_resolution() -> (u32, u32) {
        (1920, 1080)
    }
}

pub fn set_wallpaper(path: &Path) -> Result<()> {
    if !path.is_file() {
        return Err(anyhow!("wallpaper file does not exist: {}", path.display()));
    }
    imp::set_wallpaper(path)
}

pub fn primary_resolution() -> (u32, u32) {
    imp::primary_resolution()
}
