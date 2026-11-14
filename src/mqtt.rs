//! Live-Daten über den MQTT-Broker der EcoFlow-Open-API.
//!
//! Die HTTP-Schnittstelle liefert bei STREAM-Geräten nur wenige Systemwerte. Die
//! vollständigen Werte pro Gerät (z. B. der Ladestand jedes einzelnen Speichers)
//! kommen über MQTT auf dem Thema `/open/<Konto>/<Seriennummer>/quota`.

use crate::db;
use crate::ecoflow::flatten_mqtt_message;
use crate::engine;
use crate::AppState;
use anyhow::{bail, Result};
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS, TlsConfiguration, Transport};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Default, Serialize)]
pub struct MqttStatus {
    pub connected: bool,
    pub last_message: Option<i64>,
    pub messages: u64,
    pub error: Option<String>,
}

/// Endlosschleife: verbinden, abonnieren, bei Fehlern nach einer Pause neu versuchen.
pub async fn run(state: Arc<AppState>) {
    let mut last_error: Option<String> = None;
    loop {
        let result = session(&state).await;
        {
            let mut st = state.mqtt.lock().unwrap();
            st.connected = false;
            if let Err(e) = &result {
                st.error = Some(e.to_string());
            }
        }
        let wait = match result {
            Ok(()) => Duration::from_secs(2), // gewollter Neustart (z. B. neue Geräte)
            Err(e) => {
                let msg = e.to_string();
                if last_error.as_deref() != Some(msg.as_str()) {
                    let conn = state.db.lock().unwrap();
                    db::log_event(&conn, "", "warn", &format!("MQTT: {msg}"));
                }
                last_error = Some(msg);
                Duration::from_secs(60)
            }
        };
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = state.mqtt_restart.notified() => {}
        }
    }
}

fn enabled_devices(state: &AppState) -> BTreeSet<String> {
    let conn = state.db.lock().unwrap();
    db::list_devices(&conn).unwrap_or_default().into_iter().filter(|d| d.enabled).map(|d| d.sn).collect()
}

fn tls_config() -> TlsConfiguration {
    let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("TLS-Versionen")
        .with_root_certificates(roots)
        .with_no_client_auth();
    TlsConfiguration::Rustls(Arc::new(config))
}

/// Eine feste, pro Installation eindeutige Client-ID (EcoFlow begrenzt die Anzahl).
fn client_id(state: &AppState, account: &str) -> String {
    let conn = state.db.lock().unwrap();
    let suffix = match db::get_setting(&conn, "mqtt_client_suffix").ok().flatten() {
        Some(s) if !s.is_empty() => s,
        _ => {
            let s = format!("{:06x}", rand::random::<u32>() & 0xff_ffff);
            let _ = db::set_setting(&conn, "mqtt_client_suffix", &s);
            s
        }
    };
    format!("ecoflower-{account}-{suffix}")
}

async fn session(state: &Arc<AppState>) -> Result<()> {
    let enabled = {
        let conn = state.db.lock().unwrap();
        db::get_setting(&conn, "mqtt_enabled")?.as_deref() != Some("0")
    };
    if !enabled {
        // ausgeschaltet: warten, bis die Einstellungen sich ändern
        state.mqtt.lock().unwrap().error = Some("ausgeschaltet".into());
        state.mqtt_restart.notified().await;
        return Ok(());
    }
    let client = match engine::client(state) {
        Ok(c) => c,
        Err(_) => {
            state.mqtt_restart.notified().await;
            return Ok(());
        }
    };
    let sns = enabled_devices(state);
    if sns.is_empty() {
        state.mqtt_restart.notified().await;
        return Ok(());
    }

    let cert = client.mqtt_certification().await?;
    let mut opts = MqttOptions::new(client_id(state, &cert.account), cert.url.clone(), cert.port);
    opts.set_credentials(cert.account.clone(), cert.password.clone());
    opts.set_keep_alive(Duration::from_secs(30));
    opts.set_max_packet_size(256 * 1024, 256 * 1024);
    if cert.protocol != "mqtt" {
        opts.set_transport(Transport::tls_with_config(tls_config()));
    }

    let (mqtt, mut events) = AsyncClient::new(opts, 64);
    let topics: Vec<String> = sns
        .iter()
        .flat_map(|sn| [format!("/open/{}/{sn}/quota", cert.account), format!("/open/{}/{sn}/status", cert.account)])
        .collect();

    let mut check = tokio::time::interval(Duration::from_secs(60));
    check.tick().await;
    loop {
        tokio::select! {
            ev = events.poll() => match ev {
                Ok(Event::Incoming(Packet::ConnAck(_))) => {
                    for t in &topics {
                        mqtt.subscribe(t.clone(), QoS::AtLeastOnce).await?;
                    }
                    let mut st = state.mqtt.lock().unwrap();
                    st.connected = true;
                    st.error = None;
                    drop(st);
                    let conn = state.db.lock().unwrap();
                    db::log_event(&conn, "", "info", &format!("MQTT verbunden ({} Geräte abonniert)", sns.len()));
                }
                Ok(Event::Incoming(Packet::Publish(p))) => {
                    handle_message(state, &p.topic, &p.payload);
                }
                Ok(_) => {}
                Err(e) => bail!("Verbindung getrennt: {e}"),
            },
            _ = check.tick() => {
                if enabled_devices(state) != sns {
                    let _ = mqtt.disconnect().await;
                    return Ok(());
                }
            }
            _ = state.mqtt_restart.notified() => {
                let _ = mqtt.disconnect().await;
                return Ok(());
            }
        }
    }
}

fn handle_message(state: &AppState, topic: &str, payload: &[u8]) {
    // /open/<Konto>/<Seriennummer>/<Art>
    let parts: Vec<&str> = topic.split('/').collect();
    if parts.len() < 5 {
        return;
    }
    let (sn, kind) = (parts[3], parts[4]);
    let Ok(msg) = serde_json::from_slice::<Value>(payload) else {
        tracing::debug!("MQTT: keine JSON-Nachricht auf {topic}");
        return;
    };
    let now = chrono::Utc::now().timestamp();
    {
        let mut st = state.mqtt.lock().unwrap();
        st.last_message = Some(now);
        st.messages += 1;
    }
    let conn = state.db.lock().unwrap();
    match kind {
        "quota" => {
            let mut values = flatten_mqtt_message(&msg);
            if values.is_empty() {
                return;
            }
            values.insert("quota_mqtt_ts".into(), Value::from(now));
            if let Err(e) = engine::apply_quotas(&conn, sn, &values) {
                tracing::warn!("MQTT-Werte für {sn} nicht gespeichert: {e}");
            }
        }
        "status" => {
            let online = msg
                .get("params")
                .and_then(|p| p.get("status"))
                .or_else(|| msg.get("online"))
                .and_then(crate::ecoflow::as_f64)
                .map(|v| v != 0.0);
            if let Some(online) = online {
                let _ = conn.execute(
                    "UPDATE devices SET online=?2 WHERE sn=?1",
                    rusqlite::params![sn, online as i64],
                );
            }
        }
        _ => {}
    }
}
