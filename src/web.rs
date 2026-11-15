//! JSON-API und Auslieferung der Weboberfläche.

use crate::db::{self, Command, Rule};
use crate::engine;
use crate::AppState;
use axum::{
    body::Body,
    extract::{Path, Query, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

type AppResult<T> = Result<T, AppError>;

pub struct AppError(StatusCode, String);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl<E: std::fmt::Display> From<E> for AppError {
    fn from(e: E) -> Self {
        AppError(StatusCode::BAD_REQUEST, e.to_string())
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    let api = Router::new()
        .route("/status", get(status))
        .route("/settings", get(get_settings).post(save_settings))
        .route("/devices", get(list_devices))
        .route("/systems", get(list_systems).post(save_system))
        .route("/systems/{id}", delete(delete_system))
        .route("/devices/sync", post(sync_devices))
        .route("/devices/{sn}", put(update_device))
        .route("/devices/{sn}/quotas", get(device_quotas))
        .route("/devices/{sn}/refresh", post(refresh_device))
        .route("/devices/{sn}/history", get(device_history))
        .route("/devices/{sn}/send", post(send_command))
        .route("/commands", get(list_commands).post(save_command))
        .route("/commands/{id}", delete(delete_command))
        .route("/rules", get(list_rules).post(save_rule))
        .route("/rules/{id}", delete(delete_rule))
        .route("/events", get(list_events))
        .route("/export", get(export))
        .route("/update", get(update_info))
        .route("/update/check", post(update_check))
        .route("/update/install", post(update_install));

    Router::new()
        .route("/", get(|| async { static_file("index.html") }))
        .route("/app.js", get(|| async { static_file("app.js") }))
        .route("/style.css", get(|| async { static_file("style.css") }))
        .nest("/api", api)
        .layer(middleware::from_fn_with_state(state.clone(), basic_auth))
        .with_state(state)
}

fn static_file(name: &str) -> Response {
    let (body, mime) = match name {
        "index.html" => (include_str!("../static/index.html"), "text/html; charset=utf-8"),
        "app.js" => (include_str!("../static/app.js"), "application/javascript; charset=utf-8"),
        "style.css" => (include_str!("../static/style.css"), "text/css; charset=utf-8"),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    ([(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, "no-cache")], body).into_response()
}

/// Optionaler Passwortschutz (HTTP Basic Auth) über WEB_USER / WEB_PASSWORD.
async fn basic_auth(State(state): State<Arc<AppState>>, headers: HeaderMap, req: Request, next: Next) -> Response {
    let Some(password) = &state.web_password else {
        return next.run(req).await;
    };
    let ok = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
        .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok())
        .and_then(|b| String::from_utf8(b).ok())
        .map(|creds| {
            let (u, p) = creds.split_once(':').unwrap_or(("", ""));
            u == state.web_user && p == password
        })
        .unwrap_or(false);
    if ok {
        next.run(req).await
    } else {
        Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(header::WWW_AUTHENTICATE, "Basic realm=\"ecoflower-home\"")
            .body(Body::from("Anmeldung erforderlich"))
            .unwrap()
    }
}

// ---------- Status & Einstellungen ----------

async fn status(State(s): State<Arc<AppState>>) -> AppResult<Json<Value>> {
    let conn = s.db.lock().unwrap();
    let st = db::load_settings(&conn)?;
    Ok(Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "configured": !st.access_key.is_empty() && !st.secret_key.is_empty(),
        "dry_run": st.dry_run,
        "poll_seconds": st.poll_seconds,
        "mqtt": *s.mqtt.lock().unwrap(),
        "update": *s.update.lock().unwrap(),
    })))
}

async fn get_settings(State(s): State<Arc<AppState>>) -> AppResult<Json<Value>> {
    let conn = s.db.lock().unwrap();
    let st = db::load_settings(&conn)?;
    Ok(Json(json!({
        "access_key": st.access_key,
        "secret_key_set": !st.secret_key.is_empty(),
        "api_url": st.api_url,
        "poll_seconds": st.poll_seconds,
        "dry_run": st.dry_run,
        "retention_days": st.retention_days,
        "mqtt_enabled": st.mqtt_enabled,
    })))
}

