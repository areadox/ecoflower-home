//! Abfrage-Schleife und Regel-Engine.

use crate::db::{self, Rule};
use crate::ecoflow::{as_f64, EcoFlowClient};
use crate::AppState;
use anyhow::{anyhow, bail, Result};
use chrono::{Local, NaiveTime, Timelike};
use serde_json::{Map, Value};
use std::sync::Arc;
use std::time::Duration;

/// Mindestabstand zwischen zwei Sendeversuchen derselben Regel.
const RULE_COOLDOWN_SECS: i64 = 600;
/// Abstand für erneute Versuche, nachdem das Gerät einen Wert abgelehnt hat.
const RULE_RETRY_AFTER_REJECT_SECS: i64 = 3600;
/// Reihenfolge, in der der Ladestand eines einzelnen Geräts gesucht wird.
const SOC_KEYS: [&str; 3] = ["bmsBattSoc", "soc", "f32ShowSoc"];
const ROLE_KEYS: [&str; 2] = ["distributedDeviceStatus", "seriesConnectDeviceStatus"];

pub fn client(state: &AppState) -> Result<EcoFlowClient> {
    let s = {
        let conn = state.db.lock().unwrap();
        db::load_settings(&conn)?
    };
    if s.access_key.is_empty() || s.secret_key.is_empty() {
        bail!("Access Key und Secret Key fehlen (Einstellungen)");
    }
    EcoFlowClient::new(&s.api_url, &s.access_key, &s.secret_key)
}

/// Platzhalter `{{sn}}` und `{{value}}` in einer Befehlsvorlage ersetzen.
pub fn render_command(template: &str, sn: &str, value: &str) -> Result<Value> {
    let v = value.trim();
    let literal = if v.is_empty() {
        "null".to_string()
    } else if v == "true" || v == "false" {
        v.to_string()
    } else if let Ok(n) = v.replace(',', ".").parse::<f64>() {
        if n.fract() == 0.0 { format!("{}", n as i64) } else { n.to_string() }
    } else {
        serde_json::to_string(v)?
    };
    let sn_escaped = serde_json::to_string(sn)?;
    let sn_inner = &sn_escaped[1..sn_escaped.len() - 1];
    let text = template.replace("{{sn}}", sn_inner).replace("{{value}}", &literal);
    serde_json::from_str(&text).map_err(|e| anyhow!("Befehl ist kein gültiges JSON ({e}): {text}"))
}

pub fn pick_soc(quotas: &Map<String, Value>, override_key: &str) -> Option<f64> {
    if !override_key.is_empty() {
        return quotas.get(override_key).and_then(as_f64);
    }
    let unit: Vec<f64> = SOC_KEYS.iter().filter_map(|k| quotas.get(*k).and_then(as_f64)).collect();
    if let Some(v) = unit.iter().copied().find(|v| *v > 0.0).or_else(|| unit.first().copied()) {
        return Some(v);
    }
    // Systemladestand: nur das Hauptgerät meldet hier einen Wert über 0.
    quotas.get("cmsBattSoc").and_then(as_f64).filter(|v| *v > 0.0)
}

