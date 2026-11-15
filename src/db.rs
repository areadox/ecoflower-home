//! SQLite-Datenbank: Einstellungen, Geräte, Messwerte, Befehle, Regeln, Ereignisse.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub fn open(path: &str) -> Result<Connection> {
    if let Some(dir) = std::path::Path::new(path).parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    let conn = Connection::open(path)?;
    conn.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS devices (
            sn         TEXT PRIMARY KEY,
            name       TEXT NOT NULL DEFAULT '',
            product    TEXT NOT NULL DEFAULT '',
            alias      TEXT NOT NULL DEFAULT '',
            online     INTEGER NOT NULL DEFAULT 0,
            enabled    INTEGER NOT NULL DEFAULT 1,
            soc_key    TEXT NOT NULL DEFAULT '',
            role       TEXT NOT NULL DEFAULT '',
            soc        REAL,
            quotas     TEXT,
            last_poll  INTEGER,
            last_error TEXT
        );

        CREATE TABLE IF NOT EXISTS samples (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            sn         TEXT NOT NULL,
            ts         INTEGER NOT NULL,
            soc        REAL,
            battery_w  REAL,
            min_dsg    REAL,
            max_chg    REAL,
            backup     REAL
        );
        CREATE INDEX IF NOT EXISTS idx_samples_sn_ts ON samples(sn, ts);

        CREATE TABLE IF NOT EXISTS commands (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            name        TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            template    TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS rules (
            id                INTEGER PRIMARY KEY AUTOINCREMENT,
            sn                TEXT NOT NULL,
            name              TEXT NOT NULL,
            enabled           INTEGER NOT NULL DEFAULT 1,
            kind              TEXT NOT NULL,          -- 'keep' oder 'threshold'
            window_start      TEXT NOT NULL DEFAULT '',
            window_end        TEXT NOT NULL DEFAULT '',
            -- keep: Wert dauerhaft halten
            command_id        INTEGER,
            value             TEXT NOT NULL DEFAULT '',
            read_key          TEXT NOT NULL DEFAULT '',
            -- threshold: Schaltschwellen auf den Ladestand
            below             REAL,
            below_command_id  INTEGER,
            below_value       TEXT NOT NULL DEFAULT '',
            above             REAL,
            above_command_id  INTEGER,
            above_value       TEXT NOT NULL DEFAULT '',
            -- Laufzeitzustand
            last_state        TEXT NOT NULL DEFAULT '',
            last_action_ts    INTEGER NOT NULL DEFAULT 0,
            pending_check     INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS events (
            id      INTEGER PRIMARY KEY AUTOINCREMENT,
            ts      INTEGER NOT NULL,
            sn      TEXT NOT NULL DEFAULT '',
            level   TEXT NOT NULL,
            message TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_events_ts ON events(ts);
        "#,
    )?;
    migrate(&conn)?;
    seed_commands(&conn)?;
    Ok(conn)
}

/// Spätere Schema-Erweiterungen für bestehende Datenbanken.
fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS systems (
            id        INTEGER PRIMARY KEY AUTOINCREMENT,
            name      TEXT NOT NULL,
            group_key TEXT UNIQUE
        );",
    )?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meter_samples (
            ts      INTEGER NOT NULL,
            power_w REAL
        );
        CREATE INDEX IF NOT EXISTS idx_meter_ts ON meter_samples(ts);",
    )?;
    let has_col = |col: &str| -> Result<bool> {
        let mut stmt = conn.prepare("SELECT 1 FROM pragma_table_info('devices') WHERE name = ?1")?;
        Ok(stmt.exists([col])?)
    };
    if !has_col("system_id")? {
        conn.execute_batch(
            "ALTER TABLE devices ADD COLUMN system_id INTEGER;
             ALTER TABLE devices ADD COLUMN system_manual INTEGER NOT NULL DEFAULT 0;",
        )?;
    }
    // 0.0.6: Feldnamen wie bei der Backup-Reserve („cfg“ + Quota-Name)
    conn.execute_batch(
        r#"UPDATE commands SET template = replace(template, '"cfgMinDsgSoc"', '"cfgCmsMinDsgSoc"')
             WHERE name = 'STREAM Entladegrenze';
           UPDATE commands SET template = replace(template, '"cfgMaxChgSoc"', '"cfgCmsMaxChgSoc"')
             WHERE name = 'STREAM Ladegrenze';"#,
    )?;
    let has_feed: bool = conn
        .query_row("SELECT 1 FROM commands WHERE name = 'STREAM Einspeiselimit'", [], |_| Ok(true))
        .optional()?
        .unwrap_or(false);
    let any: i64 = conn.query_row("SELECT COUNT(*) FROM commands", [], |r| r.get(0))?;
    if !has_feed && any > 0 {
        conn.execute(
            "INSERT INTO commands (name, description, template) VALUES ('STREAM Einspeiselimit', ?1, ?2)",
            params![
                EINSPEISE_DESC,
                r#"{"sn":"{{sn}}","cmdId":17,"cmdFunc":254,"dirDest":1,"dirSrc":1,"dest":2,"needAck":true,"params":{"cfgFeedGridModePowLimit":{{value}}}}"#
            ],
        )?;
    }
    Ok(())
}