#[derive(Deserialize)]
struct SettingsInput {
    access_key: Option<String>,
    secret_key: Option<String>,
    api_url: Option<String>,
    poll_seconds: Option<u64>,
    dry_run: Option<bool>,
    retention_days: Option<u64>,
    mqtt_enabled: Option<bool>,
}

async fn save_settings(State(s): State<Arc<AppState>>, Json(i): Json<SettingsInput>) -> AppResult<Json<Value>> {
    {
        let conn = s.db.lock().unwrap();
        if let Some(v) = i.access_key {
            db::set_setting(&conn, "access_key", v.trim())?;
        }
        if let Some(v) = i.secret_key.filter(|v| !v.trim().is_empty()) {
            db::set_setting(&conn, "secret_key", v.trim())?;
        }
        if let Some(v) = i.api_url {
            db::set_setting(&conn, "api_url", v.trim())?;
        }
        if let Some(v) = i.poll_seconds {
            db::set_setting(&conn, "poll_seconds", &v.max(10).to_string())?;
        }
        if let Some(v) = i.dry_run {
            db::set_setting(&conn, "dry_run", if v { "1" } else { "0" })?;
            db::log_event(&conn, "", "info", if v { "Testmodus eingeschaltet" } else { "Testmodus ausgeschaltet – Regeln senden jetzt echte Befehle" });
        }
        if let Some(v) = i.retention_days {
            db::set_setting(&conn, "retention_days", &v.max(1).to_string())?;
        }
        if let Some(v) = i.mqtt_enabled {
            db::set_setting(&conn, "mqtt_enabled", if v { "1" } else { "0" })?;
        }
    }
    s.poll_now.notify_one();
    s.mqtt_restart.notify_one();
    Ok(Json(json!({ "ok": true })))
}

// ---------- Geräte ----------

async fn list_devices(State(s): State<Arc<AppState>>) -> AppResult<Json<Value>> {
    let conn = s.db.lock().unwrap();
    Ok(Json(json!(db::list_devices(&conn)?)))
}

async fn sync_devices(State(s): State<Arc<AppState>>) -> AppResult<Json<Value>> {
    let client = engine::client(&s)?;
    let list = client.device_list().await?;
    let mut added = 0;
    {
        let conn = s.db.lock().unwrap();
        for d in &list {
            let name = d.device_name.clone().unwrap_or_default();
            let product = d.product_name.clone().unwrap_or_default();
            if db::upsert_api_device(&conn, &d.sn, &name, &product, d.online.unwrap_or(0) == 1)? {
                added += 1;
                db::log_event(&conn, &d.sn, "info", &format!("Neues Gerät gefunden: {name} ({product})"));
            }
        }
    }
    s.poll_now.notify_one();
    s.mqtt_restart.notify_one();
    Ok(Json(json!({ "found": list.len(), "added": added })))
}

#[derive(Deserialize)]
struct DeviceInput {
    /// > 0 = System von Hand zuordnen, 0 = keinem System zuordnen
    system_id: Option<i64>,
    /// true = Zuordnung wieder automatisch (über systemGroupId von EcoFlow)
    system_auto: Option<bool>,
    alias: Option<String>,
    enabled: Option<bool>,
    soc_key: Option<String>,
}