/// Haupt- oder Nebengerät? Aus MQTT kommt `distributedDeviceStatus`; ohne MQTT
/// gilt: Nur das Hauptgerät meldet einen Systemladestand (`cmsBattSoc`) über 0.
pub fn pick_role(quotas: &Map<String, Value>) -> String {
    // Beide Felder ansehen: `distributedDeviceStatus` steht bei Nebengeräten oft auf
    // „ELECTION“, während `seriesConnectDeviceStatus` korrekt „SLAVE“ meldet.
    let mut other = String::new();
    for k in ROLE_KEYS {
        let role = match quotas.get(k) {
            Some(Value::String(s)) => s.trim().to_uppercase(),
            // als Zahl: 0 = Wahl läuft, 1 = Haupt, 2 = Neben
            Some(v) => match as_f64(v).map(|n| n as i64) {
                Some(1) => "MASTER".into(),
                Some(2) => "SLAVE".into(),
                _ => String::new(),
            },
            None => String::new(),
        };
        if role == "MASTER" || role == "SLAVE" {
            return role;
        }
        if other.is_empty() {
            other = role;
        }
    }
    if !other.is_empty() {
        return other;
    }
    match quotas.get("cmsBattSoc").and_then(as_f64) {
        Some(v) if v > 0.0 => "MASTER".into(),
        Some(_) => "SLAVE".into(),
        None => String::new(),
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Activity {
    /// "charging", "discharging" oder "idle"
    pub state: &'static str,
    /// Aus welchem Wert der Zustand stammt (zur Nachvollziehbarkeit)
    pub source: &'static str,
    /// Batterieleistung des Geräts in W (Betrag), falls bekannt
    pub power_w: Option<f64>,
}

/// Was macht der Speicher gerade? Reihenfolge der Quellen:
/// 1. **Energiebilanz** (MQTT, alle ~2 s, pro Gerät):
///    Akku = Solar (`powGetPv`…`powGetPv4`) − Netzanschluss (`gridConnectionPower`, + = Abgabe ans Haus)
///    − Steckdosen (`offgrid1/2ActivePower`). Positiv = Laden.
///    Die BMS-Werte (`bmsChgDsgState`, `amp`) kommen seltener und erfassen teils nur einen
///    Akkublock – im Betrieb zeigten sie z. B. „Standby“, während ein AC Pro 300 W abgab.
/// 2. `bmsChgDsgState` (0 = Standby, 1 = Entladen, 2 = Laden), falls keine Bilanz möglich ist
/// 3. Akkustrom `amp` × `vol`
/// 4. nur Hauptgerät: `cmsChgDsgState` bzw. Vorzeichen von `powGetBpCms` (Systemwerte)
pub fn pick_activity(quotas: &Map<String, Value>, role: &str) -> Option<Activity> {
    let num = |k: &str| quotas.get(k).and_then(as_f64);
    // Leistung pro Gerät aus Strom (mA) × Spannung (mV)
    let unit_power = match (num("amp"), num("vol")) {
        (Some(a), Some(v)) if v > 0.0 => Some(a * v / 1_000_000.0),
        _ => None,
    };
    let from_code = |c: f64| match c as i64 {
        1 => Some("discharging"),
        2 => Some("charging"),
        0 => Some("idle"),
        _ => None,
    };
    // Energiebilanz nur mit MQTT-Daten (die HTTP-Abfrage liefert keine Solarwerte pro Gerät)
    if let (Some(grid_conn), true) = (num("gridConnectionPower"), quotas.contains_key("powGetPv")) {
        let pv: f64 = ["powGetPv", "powGetPv2", "powGetPv3", "powGetPv4"].iter().filter_map(|k| num(k)).sum();
        let outlets: f64 = ["offgrid1ActivePower", "offgrid2ActivePower"].iter().filter_map(|k| num(k)).sum();
        let p = pv - grid_conn - outlets;
        let state = if p > 30.0 { "charging" } else if p < -30.0 { "discharging" } else { "idle" };
        return Some(Activity { state, source: "Energiebilanz (Solar − Netzanschluss − Steckdosen)", power_w: Some(p.abs()) });
    }
    if let Some(state) = num("bmsChgDsgState").and_then(from_code) {
        return Some(Activity { state, source: "bmsChgDsgState", power_w: unit_power.map(f64::abs) });
    }
    if let Some(p) = unit_power {
        let state = if p > 10.0 { "charging" } else if p < -10.0 { "discharging" } else { "idle" };
        return Some(Activity { state, source: "amp×vol", power_w: Some(p.abs()) });
    }
    if role == "MASTER" {
        let sys_power = num("powGetBpCms");
        if let Some(state) = num("cmsChgDsgState").and_then(from_code) {
            return Some(Activity { state, source: "cmsChgDsgState (System)", power_w: sys_power.map(f64::abs) });
        }
        if let Some(p) = sys_power {
            // EcoFlow: positiv = Laden, negativ = Entladen
            let state = if p > 20.0 { "charging" } else if p < -20.0 { "discharging" } else { "idle" };
            return Some(Activity { state, source: "powGetBpCms (System)", power_w: Some(p.abs()) });
        }
    }
    None
}

/// `systemGroupId` als Text (0 bzw. leer = kein System)
fn group_key(v: &Value) -> Option<String> {
    let s = match v {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    (!s.is_empty() && s != "0").then_some(s)
}

/// Neue Werte (HTTP oder MQTT) speichern und Ladestand/Rolle neu bestimmen.
pub fn apply_quotas(
    conn: &rusqlite::Connection,
    sn: &str,
    new: &Map<String, Value>,
) -> Result<(Map<String, Value>, Option<f64>)> {
    let merged = db::merge_quotas(conn, sn, new)?;
    let soc_key = db::device_soc_key(conn, sn)?;
    let soc = pick_soc(&merged, &soc_key);
    let role = pick_role(&merged);
    conn.execute(
        "UPDATE devices SET soc=?2, role=?3, online=1 WHERE sn=?1",
        rusqlite::params![sn, soc, role],
    )?;
    if let Some(group) = merged.get("systemGroupId").and_then(group_key) {
        db::auto_assign_system(conn, sn, &group)?;
    }
    Ok((merged, soc))
}

/// Ein Gerät abfragen, Werte speichern und dessen Regeln auswerten.
pub async fn poll_device(state: &Arc<AppState>, client: &EcoFlowClient, sn: &str) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    match client.quota_all(sn).await {
        Ok(http_quotas) => {
            let quotas = {
                let conn = state.db.lock().unwrap();
                let prev_err: Option<String> = conn
                    .query_row("SELECT last_error FROM devices WHERE sn = ?1", [sn], |r| r.get(0))
                    .unwrap_or(None);
                if prev_err.is_some() {
                    db::log_event(&conn, sn, "info", "Gerät antwortet wieder");
                }
                let (quotas, soc) = apply_quotas(&conn, sn, &http_quotas)?;
                conn.execute(
                    "UPDATE devices SET last_poll=?2, last_error=NULL WHERE sn=?1",
                    rusqlite::params![sn, now],
                )?;
                let num = |k: &str| quotas.get(k).and_then(as_f64);
                conn.execute(
                    "INSERT INTO samples (sn, ts, soc, battery_w, min_dsg, max_chg, backup) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    rusqlite::params![
                        sn,
                        now,
                        soc,
                        num("powGetBpCms"),
                        num("cmsMinDsgSoc"),
                        num("cmsMaxChgSoc"),
                        num("backupReverseSoc")
                    ],
                )?;
                quotas
            };
            evaluate_rules(state, client, sn, &quotas).await;
            Ok(())
        }
        Err(e) => {
            let msg = e.to_string();
            let conn = state.db.lock().unwrap();
            let prev_err: Option<String> = conn
                .query_row("SELECT last_error FROM devices WHERE sn = ?1", [sn], |r| r.get(0))
                .unwrap_or(None);
            if prev_err.as_deref() != Some(msg.as_str()) {
                db::log_event(&conn, sn, "error", &format!("Abfrage fehlgeschlagen: {msg}"));
            }
            conn.execute("UPDATE devices SET last_error=?2 WHERE sn=?1", rusqlite::params![sn, msg])?;
            Err(e)
        }
    }
}

/// Endlosschleife: alle aktiven Geräte regelmäßig abfragen.
pub async fn run_poller(state: Arc<AppState>) {
    let mut last_cleanup = 0i64;
    loop {
        let settings = {
            let conn = state.db.lock().unwrap();
            db::load_settings(&conn).unwrap_or_default()
        };
        match client(&state) {
            Ok(client) => {
                let sns: Vec<String> = {
                    let conn = state.db.lock().unwrap();
                    db::list_devices(&conn)
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|d| d.enabled)
                        .map(|d| d.sn)
                        .collect()
                };
                for sn in sns {
                    let _ = poll_device(&state, &client, &sn).await;
                }
            }
            Err(e) => tracing::debug!("Abfrage übersprungen: {e}"),
        }

        let now = chrono::Utc::now().timestamp();
        if now - last_cleanup > 3600 {
            let conn = state.db.lock().unwrap();
            if let Err(e) = db::cleanup(&conn, settings.retention_days) {
                tracing::warn!("Aufräumen fehlgeschlagen: {e}");
            }
            last_cleanup = now;
        }

        let wait = Duration::from_secs(settings.poll_seconds.max(10));
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = state.poll_now.notified() => {}
        }
    }
}

