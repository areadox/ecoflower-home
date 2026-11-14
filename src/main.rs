//! ecoflower-home – Verwaltung von EcoFlow-STREAM-Speichern mit eigenen Regeln pro Gerät.

mod db;
mod ecoflow;
mod engine;
mod mqtt;
mod web;

use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

pub struct AppState {
    pub db: Mutex<rusqlite::Connection>,
    pub poll_now: Notify,
    pub mqtt: Mutex<mqtt::MqttStatus>,
    pub mqtt_restart: Notify,
    pub web_user: String,
    pub web_password: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let db_path = std::env::var("DATABASE_PATH").unwrap_or_else(|_| "data/ecoflower.db".into());
    let bind = std::env::var("BIND").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let web_user = std::env::var("WEB_USER").unwrap_or_else(|_| "admin".into());
    let web_password = std::env::var("WEB_PASSWORD").ok().filter(|p| !p.is_empty());

    let conn = db::open(&db_path)?;
    let state = Arc::new(AppState {
        db: Mutex::new(conn),
        poll_now: Notify::new(),
        mqtt: Mutex::new(mqtt::MqttStatus::default()),
        mqtt_restart: Notify::new(),
        web_user,
        web_password,
    });

    if state.web_password.is_none() {
        tracing::warn!("WEB_PASSWORD ist nicht gesetzt – die Weboberfläche ist ohne Passwort erreichbar");
    }

    tokio::spawn(engine::run_poller(state.clone()));
    tokio::spawn(mqtt::run(state.clone()));

    let app = web::router(state);
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!("ecoflower-home {} läuft auf http://{bind} (Datenbank: {db_path})", env!("CARGO_PKG_VERSION"));
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Beenden bei Strg+C oder SIGTERM (z. B. `docker stop`).
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = term => {}
    }
    tracing::info!("Wird beendet …");
}