async fn update_device(
    State(s): State<Arc<AppState>>,
    Path(sn): Path<String>,
    Json(i): Json<DeviceInput>,
) -> AppResult<Json<Value>> {
    let conn = s.db.lock().unwrap();
    if let Some(v) = i.alias {
        conn.execute("UPDATE devices SET alias=?2 WHERE sn=?1", rusqlite::params![sn, v.trim()])?;
    }
    if let Some(v) = i.enabled {
        conn.execute("UPDATE devices SET enabled=?2 WHERE sn=?1", rusqlite::params![sn, v as i64])?;
    }
    if i.system_auto == Some(true) {
        conn.execute("UPDATE devices SET system_manual=0 WHERE sn=?1", [&sn])?;
    } else if let Some(id) = i.system_id {
        let id = (id > 0).then_some(id);
        conn.execute(
            "UPDATE devices SET system_id=?2, system_manual=1 WHERE sn=?1",
            rusqlite::params![sn, id],
        )?;
    }
    if let Some(v) = i.soc_key {
        conn.execute("UPDATE devices SET soc_key=?2 WHERE sn=?1", rusqlite::params![sn, v.trim()])?;
    }
    Ok(Json(json!({ "ok": true })))
}

async fn device_quotas(State(s): State<Arc<AppState>>, Path(sn): Path<String>) -> AppResult<Json<Value>> {
    let conn = s.db.lock().unwrap();
    Ok(Json(db::get_quotas(&conn, &sn)?.unwrap_or(json!({}))))
}

async fn refresh_device(State(s): State<Arc<AppState>>, Path(sn): Path<String>) -> AppResult<Json<Value>> {
    let client = engine::client(&s)?;
    engine::poll_device(&s, &client, &sn).await?;
    let conn = s.db.lock().unwrap();
    Ok(Json(db::get_quotas(&conn, &sn)?.unwrap_or(json!({}))))
}

#[derive(Deserialize)]
struct HistoryQuery {
    hours: Option<i64>,
}