const EINSPEISE_DESC: &str = "Maximale Abgabeleistung ins Hausnetz in W – ungetestet (Quota: feedGridModePowLimit)";

/// Vorlagen für STREAM-Geräte (Format wie bei evcc / hassio-ecoflow-cloud).
fn seed_commands(conn: &Connection) -> Result<()> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM commands", [], |r| r.get(0))?;
    if count > 0 {
        return Ok(());
    }
    let base = |param: &str| {
        format!(
            r#"{{"sn":"{{{{sn}}}}","cmdId":17,"cmdFunc":254,"dirDest":1,"dirSrc":1,"dest":2,"needAck":true,"params":{{"{param}":{{{{value}}}}}}}}"#
        )
    };
    let seeds = [
        ("STREAM Entladegrenze", "Untere Entladegrenze in % (Quota: cmsMinDsgSoc)", base("cfgCmsMinDsgSoc")),
        ("STREAM Ladegrenze", "Obere Ladegrenze in % (Quota: cmsMaxChgSoc)", base("cfgCmsMaxChgSoc")),
        ("STREAM Backup-Reserve", "Backup-Reserve in %, muss 3 % über der Entladegrenze liegen (Quota: backupReverseSoc)", base("cfgBackupReverseSoc")),
        ("STREAM AC-Ausgang 1", "true = an, false = aus (Quota: relay2Onoff)", base("cfgRelay2Onoff")),
        ("STREAM AC-Ausgang 2", "true = an, false = aus (Quota: relay3Onoff)", base("cfgRelay3Onoff")),
        ("STREAM Einspeiselimit", EINSPEISE_DESC, base("cfgFeedGridModePowLimit")),
    ];
    for (name, desc, tpl) in seeds {
        conn.execute(
            "INSERT INTO commands (name, description, template) VALUES (?1, ?2, ?3)",
            params![name, desc, tpl],
        )?;
    }
    Ok(())
}

// ---------- Einstellungen ----------

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
        .optional()?)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    pub access_key: String,
    pub secret_key: String,
    pub api_url: String,
    pub poll_seconds: u64,
    pub dry_run: bool,
    pub retention_days: u64,
    pub mqtt_enabled: bool,
}

pub fn load_settings(conn: &Connection) -> Result<Settings> {
    let get = |k: &str| get_setting(conn, k).map(|v| v.unwrap_or_default());
    Ok(Settings {
        access_key: get("access_key")?,
        secret_key: get("secret_key")?,
        api_url: {
            let v = get("api_url")?;
            if v.is_empty() { "https://api-e.ecoflow.com".into() } else { v }
        },
        poll_seconds: get("poll_seconds")?.parse().unwrap_or(60).max(10),
        dry_run: get("dry_run")? != "0",
        retention_days: get("retention_days")?.parse().unwrap_or(30).max(1),
        mqtt_enabled: get("mqtt_enabled")? != "0",
    })
}

// ---------- Ereignisse ----------

pub fn log_event(conn: &Connection, sn: &str, level: &str, message: &str) {
    let ts = chrono::Utc::now().timestamp();
    if let Err(e) = conn.execute(
        "INSERT INTO events (ts, sn, level, message) VALUES (?1, ?2, ?3, ?4)",
        params![ts, sn, level, message],
    ) {
        tracing::error!("Ereignis konnte nicht gespeichert werden: {e}");
    }
    match level {
        "error" => tracing::error!("[{sn}] {message}"),
        "warn" => tracing::warn!("[{sn}] {message}"),
        _ => tracing::info!("[{sn}] {message}"),
    }
}

#[derive(Serialize)]
pub struct Event {
    pub id: i64,
    pub ts: i64,
    pub sn: String,
    pub level: String,
    pub message: String,
}

