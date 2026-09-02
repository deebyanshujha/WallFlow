//! API keys live in the Windows Credential Manager, never in the database,
//! never in a config file, and never in a value returned to the frontend.

use anyhow::{anyhow, Result};
use keyring::Entry;

const SERVICE: &str = "WallFlow";

fn entry(provider: &str) -> Result<Entry> {
    Entry::new(SERVICE, provider).map_err(|e| anyhow!("credential store unavailable: {e}"))
}

pub fn set_api_key(provider: &str, key: &str) -> Result<()> {
    let e = entry(provider)?;
    if key.trim().is_empty() {
        let _ = e.delete_credential();
        return Ok(());
    }
    e.set_password(key.trim())
        .map_err(|e| anyhow!("could not store API key: {e}"))
}

pub fn get_api_key(provider: &str) -> Option<String> {
    entry(provider).ok()?.get_password().ok()
}

pub fn has_api_key(provider: &str) -> bool {
    get_api_key(provider).is_some_and(|k| !k.is_empty())
}