async fn device_history(
    State(s): State<Arc<AppState>>,
    Path(sn): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> AppResult<Json<Value>> {
    let since = chrono::Utc::now().timestamp() - q.hours.unwrap_or(24).clamp(1, 24 * 90) * 3600;
    let conn = s.db.lock().unwrap();
    Ok(Json(json!(db::history(&conn, &sn, since)?)))
}

#[derive(Deserialize)]
struct SendInput {
    command_id: Option<i64>,
    value: Option<String>,
    raw: Option<Value>,
}

/// Befehl manuell senden – auch im Testmodus wirklich (für Experimente).
async fn send_command(
    State(s): State<Arc<AppState>>,
    Path(sn): Path<String>,
    Json(i): Json<SendInput>,
) -> AppResult<Json<Value>> {
    let client = engine::client(&s)?;
    let (payload, label) = if let Some(mut raw) = i.raw {
        if raw.get("sn").is_none() {
            raw["sn"] = json!(sn);
        }
        (raw, "Rohbefehl".to_string())
    } else {
        let id = i.command_id.ok_or_else(|| AppError(StatusCode::BAD_REQUEST, "Kein Befehl gewählt".into()))?;
        let cmd = {
            let conn = s.db.lock().unwrap();
            db::get_command(&conn, id)?.ok_or_else(|| AppError(StatusCode::NOT_FOUND, "Befehl nicht gefunden".into()))?
        };
        let value = i.value.unwrap_or_default();
        (engine::render_command(&cmd.template, &sn, &value)?, format!("„{}“ = {value}", cmd.name))
    };
    let result = client.set_quota(payload.clone()).await;
    let conn = s.db.lock().unwrap();
    match result {
        Ok(resp) => {
            db::log_event(&conn, &sn, "info", &format!("Manuell gesendet: {label}"));
            Ok(Json(json!({ "ok": true, "request": payload, "response": resp })))
        }
        Err(e) => {
            db::log_event(&conn, &sn, "error", &format!("Manuell gesendet: {label} – fehlgeschlagen: {e}"));
            Err(AppError(StatusCode::BAD_GATEWAY, e.to_string()))
        }
    }
}

// ---------- Systeme ----------

async fn list_systems(State(s): State<Arc<AppState>>) -> AppResult<Json<Value>> {
    let conn = s.db.lock().unwrap();
    Ok(Json(json!(db::list_systems(&conn)?)))
}

#[derive(Deserialize)]
struct SystemInput {
    #[serde(default)]
    id: i64,
    name: String,
}

async fn save_system(State(s): State<Arc<AppState>>, Json(i): Json<SystemInput>) -> AppResult<Json<Value>> {
    let name = i.name.trim();
    if name.is_empty() {
        return Err(AppError(StatusCode::BAD_REQUEST, "Bitte einen Namen eingeben".into()));
    }
    let conn = s.db.lock().unwrap();
    if i.id > 0 {
        conn.execute("UPDATE systems SET name=?2 WHERE id=?1", rusqlite::params![i.id, name])?;
        Ok(Json(json!({ "id": i.id })))
    } else {
        conn.execute("INSERT INTO systems (name) VALUES (?1)", [name])?;
        Ok(Json(json!({ "id": conn.last_insert_rowid() })))
    }
}

async fn delete_system(State(s): State<Arc<AppState>>, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    let conn = s.db.lock().unwrap();
    conn.execute("UPDATE devices SET system_id=NULL, system_manual=0 WHERE system_id=?1", [id])?;
    conn.execute("DELETE FROM systems WHERE id=?1", [id])?;
    Ok(Json(json!({ "ok": true })))
}

// ---------- Befehle ----------

async fn list_commands(State(s): State<Arc<AppState>>) -> AppResult<Json<Value>> {
    let conn = s.db.lock().unwrap();
    Ok(Json(json!(db::list_commands(&conn)?)))
}

async fn save_command(State(s): State<Arc<AppState>>, Json(c): Json<Command>) -> AppResult<Json<Value>> {
    // Vorlage prüfen
    engine::render_command(&c.template, "TEST", "1")?;
    let conn = s.db.lock().unwrap();
    if c.id > 0 {
        conn.execute(
            "UPDATE commands SET name=?2, description=?3, template=?4 WHERE id=?1",
            rusqlite::params![c.id, c.name, c.description, c.template],
        )?;
        Ok(Json(json!({ "id": c.id })))
    } else {
        conn.execute(
            "INSERT INTO commands (name, description, template) VALUES (?1, ?2, ?3)",
            rusqlite::params![c.name, c.description, c.template],
        )?;
        Ok(Json(json!({ "id": conn.last_insert_rowid() })))
    }
}

async fn delete_command(State(s): State<Arc<AppState>>, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    let conn = s.db.lock().unwrap();
    let used: i64 = conn.query_row(
        "SELECT COUNT(*) FROM rules WHERE command_id=?1 OR below_command_id=?1 OR above_command_id=?1",
        [id],
        |r| r.get(0),
    )?;
    if used > 0 {
        return Err(AppError(StatusCode::CONFLICT, "Befehl wird noch von Regeln verwendet".into()));
    }
    conn.execute("DELETE FROM commands WHERE id=?1", [id])?;
    Ok(Json(json!({ "ok": true })))
}

// ---------- Regeln ----------

async fn list_rules(State(s): State<Arc<AppState>>) -> AppResult<Json<Value>> {
    let conn = s.db.lock().unwrap();
    Ok(Json(json!(db::list_rules(&conn, None)?)))
}

async fn save_rule(State(s): State<Arc<AppState>>, Json(r): Json<Rule>) -> AppResult<Json<Value>> {
    if r.kind != "keep" && r.kind != "threshold" {
        return Err(AppError(StatusCode::BAD_REQUEST, "Unbekannter Regeltyp".into()));
    }
    let id = {
        let conn = s.db.lock().unwrap();
        let id = db::save_rule(&conn, &r)?;
        db::log_event(&conn, &r.sn, "info", &format!("Regel „{}“ gespeichert", r.name));
        id
    };
    s.poll_now.notify_one();
    Ok(Json(json!({ "id": id })))
}

async fn delete_rule(State(s): State<Arc<AppState>>, Path(id): Path<i64>) -> AppResult<Json<Value>> {
    let conn = s.db.lock().unwrap();
    conn.execute("DELETE FROM rules WHERE id=?1", [id])?;
    Ok(Json(json!({ "ok": true })))
}

// ---------- Ereignisse ----------

#[derive(Deserialize)]
struct EventQuery {
    limit: Option<i64>,
    sn: Option<String>,
}

async fn list_events(State(s): State<Arc<AppState>>, Query(q): Query<EventQuery>) -> AppResult<Json<Value>> {
    let conn = s.db.lock().unwrap();
    let events = db::list_events(&conn, q.limit.unwrap_or(200).clamp(1, 2000), q.sn.as_deref())?;
    Ok(Json(json!(events)))
}

// ---------- Update ----------

async fn update_info(State(s): State<Arc<AppState>>) -> AppResult<Json<Value>> {
    let mut info = s.update.lock().unwrap().clone();
    info.blocked = crate::update::update_blocker();
    Ok(Json(json!(info)))
}

async fn update_check(State(s): State<Arc<AppState>>) -> AppResult<Json<Value>> {
    let info = crate::update::check().await;
    *s.update.lock().unwrap() = info.clone();
    Ok(Json(json!(info)))
}

async fn update_install(State(s): State<Arc<AppState>>) -> AppResult<Json<Value>> {
    let (version, exe) = crate::update::install()
        .await
        .map_err(|e| AppError(StatusCode::BAD_GATEWAY, e.to_string()))?;
    {
        let conn = s.db.lock().unwrap();
        db::log_event(&conn, "", "info", &format!("Update auf Version {version} installiert – Neustart"));
    }
    // Antwort zuerst senden, dann den Prozess durch die neue Version ersetzen.
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        let err = crate::update::restart(&exe);
        tracing::error!("{err}");
    });
    Ok(Json(json!({ "ok": true, "version": version })))
}