fn parse_time(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").ok()
}

/// Liegt die aktuelle Uhrzeit im Zeitfenster der Regel? (Leer = immer.)
pub fn in_window(start: &str, end: &str, now: NaiveTime) -> bool {
    match (parse_time(start), parse_time(end)) {
        (Some(s), Some(e)) if s <= e => now >= s && now < e,
        (Some(s), Some(e)) => now >= s || now < e, // über Mitternacht
        _ => true,
    }
}

/// Befehl ausführen (oder im Testmodus nur protokollieren).
pub async fn execute(
    state: &Arc<AppState>,
    client: &EcoFlowClient,
    sn: &str,
    command_id: i64,
    value: &str,
    origin: &str,
) -> Result<()> {
    let (cmd, dry_run) = {
        let conn = state.db.lock().unwrap();
        let cmd = db::get_command(&conn, command_id)?.ok_or_else(|| anyhow!("Befehl {command_id} existiert nicht"))?;
        (cmd, db::load_settings(&conn)?.dry_run)
    };
    let payload = render_command(&cmd.template, sn, value)?;
    if dry_run {
        let conn = state.db.lock().unwrap();
        db::log_event(&conn, sn, "info", &format!("[Testmodus] {origin}: würde „{}“ = {value} senden", cmd.name));
        return Ok(());
    }
    match client.set_quota(payload).await {
        Ok(_) => {
            let conn = state.db.lock().unwrap();
            db::log_event(&conn, sn, "info", &format!("{origin}: „{}“ = {value} gesendet", cmd.name));
            Ok(())
        }
        Err(e) => {
            let conn = state.db.lock().unwrap();
            db::log_event(&conn, sn, "error", &format!("{origin}: „{}“ = {value} fehlgeschlagen: {e}", cmd.name));
            Err(e)
        }
    }
}