pub fn list_events(conn: &Connection, limit: i64, sn: Option<&str>) -> Result<Vec<Event>> {
    let mut stmt = conn.prepare(
        "SELECT id, ts, sn, level, message FROM events
         WHERE (?2 IS NULL OR sn = ?2) ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit, sn], |r| {
            Ok(Event { id: r.get(0)?, ts: r.get(1)?, sn: r.get(2)?, level: r.get(3)?, message: r.get(4)? })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ---------- Geräte ----------

#[derive(Debug, Clone, Serialize)]
pub struct Device {
    pub sn: String,
    pub name: String,
    pub product: String,
    pub alias: String,
    pub online: bool,
    pub enabled: bool,
    pub soc_key: String,
    pub role: String,
    pub soc: Option<f64>,
    pub last_poll: Option<i64>,
    pub last_error: Option<String>,
    pub min_dsg: Option<f64>,
    pub max_chg: Option<f64>,
    pub backup: Option<f64>,
    pub battery_w: Option<f64>,
    pub ac_w: Option<f64>,
    pub ac1_on: Option<bool>,
    pub ac2_on: Option<bool>,
    pub sys_soc: Option<f64>,
    pub pv_w: Option<f64>,
    pub grid_w: Option<f64>,
    pub load_w: Option<f64>,
    pub mqtt_ts: Option<i64>,
    pub activity: Option<crate::engine::Activity>,
    pub system_id: Option<i64>,
    pub system_manual: bool,
    pub mode: Option<String>,
    pub feed_grid_mode: Option<f64>,
}

pub fn list_devices(conn: &Connection) -> Result<Vec<Device>> {
    let mut stmt = conn.prepare(
        "SELECT sn, name, product, alias, online, enabled, soc_key, role, soc, last_poll, last_error, quotas,
                system_id, system_manual
         FROM devices ORDER BY CASE WHEN alias = '' THEN name ELSE alias END, sn",
    )?;
    let rows = stmt
        .query_map([], |r| {
            let quotas: Option<String> = r.get(11)?;
            let q: Value = quotas.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
            let num = |k: &str| q.get(k).and_then(crate::ecoflow::as_f64);
            let role: String = r.get(7)?;
            let activity = q.as_object().and_then(|m| crate::engine::pick_activity(m, &role));
            Ok(Device {
                sn: r.get(0)?,
                name: r.get(1)?,
                product: r.get(2)?,
                alias: r.get(3)?,
                online: r.get::<_, i64>(4)? != 0,
                enabled: r.get::<_, i64>(5)? != 0,
                soc_key: r.get(6)?,
                role,
                soc: r.get(8)?,
                last_poll: r.get(9)?,
                last_error: r.get(10)?,
                min_dsg: num("cmsMinDsgSoc"),
                max_chg: num("cmsMaxChgSoc"),
                backup: num("backupReverseSoc"),
                battery_w: num("powGetBpCms"),
                ac_w: num("gridConnectionPower"),
                ac1_on: num("relay2Onoff").map(|v| v != 0.0),
                ac2_on: num("relay3Onoff").map(|v| v != 0.0),
                sys_soc: num("cmsBattSoc"),
                pv_w: num("powGetPvSum"),
                grid_w: num("powGetSysGrid"),
                load_w: num("powGetSysLoad"),
                mqtt_ts: num("quota_mqtt_ts").map(|v| v as i64),
                activity,
                system_id: r.get(12)?,
                system_manual: r.get::<_, i64>(13)? != 0,
                mode: {
                    let b = |k: &str| q.get(k).and_then(|v| v.as_bool());
                    if b("energyStrategyOperateMode.operateSelfPoweredOpen") == Some(true) {
                        Some("Eigenverbrauch".into())
                    } else if b("energyStrategyOperateMode.operateIntelligentScheduleModeOpen") == Some(true) {
                        Some("Intelligenter Zeitplan".into())
                    } else if q.get("energyStrategyOperateMode.operateSelfPoweredOpen").is_some() {
                        Some("Benutzerdefiniert".into())
                    } else {
                        None
                    }
                },
                feed_grid_mode: num("feedGridMode"),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn upsert_api_device(conn: &Connection, sn: &str, name: &str, product: &str, online: bool) -> Result<bool> {
    let existed: bool = conn
        .query_row("SELECT 1 FROM devices WHERE sn = ?1", [sn], |_| Ok(true))
        .optional()?
        .unwrap_or(false);
    conn.execute(
        "INSERT INTO devices (sn, name, product, online) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(sn) DO UPDATE SET name = excluded.name, product = excluded.product, online = excluded.online",
        params![sn, name, product, online as i64],
    )?;
    Ok(!existed)
}

pub fn get_quotas(conn: &Connection, sn: &str) -> Result<Option<Value>> {
    let s: Option<Option<String>> = conn
        .query_row("SELECT quotas FROM devices WHERE sn = ?1", [sn], |r| r.get(0))
        .optional()?;
    Ok(s.flatten().and_then(|s| serde_json::from_str(&s).ok()))
}

/// Neue Werte in die gespeicherten Quotas eines Geräts einmischen (HTTP und MQTT
/// liefern unterschiedliche Teilmengen). Gibt die zusammengeführten Werte zurück.
pub fn merge_quotas(conn: &Connection, sn: &str, new: &serde_json::Map<String, Value>) -> Result<serde_json::Map<String, Value>> {
    let mut merged = match get_quotas(conn, sn)? {
        Some(Value::Object(m)) => m,
        _ => serde_json::Map::new(),
    };
    for (k, v) in new {
        merged.insert(k.clone(), v.clone());
    }
    conn.execute(
        "UPDATE devices SET quotas=?2 WHERE sn=?1",
        params![sn, serde_json::to_string(&merged)?],
    )?;
    Ok(merged)
}

// ---------- Systeme ----------

#[derive(Debug, Clone, Serialize)]
pub struct System {
    pub id: i64,
    pub name: String,
    pub group_key: Option<String>,
}

pub fn list_systems(conn: &Connection) -> Result<Vec<System>> {
    let mut stmt = conn.prepare("SELECT id, name, group_key FROM systems ORDER BY name, id")?;
    let rows = stmt
        .query_map([], |r| Ok(System { id: r.get(0)?, name: r.get(1)?, group_key: r.get(2)? }))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Gerät anhand der von EcoFlow gemeldeten Gruppe einem System zuordnen
/// (außer, es wurde von Hand zugeordnet).
pub fn auto_assign_system(conn: &Connection, sn: &str, group: &str) -> Result<()> {
    let manual: i64 = conn
        .query_row("SELECT system_manual FROM devices WHERE sn = ?1", [sn], |r| r.get(0))
        .optional()?
        .unwrap_or(0);
    if manual != 0 {
        return Ok(());
    }
    let id: Option<i64> = conn
        .query_row("SELECT id FROM systems WHERE group_key = ?1", [group], |r| r.get(0))
        .optional()?;
    let id = match id {
        Some(id) => id,
        None => {
            let n: i64 = conn.query_row("SELECT COUNT(*) FROM systems", [], |r| r.get(0))?;
            conn.execute(
                "INSERT INTO systems (name, group_key) VALUES (?1, ?2)",
                params![format!("System {}", n + 1), group],
            )?;
            let id = conn.last_insert_rowid();
            log_event(conn, sn, "info", &format!("Neues EcoFlow-System erkannt: „System {}“", n + 1));
            id
        }
    };
    conn.execute("UPDATE devices SET system_id = ?2 WHERE sn = ?1 AND (system_id IS NOT ?2)", params![sn, id])?;
    Ok(())
}

pub fn device_soc_key(conn: &Connection, sn: &str) -> Result<String> {
    Ok(conn
        .query_row("SELECT soc_key FROM devices WHERE sn = ?1", [sn], |r| r.get(0))
        .optional()?
        .unwrap_or_default())
}

// ---------- Befehle ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    #[serde(default)]
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub template: String,
}

pub fn list_commands(conn: &Connection) -> Result<Vec<Command>> {
    let mut stmt = conn.prepare("SELECT id, name, description, template FROM commands ORDER BY name")?;
    let rows = stmt
        .query_map([], |r| Ok(Command { id: r.get(0)?, name: r.get(1)?, description: r.get(2)?, template: r.get(3)? }))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_command(conn: &Connection, id: i64) -> Result<Option<Command>> {
    Ok(conn
        .query_row("SELECT id, name, description, template FROM commands WHERE id = ?1", [id], |r| {
            Ok(Command { id: r.get(0)?, name: r.get(1)?, description: r.get(2)?, template: r.get(3)? })
        })
        .optional()?)
}

// ---------- Regeln ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    #[serde(default)]
    pub id: i64,
    pub sn: String,
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub kind: String,
    #[serde(default)]
    pub window_start: String,
    #[serde(default)]
    pub window_end: String,
    #[serde(default)]
    pub command_id: Option<i64>,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub read_key: String,
    #[serde(default)]
    pub below: Option<f64>,
    #[serde(default)]
    pub below_command_id: Option<i64>,
    #[serde(default)]
    pub below_value: String,
    #[serde(default)]
    pub above: Option<f64>,
    #[serde(default)]
    pub above_command_id: Option<i64>,
    #[serde(default)]
    pub above_value: String,
    #[serde(default)]
    pub last_state: String,
    #[serde(default)]
    pub last_action_ts: i64,
    #[serde(default)]
    pub pending_check: bool,
}

fn yes() -> bool {
    true
}

const RULE_COLS: &str = "id, sn, name, enabled, kind, window_start, window_end, command_id, value, read_key,
    below, below_command_id, below_value, above, above_command_id, above_value, last_state, last_action_ts, pending_check";

fn row_to_rule(r: &rusqlite::Row) -> rusqlite::Result<Rule> {
    Ok(Rule {
        id: r.get(0)?,
        sn: r.get(1)?,
        name: r.get(2)?,
        enabled: r.get::<_, i64>(3)? != 0,
        kind: r.get(4)?,
        window_start: r.get(5)?,
        window_end: r.get(6)?,
        command_id: r.get(7)?,
        value: r.get(8)?,
        read_key: r.get(9)?,
        below: r.get(10)?,
        below_command_id: r.get(11)?,
        below_value: r.get(12)?,
        above: r.get(13)?,
        above_command_id: r.get(14)?,
        above_value: r.get(15)?,
        last_state: r.get(16)?,
        last_action_ts: r.get(17)?,
        pending_check: r.get::<_, i64>(18)? != 0,
    })
}

pub fn list_rules(conn: &Connection, sn: Option<&str>) -> Result<Vec<Rule>> {
    let sql = format!("SELECT {RULE_COLS} FROM rules WHERE (?1 IS NULL OR sn = ?1) ORDER BY sn, name");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([sn], row_to_rule)?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn save_rule(conn: &Connection, r: &Rule) -> Result<i64> {
    if r.id > 0 {
        conn.execute(
            "UPDATE rules SET sn=?2, name=?3, enabled=?4, kind=?5, window_start=?6, window_end=?7, command_id=?8,
             value=?9, read_key=?10, below=?11, below_command_id=?12, below_value=?13, above=?14,
             above_command_id=?15, above_value=?16, last_state='', last_action_ts=0, pending_check=0 WHERE id=?1",
            params![
                r.id, r.sn, r.name, r.enabled as i64, r.kind, r.window_start, r.window_end, r.command_id, r.value,
                r.read_key, r.below, r.below_command_id, r.below_value, r.above, r.above_command_id, r.above_value
            ],
        )?;
        Ok(r.id)
    } else {
        conn.execute(
            "INSERT INTO rules (sn, name, enabled, kind, window_start, window_end, command_id, value, read_key,
             below, below_command_id, below_value, above, above_command_id, above_value)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            params![
                r.sn, r.name, r.enabled as i64, r.kind, r.window_start, r.window_end, r.command_id, r.value,
                r.read_key, r.below, r.below_command_id, r.below_value, r.above, r.above_command_id, r.above_value
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }
}

pub fn update_rule_state(conn: &Connection, id: i64, state: &str, action_ts: i64, pending: bool) -> Result<()> {
    conn.execute(
        "UPDATE rules SET last_state=?2, last_action_ts=?3, pending_check=?4 WHERE id=?1",
        params![id, state, action_ts, pending as i64],
    )?;
    Ok(())
}

// ---------- Verlauf ----------

#[derive(Serialize)]
pub struct Sample {
    pub ts: i64,
    pub soc: Option<f64>,
    pub battery_w: Option<f64>,
    pub min_dsg: Option<f64>,
}

pub fn history(conn: &Connection, sn: &str, since: i64) -> Result<Vec<Sample>> {
    let mut stmt =
        conn.prepare("SELECT ts, soc, battery_w, min_dsg FROM samples WHERE sn = ?1 AND ts >= ?2 ORDER BY ts")?;
    let rows = stmt
        .query_map(params![sn, since], |r| {
            Ok(Sample { ts: r.get(0)?, soc: r.get(1)?, battery_w: r.get(2)?, min_dsg: r.get(3)? })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn cleanup(conn: &Connection, retention_days: u64) -> Result<()> {
    let cutoff = chrono::Utc::now().timestamp() - (retention_days as i64) * 86_400;
    conn.execute("DELETE FROM samples WHERE ts < ?1", [cutoff])?;
    conn.execute("DELETE FROM events WHERE ts < ?1", [cutoff])?;
    conn.execute("DELETE FROM meter_samples WHERE ts < ?1", [cutoff])?;
    Ok(())
}