// ---------- Daten-Download ----------

/// Alle Daten zur Fehlersuche als JSON-Datei – ohne Secret Key und Passwörter.
async fn export(State(s): State<Arc<AppState>>) -> AppResult<Response> {
    let mqtt = s.mqtt.lock().unwrap().clone();
    let mqtt_raw = s.mqtt_raw.lock().unwrap().clone();
    let update = s.update.lock().unwrap().clone();
    let body = {
        let conn = s.db.lock().unwrap();
        let st = db::load_settings(&conn)?;
        let mask = |v: &str| {
            if v.len() > 6 { format!("{}…{}", &v[..3], &v[v.len() - 3..]) } else if v.is_empty() { String::new() } else { "***".into() }
        };
        let devices = db::list_devices(&conn)?;
        let mut device_data = serde_json::Map::new();
        let since = chrono::Utc::now().timestamp() - 24 * 3600;
        for d in &devices {
            device_data.insert(
                d.sn.clone(),
                json!({
                    "quotas": db::get_quotas(&conn, &d.sn)?.unwrap_or(json!({})),
                    "history_24h": db::history(&conn, &d.sn, since)?,
                }),
            );
        }
        json!({
            "export": {
                "version": env!("CARGO_PKG_VERSION"),
                "time": chrono::Utc::now().to_rfc3339(),
                "arch": std::env::consts::ARCH,
            },
            "settings": {
                "access_key": mask(&st.access_key),
                "secret_key_set": !st.secret_key.is_empty(),
                "api_url": st.api_url,
                "poll_seconds": st.poll_seconds,
                "dry_run": st.dry_run,
                "mqtt_enabled": st.mqtt_enabled,
                "retention_days": st.retention_days,
            },
            "mqtt": mqtt,
            "mqtt_last_messages": mqtt_raw,
            "update": update,
            "devices": devices,
            "device_data": device_data,
            "systems": db::list_systems(&conn)?,
            "commands": db::list_commands(&conn)?,
            "rules": db::list_rules(&conn, None)?,
            "events": db::list_events(&conn, 500, None)?,
        })
    };
    let name = format!("ecoflower-debug-{}.json", chrono::Local::now().format("%Y%m%d-%H%M"));
    let text = serde_json::to_string_pretty(&body)?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
        ],
        text,
    )
        .into_response())
}