async fn evaluate_rules(state: &Arc<AppState>, client: &EcoFlowClient, sn: &str, quotas: &Map<String, Value>) {
    let (rules, soc_key) = {
        let conn = state.db.lock().unwrap();
        (db::list_rules(&conn, Some(sn)).unwrap_or_default(), db::device_soc_key(&conn, sn).unwrap_or_default())
    };
    let now = chrono::Utc::now().timestamp();
    let local = Local::now().time().with_second(0).unwrap_or_default();
    for rule in rules.into_iter().filter(|r| r.enabled) {
        let res = match rule.kind.as_str() {
            "keep" => eval_keep(state, client, &rule, quotas, now, local).await,
            "threshold" => eval_threshold(state, client, &rule, pick_soc(quotas, &soc_key), local).await,
            other => Err(anyhow!("Unbekannter Regeltyp {other}")),
        };
        if let Err(e) = res {
            tracing::warn!("Regel „{}“: {e}", rule.name);
        }
    }
}

/// „Wert halten“: Befehl senden, solange der zurückgelesene Wert vom Sollwert abweicht.
async fn eval_keep(
    state: &Arc<AppState>,
    client: &EcoFlowClient,
    rule: &Rule,
    quotas: &Map<String, Value>,
    now: i64,
    local: NaiveTime,
) -> Result<()> {
    let set = |st: &str, ts: i64, pending: bool| {
        let conn = state.db.lock().unwrap();
        db::update_rule_state(&conn, rule.id, st, ts, pending)
    };
    let cmd_id = rule.command_id.ok_or_else(|| anyhow!("kein Befehl gewählt"))?;
    if !in_window(&rule.window_start, &rule.window_end, local) {
        if rule.last_state != "outside" {
            set("outside", rule.last_action_ts, false)?;
        }
        return Ok(());
    }

    let target = rule.value.replace(',', ".");
    let target_num = match target.as_str() {
        "true" => Some(1.0),
        "false" => Some(0.0),
        t => t.parse::<f64>().ok(),
    };
    let current = if rule.read_key.is_empty() { None } else { quotas.get(&rule.read_key).and_then(as_f64) };

    let matches = match (current, target_num) {
        (Some(c), Some(t)) => (c - t).abs() < 0.5,
        _ => false,
    };

    if rule.read_key.is_empty() {
        // Ohne Rücklese-Wert nur einmal pro Zeitfenster senden.
        if rule.last_state != "sent" {
            execute(state, client, &rule.sn, cmd_id, &rule.value, &format!("Regel „{}“", rule.name)).await?;
            set("sent", now, false)?;
        }
        return Ok(());
    }

    if matches {
        if rule.pending_check {
            let conn = state.db.lock().unwrap();
            db::log_event(
                &conn,
                &rule.sn,
                "info",
                &format!("Regel „{}“: Gerät hat {} = {} übernommen ✔", rule.name, rule.read_key, rule.value),
            );
        }
        if rule.last_state != "ok" || rule.pending_check {
            set("ok", rule.last_action_ts, false)?;
        }
        return Ok(());
    }

    if rule.pending_check && now - rule.last_action_ts >= 120 {
        let conn = state.db.lock().unwrap();
        db::log_event(
            &conn,
            &rule.sn,
            "warn",
            &format!(
                "Regel „{}“: Gerät meldet weiterhin {} = {} statt {} – Wert wurde nicht übernommen oder vom System überschrieben",
                rule.name,
                rule.read_key,
                current.map(|c| c.to_string()).unwrap_or_else(|| "–".into()),
                rule.value
            ),
        );
        drop(conn);
        set("rejected", rule.last_action_ts, false)?;
        return Ok(());
    }

    // Nach einer Ablehnung nur noch stündlich erneut versuchen.
    let cooldown = if rule.last_state == "rejected" { RULE_RETRY_AFTER_REJECT_SECS } else { RULE_COOLDOWN_SECS };
    if !rule.pending_check && now - rule.last_action_ts >= cooldown {
        execute(state, client, &rule.sn, cmd_id, &rule.value, &format!("Regel „{}“", rule.name)).await?;
        let dry_run = {
            let conn = state.db.lock().unwrap();
            db::load_settings(&conn)?.dry_run
        };
        set("sent", now, !dry_run)?;
    }
    Ok(())
}

