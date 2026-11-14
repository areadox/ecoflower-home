//! Selbst-Update über GitHub-Releases.
//!
//! Ablauf: neuestes Release abfragen → passende Programmdatei und deren SHA-256
//! herunterladen → Prüfsumme kontrollieren → Datei neben der laufenden ablegen und
//! atomar umbenennen → Prozess per `exec` durch die neue Version ersetzen.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

const REPO: &str = "areadox/ecoflower-home";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Default)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: Option<String>,
    pub available: bool,
    pub asset: Option<String>,
    pub notes: Option<String>,
    pub url: Option<String>,
    pub checked_at: Option<i64>,
    pub error: Option<String>,
    /// Kann dieses Programm sich selbst ersetzen? Sonst steht hier der Grund.
    pub blocked: Option<String>,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    assets: Vec<GhAsset>,
}

#[derive(Deserialize, Clone)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

fn http() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("ecoflower-home/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(120))
        .build()?)
}

fn asset_name() -> String {
    format!("ecoflower-home-linux-{}", std::env::consts::ARCH)
}

/// "0.0.3" bzw. "v0.0.3" → (0, 0, 3)
pub fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.trim().trim_start_matches('v').split('.').map(|p| p.parse::<u64>().ok());
    Some((it.next()??, it.next()??, it.next()??))
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// Warum ein Selbst-Update nicht möglich ist (oder None, wenn es geht).
pub fn update_blocker() -> Option<String> {
    if Path::new("/.dockerenv").exists() || Path::new("/run/.containerenv").exists() {
        return Some("Läuft in einem Container – bitte das Image neu bauen (git pull, dann compose up -d --build).".into());
    }
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => return Some(format!("Programmpfad unbekannt: {e}")),
    };
    let dir = exe.parent().unwrap_or(Path::new("/"));
    let probe = dir.join(".ecoflower-schreibtest");
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            None
        }
        Err(_) => Some(format!(
            "Keine Schreibrechte im Ordner {} – siehe README, Abschnitt „Updates über die Weboberfläche“.",
            dir.display()
        )),
    }
}

pub async fn check() -> UpdateInfo {
    let mut info = UpdateInfo {
        current: CURRENT.into(),
        checked_at: Some(chrono::Utc::now().timestamp()),
        blocked: update_blocker(),
        ..Default::default()
    };
    match fetch_latest().await {
        Ok((rel, asset)) => {
            let latest = rel.tag_name.trim_start_matches('v').to_string();
            info.available = is_newer(&latest, CURRENT) && asset.is_some();
            info.latest = Some(latest);
            info.asset = asset.map(|a| a.name);
            info.notes = rel.body;
            info.url = Some(rel.html_url);
        }
        Err(e) => info.error = Some(e.to_string()),
    }
    info
}

async fn fetch_latest() -> Result<(GhRelease, Option<GhAsset>)> {
    // UPDATE_API_URL nur für Tests (z. B. eigener Server statt GitHub)
    let url = std::env::var("UPDATE_API_URL")
        .unwrap_or_else(|_| format!("https://api.github.com/repos/{REPO}/releases/latest"));
    let resp = http()?.get(&url).header("Accept", "application/vnd.github+json").send().await?;
    if !resp.status().is_success() {
        bail!("GitHub antwortet mit {}", resp.status());
    }
    let rel: GhRelease = resp.json().await?;
    let name = asset_name();
    let asset = rel.assets.iter().find(|a| a.name == name).cloned();
    Ok((rel, asset))
}

/// Neue Version herunterladen, prüfen und an die Stelle der laufenden Datei setzen.
/// Gibt den Pfad der (neuen) Programmdatei zurück.
pub async fn install() -> Result<(String, PathBuf)> {
    if let Some(reason) = update_blocker() {
        bail!(reason);
    }
    let (rel, asset) = fetch_latest().await?;
    let latest = rel.tag_name.trim_start_matches('v').to_string();
    if !is_newer(&latest, CURRENT) {
        bail!("Version {CURRENT} ist bereits aktuell");
    }
    let asset = asset.ok_or_else(|| anyhow!("Im Release fehlt die Datei {}", asset_name()))?;
    let sum_asset = rel
        .assets
        .iter()
        .find(|a| a.name == format!("{}.sha256", asset.name))
        .ok_or_else(|| anyhow!("Im Release fehlt die Prüfsumme {}.sha256", asset.name))?;

    let client = http()?;
    let expected = client.get(&sum_asset.browser_download_url).send().await?.error_for_status()?.text().await?;
    let expected = expected.split_whitespace().next().unwrap_or("").to_lowercase();
    if expected.len() != 64 {
        bail!("Prüfsummendatei ist ungültig");
    }
    let bytes = client
        .get(&asset.browser_download_url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await
        .context("Download fehlgeschlagen")?;
    let actual = hex::encode(Sha256::digest(&bytes));
    if actual != expected {
        bail!("Prüfsumme stimmt nicht – Update abgebrochen");
    }

    let exe = std::env::current_exe()?;
    let tmp = exe.with_file_name(".ecoflower-home.neu");
    std::fs::write(&tmp, &bytes).with_context(|| format!("{} nicht schreibbar", tmp.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    // Sicherung der alten Version behalten
    let backup = exe.with_file_name("ecoflower-home.alt");
    let _ = std::fs::copy(&exe, &backup);
    std::fs::rename(&tmp, &exe).context("Programmdatei konnte nicht ersetzt werden")?;
    Ok((latest, exe))
}

/// Laufenden Prozess durch die (neue) Programmdatei ersetzen. Kehrt nur bei Fehlern zurück.
#[cfg(unix)]
pub fn restart(exe: &Path) -> anyhow::Error {
    use std::os::unix::process::CommandExt;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let err = std::process::Command::new(exe).args(args).exec();
    anyhow!("Neustart fehlgeschlagen: {err}")
}

#[cfg(not(unix))]
pub fn restart(_exe: &Path) -> anyhow::Error {
    anyhow!("Neustart wird nur unter Linux unterstützt")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versionen_vergleichen() {
        assert!(is_newer("0.0.3", "0.0.2"));
        assert!(is_newer("v0.0.10", "0.0.9"));
        assert!(!is_newer("0.0.2", "0.0.2"));
        assert!(!is_newer("0.0.1", "0.0.2"));
        assert!(!is_newer("kaputt", "0.0.2"));
    }
}
