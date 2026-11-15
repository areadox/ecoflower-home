//! Stromzähler Shelly Pro 3EM – direkt im Heimnetz über die lokale RPC-Schnittstelle.
//!
//! `GET http://<IP>/rpc/EM.GetStatus?id=0` liefert `total_act_power` in W
//! (positiv = Bezug aus dem Netz, negativ = Einspeisung). Ist die Anmeldung am
//! Shelly eingeschaltet, wird HTTP-Digest mit SHA-256 und Benutzer „admin“ verwendet.

use crate::db;
use crate::AppState;
use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Default, Serialize)]
pub struct MeterStatus {
    pub configured: bool,
    pub ok: bool,
    /// Gesamtleistung in W: positiv = Bezug, negativ = Einspeisung
    pub power_w: Option<f64>,
    /// Leistung je Phase (L1, L2, L3)
    pub phases: Vec<f64>,
    pub ts: Option<i64>,
    pub error: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct MeterConfig {
    pub host: String,
    pub password: String,
    pub interval_secs: u64,
}

pub fn load_config(conn: &rusqlite::Connection) -> MeterConfig {
    let get = |k: &str| db::get_setting(conn, k).ok().flatten().unwrap_or_default();
    MeterConfig {
        host: get("shelly_host").trim().to_string(),
        password: get("shelly_password"),
        interval_secs: get("shelly_interval").parse().unwrap_or(2u64).clamp(1, 60),
    }
}

fn base_url(host: &str) -> String {
    let h = host.trim().trim_end_matches('/');
    if h.starts_with("http://") || h.starts_with("https://") {
        h.to_string()
    } else {
        format!("http://{h}")
    }
}

fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

/// Wert eines Parameters aus einem `WWW-Authenticate: Digest …`-Header.
fn digest_param(header: &str, name: &str) -> Option<String> {
    let key = format!("{name}=");
    let start = header.find(&key)? + key.len();
    let rest = &header[start..];
    if let Some(stripped) = rest.strip_prefix('"') {
        stripped.split('"').next().map(String::from)
    } else {
        rest.split(',').next().map(|s| s.trim().to_string())
    }
}

/// Antwort für HTTP-Digest (SHA-256, qop=auth) berechnen.
pub fn digest_authorization(www_auth: &str, password: &str, method: &str, uri: &str, cnonce: &str) -> Result<String> {
    let realm = digest_param(www_auth, "realm").ok_or_else(|| anyhow!("Digest ohne realm"))?;
    let nonce = digest_param(www_auth, "nonce").ok_or_else(|| anyhow!("Digest ohne nonce"))?;
    let nc = "00000001";
    let ha1 = sha256_hex(&format!("admin:{realm}:{password}"));
    let ha2 = sha256_hex(&format!("{method}:{uri}"));
    let response = sha256_hex(&format!("{ha1}:{nonce}:{nc}:{cnonce}:auth:{ha2}"));
    Ok(format!(
        "Digest username=\"admin\", realm=\"{realm}\", nonce=\"{nonce}\", uri=\"{uri}\", algorithm=SHA-256, response=\"{response}\", qop=auth, nc={nc}, cnonce=\"{cnonce}\""
    ))
}

async fn rpc_get(client: &reqwest::Client, cfg: &MeterConfig, path: &str) -> Result<Value> {
    let url = format!("{}{}", base_url(&cfg.host), path);
    let resp = client.get(&url).send().await.map_err(|e| anyhow!("Shelly nicht erreichbar ({}): {e}", cfg.host))?;
    let resp = if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        if cfg.password.is_empty() {
            bail!("Der Shelly verlangt ein Passwort");
        }
        let www = resp
            .headers()
            .get(reqwest::header::WWW_AUTHENTICATE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let cnonce = format!("{:016x}", rand::random::<u64>());
        let auth = digest_authorization(&www, &cfg.password, "GET", path, &cnonce)?;
        let r = client.get(&url).header(reqwest::header::AUTHORIZATION, auth).send().await?;
        if r.status() == reqwest::StatusCode::UNAUTHORIZED {
            bail!("Passwort für den Shelly ist falsch");
        }
        r
    } else {
        resp
    };
    if !resp.status().is_success() {
        bail!("Shelly antwortet mit {}", resp.status());
    }
    Ok(resp.json().await?)
}

/// Einmal lesen: zuerst dreiphasig (EM), sonst einphasige Kanäle (EM1) aufsummiert.
pub async fn read_once(cfg: &MeterConfig) -> Result<MeterStatus> {
    if cfg.host.is_empty() {
        bail!("Keine IP-Adresse eingetragen");
    }
    let client = reqwest::Client::builder().timeout(Duration::from_secs(5)).build()?;
    let mut status = MeterStatus { configured: true, ok: true, ts: Some(chrono::Utc::now().timestamp()), ..Default::default() };

    if let Ok(info) = rpc_get(&client, cfg, "/rpc/Shelly.GetDeviceInfo").await {
        status.model = info.get("model").or_else(|| info.get("app")).and_then(|v| v.as_str()).map(String::from);
    }

    match rpc_get(&client, cfg, "/rpc/EM.GetStatus?id=0").await {
        Ok(v) if v.get("total_act_power").is_some() => {
            status.power_w = v["total_act_power"].as_f64();
            status.phases = ["a_act_power", "b_act_power", "c_act_power"]
                .iter()
                .filter_map(|k| v.get(*k).and_then(|x| x.as_f64()))
                .collect();
            Ok(status)
        }
        Ok(_) | Err(_) => {
            // Einphasiges Profil: EM1-Kanäle 0..2
            let mut phases = Vec::new();
            for id in 0..3 {
                match rpc_get(&client, cfg, &format!("/rpc/EM1.GetStatus?id={id}")).await {
                    Ok(v) => {
                        if let Some(p) = v.get("act_power").and_then(|x| x.as_f64()) {
                            phases.push(p)
                        }
                    }
                    Err(e) if id == 0 => return Err(e),
                    Err(_) => break,
                }
            }
            if phases.is_empty() {
                bail!("Keine Leistungswerte vom Shelly erhalten – ist es ein Shelly Pro 3EM?");
            }
            status.power_w = Some(phases.iter().sum());
            status.phases = phases;
            Ok(status)
        }
    }
}

/// Endlosschleife: Zähler im eingestellten Takt lesen.
pub async fn run(state: Arc<AppState>) {
    let mut last_error: Option<String> = None;
    let mut last_store = 0i64;
    loop {
        let cfg = {
            let conn = state.db.lock().unwrap();
            load_config(&conn)
        };
        if cfg.host.is_empty() {
            *state.meter.lock().unwrap() = MeterStatus::default();
            state.meter_restart.notified().await;
            continue;
        }
        match read_once(&cfg).await {
            Ok(st) => {
                let now = st.ts.unwrap_or_default();
                if last_error.take().is_some() {
                    let conn = state.db.lock().unwrap();
                    db::log_event(&conn, "", "info", "Stromzähler (Shelly) antwortet wieder");
                }
                // Verlauf alle 30 Sekunden speichern
                if now - last_store >= 30 {
                    let conn = state.db.lock().unwrap();
                    let _ = conn.execute(
                        "INSERT INTO meter_samples (ts, power_w) VALUES (?1, ?2)",
                        rusqlite::params![now, st.power_w],
                    );
                    last_store = now;
                }
                *state.meter.lock().unwrap() = st;
            }
            Err(e) => {
                let msg = e.to_string();
                if last_error.as_deref() != Some(msg.as_str()) {
                    let conn = state.db.lock().unwrap();
                    db::log_event(&conn, "", "warn", &format!("Stromzähler (Shelly): {msg}"));
                }
                let mut m = state.meter.lock().unwrap();
                m.configured = true;
                m.ok = false;
                m.error = Some(msg.clone());
                last_error = Some(msg);
            }
        }
        let wait = if last_error.is_some() { Duration::from_secs(30) } else { Duration::from_secs(cfg.interval_secs) };
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = state.meter_restart.notified() => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_parameter() {
        let h = r#"Digest qop="auth", realm="shellypro3em-abc", nonce="60dc59c6", algorithm=SHA-256"#;
        assert_eq!(digest_param(h, "realm").as_deref(), Some("shellypro3em-abc"));
        assert_eq!(digest_param(h, "nonce").as_deref(), Some("60dc59c6"));
        assert_eq!(digest_param(h, "algorithm").as_deref(), Some("SHA-256"));
    }

    #[test]
    fn digest_antwort() {
        let h = r#"Digest qop="auth", realm="shellypro3em-abc", nonce="60dc59c6", algorithm=SHA-256"#;
        let a = digest_authorization(h, "geheim", "GET", "/rpc/EM.GetStatus?id=0", "0123456789abcdef").unwrap();
        // Unabhängig nachgerechnet (siehe Test-Skript im Commit)
        let ha1 = sha256_hex("admin:shellypro3em-abc:geheim");
        let ha2 = sha256_hex("GET:/rpc/EM.GetStatus?id=0");
        let resp = sha256_hex(&format!("{ha1}:60dc59c6:00000001:0123456789abcdef:auth:{ha2}"));
        assert!(a.contains(&format!("response=\"{resp}\"")));
        assert!(a.starts_with("Digest username=\"admin\""));
    }

    #[test]
    fn adresse() {
        assert_eq!(base_url("192.168.1.50"), "http://192.168.1.50");
        assert_eq!(base_url("http://shelly.local/"), "http://shelly.local");
    }
}