/// „Schwellwert“: unter X % Befehl A, über Y % Befehl B (mit Hysterese).
async fn eval_threshold(
    state: &Arc<AppState>,
    client: &EcoFlowClient,
    rule: &Rule,
    soc: Option<f64>,
    local: NaiveTime,
) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    let set = |st: &str| {
        let conn = state.db.lock().unwrap();
        db::update_rule_state(&conn, rule.id, st, now, false)
    };
    let origin = format!("Regel „{}“", rule.name);

    if !in_window(&rule.window_start, &rule.window_end, local) {
        // Außerhalb des Zeitfensters den Normalzustand („oberhalb“) wiederherstellen.
        if rule.last_state == "below" {
            if let Some(cmd) = rule.above_command_id {
                execute(state, client, &rule.sn, cmd, &rule.above_value, &format!("{origin} (Zeitfenster vorbei)")).await?;
            }
        }
        if rule.last_state != "outside" {
            set("outside")?;
        }
        return Ok(());
    }

    let soc = soc.ok_or_else(|| anyhow!("kein Ladestand verfügbar"))?;
    if let Some(below) = rule.below {
        if soc <= below && rule.last_state != "below" {
            if let Some(cmd) = rule.below_command_id {
                execute(state, client, &rule.sn, cmd, &rule.below_value, &format!("{origin} (Ladestand {soc:.0} % ≤ {below} %)")).await?;
            }
            set("below")?;
            return Ok(());
        }
    }
    if let Some(above) = rule.above {
        if soc >= above && rule.last_state != "above" {
            if let Some(cmd) = rule.above_command_id {
                execute(state, client, &rule.sn, cmd, &rule.above_value, &format!("{origin} (Ladestand {soc:.0} % ≥ {above} %)")).await?;
            }
            set("above")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn vorlage_wird_befuellt() {
        let tpl = r#"{"sn":"{{sn}}","cmdId":17,"params":{"cfgMinDsgSoc":{{value}}}}"#;
        assert_eq!(render_command(tpl, "BK123", "20").unwrap(), json!({"sn":"BK123","cmdId":17,"params":{"cfgMinDsgSoc":20}}));
        assert_eq!(render_command(tpl, "BK123", "false").unwrap()["params"]["cfgMinDsgSoc"], json!(false));
        assert!(render_command("{kaputt", "x", "1").is_err());
    }

    #[test]
    fn zustand_aus_energiebilanz() {
        let q = |v: Value| -> Map<String, Value> { serde_json::from_value(v).unwrap() };
        // Echte Werte: AC Pro gibt 304 W ab, BMS meldet „Standby“ → Bilanz gewinnt
        let ac_pro = q(json!({"powGetPv": 0.0, "powGetPv2": 0.0, "gridConnectionPower": 304.1,
            "offgrid1ActivePower": 9, "offgrid2ActivePower": 3, "bmsChgDsgState": 0, "amp": -120, "vol": 19550}));
        let a = pick_activity(&ac_pro, "SLAVE").unwrap();
        assert_eq!(a.state, "discharging");
        assert_eq!(a.power_w.map(f64::round), Some(316.0));
        // Ultra: 769 W Solar, 84 W ans Haus → lädt ca. 678 W
        let ultra = q(json!({"powGetPv": 234.2, "powGetPv2": 228.3, "powGetPv3": 241.5, "powGetPv4": 65.1,
            "gridConnectionPower": 84.1, "offgrid1ActivePower": 6, "offgrid2ActivePower": 1, "bmsChgDsgState": 2}));
        let a = pick_activity(&ultra, "SLAVE").unwrap();
        assert_eq!(a.state, "charging");
        assert_eq!(a.power_w.map(f64::round), Some(678.0));
        // AC Pro lädt aus dem Netz (Netzanschluss negativ)
        let bad = q(json!({"powGetPv": 0.0, "gridConnectionPower": -1052.4, "offgrid2ActivePower": -2}));
        assert_eq!(pick_activity(&bad, "MASTER").unwrap().state, "charging");
        // nur HTTP-Daten (ohne Solarwerte pro Gerät) → keine Bilanz
        let http = q(json!({"gridConnectionPower": 300.0, "cmsBattSoc": 0.0}));
        assert_eq!(pick_activity(&http, "SLAVE"), None);
    }

    #[test]
    fn zustand_erkennen() {
        let q = |v: Value| -> Map<String, Value> { serde_json::from_value(v).unwrap() };
        let a = pick_activity(&q(json!({"bmsChgDsgState": 2, "amp": 44671, "vol": 20161})), "SLAVE").unwrap();
        assert_eq!(a.state, "charging");
        assert_eq!(a.power_w.map(|p| p.round()), Some(901.0));
        assert_eq!(pick_activity(&q(json!({"bmsChgDsgState": 1})), "SLAVE").unwrap().state, "discharging");
        assert_eq!(pick_activity(&q(json!({"bmsChgDsgState": 0})), "SLAVE").unwrap().state, "idle");
        assert_eq!(pick_activity(&q(json!({"amp": -5000, "vol": 20000})), "SLAVE").unwrap().state, "discharging");
        // Nur HTTP: Hauptgerät über powGetBpCms, Nebengerät unbekannt
        assert_eq!(pick_activity(&q(json!({"powGetBpCms": 1499.4})), "MASTER").unwrap().state, "charging");
        assert_eq!(pick_activity(&q(json!({"powGetBpCms": -300.0})), "MASTER").unwrap().state, "discharging");
        assert_eq!(pick_activity(&q(json!({"powGetBpCms": 0.0})), "MASTER").unwrap().state, "idle");
        assert_eq!(pick_activity(&q(json!({"powGetBpCms": 0.0})), "SLAVE"), None);
    }

    #[test]
    fn zeitfenster() {
        let t = |s| NaiveTime::parse_from_str(s, "%H:%M").unwrap();
        assert!(in_window("", "", t("03:00")));
        assert!(in_window("08:00", "18:00", t("12:00")));
        assert!(!in_window("08:00", "18:00", t("19:00")));
        assert!(in_window("22:00", "06:00", t("23:30")));
        assert!(in_window("22:00", "06:00", t("05:00")));
        assert!(!in_window("22:00", "06:00", t("12:00")));
    }

    #[test]
    fn ladestand_auswahl() {
        let q: Map<String, Value> = serde_json::from_value(json!({"cmsBattSoc": 0.0, "soc": 47, "bmsBattSoc": 0})).unwrap();
        assert_eq!(pick_soc(&q, ""), Some(47.0));
        assert_eq!(pick_soc(&q, "cmsBattSoc"), Some(0.0));
        // Nur HTTP-Daten: Nebengerät hat keinen Ladestand, Hauptgerät den Systemwert
        let neben: Map<String, Value> = serde_json::from_value(json!({"cmsBattSoc": 0.0})).unwrap();
        assert_eq!(pick_soc(&neben, ""), None);
        assert_eq!(pick_role(&neben), "SLAVE");
        assert_eq!(pick_activity(&neben, "SLAVE"), None);
        let haupt: Map<String, Value> = serde_json::from_value(json!({"cmsBattSoc": 78.0})).unwrap();
        assert_eq!(pick_soc(&haupt, ""), Some(78.0));
        assert_eq!(pick_role(&haupt), "MASTER");
        let zahl: Map<String, Value> = serde_json::from_value(json!({"distributedDeviceStatus": 2, "cmsBattSoc": 50})).unwrap();
        assert_eq!(pick_role(&zahl), "SLAVE");
        let wahl: Map<String, Value> = serde_json::from_value(json!({"distributedDeviceStatus": "ELECTION", "seriesConnectDeviceStatus": "SLAVE"})).unwrap();
        assert_eq!(pick_role(&wahl), "SLAVE");
    }
}
