//! Regler für einen Speicher in einem eigenen EcoFlow-System ohne Zähler
//! (z. B. „AC Pro Bad“), gesteuert über den Shelly im Heimnetz.
//!
//! Stellgrößen, die das Gerät nachweislich annimmt:
//! - **Abgabeleistung** über das Einspeiselimit (`cfgFeedGridModePowLimit`): das Gerät
//!   gibt min(Limit, angenommene Last) ab – bei 200 W Limit genau 200 W.
//! - **Laden** über die Backup-Reserve (`cfgBackupReverseSoc`): liegt sie über dem
//!   Ladestand, lädt das Gerät mit voller Leistung (AC Pro ca. 1050 W).
//!
//! Ablauf:
//! - **Abgeben:** Liegt der Ladestand über der Mindestreserve, wird das Limit so
//!   nachgeführt, dass der Zähler knapp auf Bezug (Ziel z. B. +20 W) steht.
//! - **Laden:** Meldet der Zähler längere Zeit genug Einspeisung, wird die Reserve
//!   auf 100 % gesetzt. Kommt Netzbezug auf oder entlädt das Hauptsystem dafür, wird
//!   wieder gestoppt – so wird kein Netzstrom und kein Strom aus dem Hauptspeicher geladen.
//! - Die Reserve steht sonst auf min(Mindestreserve, Ladestand), damit das Gerät nie
//!   von selbst aus dem Netz nachlädt.

use crate::db;
use crate::ecoflow::as_f64;
use crate::engine;
use crate::AppState;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RegConfig {
    pub enabled: bool,
    /// Seriennummer des geregelten Speichers
    pub sn: String,
    /// Hauptgerät des anderen Systems (optional, für den Stopp beim Umladen)
    pub main_sn: String,
    /// Mindestreserve in %, darunter wird nicht abgegeben
    pub reserve: f64,
    /// Maximale Abgabe in W
    pub max_out: f64,
    /// Zielwert am Zähler in W (leicht positiv = etwas Bezug, verhindert Einspeisung)
    pub target_grid: f64,
    /// Laden starten ab dieser Einspeisung in W
    pub start_export: f64,
    /// Laden stoppen bei Netzbezug bzw. Entladung des Hauptsystems über diesem Wert in W
    pub stop_import: f64,
    /// Laden bis zu diesem Ladestand in %
    pub charge_until: f64,
    /// So lange (s) muss eine Bedingung anliegen, bevor umgeschaltet wird
    pub delay_secs: i64,
    /// Regeltakt in s
    pub interval_secs: u64,
}

impl Default for RegConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            sn: String::new(),
            main_sn: String::new(),
            reserve: 60.0,
            max_out: 600.0,
            target_grid: 20.0,
            start_export: 800.0,
            stop_import: 150.0,
            charge_until: 95.0,
            delay_secs: 60,
            interval_secs: 10,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Discharge,
    Charge,
}

/// Laufzeitzustand der Regelung.
#[derive(Debug, Clone)]
pub struct RegState {
    pub mode: Mode,
    pub start_since: Option<i64>,
    pub stop_since: Option<i64>,
    pub last_limit: Option<f64>,
    pub last_limit_ts: i64,
    pub last_reserve_ts: i64,
}

impl Default for RegState {
    fn default() -> Self {
        Self { mode: Mode::Discharge, start_since: None, stop_since: None, last_limit: None, last_limit_ts: 0, last_reserve_ts: 0 }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Inputs {
    pub now: i64,
    pub soc: f64,
    /// Akkuleistung des Speichers: positiv = Laden, negativ = Entladen
    pub batt_w: f64,
    pub reserve_now: Option<f64>,
    pub limit_now: Option<f64>,
    /// Zähler: positiv = Bezug, negativ = Einspeisung
    pub grid_w: f64,
    /// Akkuleistung des Hauptsystems (positiv = Laden, negativ = Entladen)
    pub main_batt_w: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Action {
    SetLimit(f64),
    SetReserve(f64),
}

/// Ein Regelschritt. Reine Funktion – ohne Netzwerk, damit sie getestet werden kann.
/// Gibt die Befehle und eine Notiz bei Moduswechsel zurück.
pub fn decide(cfg: &RegConfig, st: &mut RegState, i: &Inputs) -> (Vec<Action>, Option<String>) {
    let mut actions = Vec::new();
    let mut note = None;
    let out_now = (-i.batt_w).max(0.0);
    let export = -i.grid_w;
    // Reserve, bei der das Gerät weder abgibt noch aus dem Netz nachlädt
    let hold_reserve = cfg.reserve.min(i.soc.floor()).max(0.0);

    match st.mode {
        Mode::Discharge => {
            // Laden starten?
            if i.soc < cfg.charge_until && export >= cfg.start_export {
                let since = *st.start_since.get_or_insert(i.now);
                if i.now - since >= cfg.delay_secs {
                    st.mode = Mode::Charge;
                    st.start_since = None;
                    st.stop_since = None;
                    actions.push(Action::SetLimit(0.0));
                    st.last_limit = Some(0.0);
                    st.last_limit_ts = i.now;
                    actions.push(Action::SetReserve(100.0));
                    st.last_reserve_ts = i.now;
                    note = Some(format!("Laden gestartet ({export:.0} W Einspeisung seit {} s)", i.now - since));
                    return (actions, note);
                }
            } else {
                st.start_since = None;
            }

            // Reserve so halten, dass das Gerät nicht von selbst nachlädt
            if i.reserve_now.map(|r| (r - hold_reserve).abs() >= 1.0).unwrap_or(true) && i.now - st.last_reserve_ts >= 60 {
                actions.push(Action::SetReserve(hold_reserve));
                st.last_reserve_ts = i.now;
            }

            // Abgabe nachführen
            let desired = if i.soc > cfg.reserve + 0.5 {
                let raw = out_now + i.grid_w - cfg.target_grid;
                ((raw / 10.0).round() * 10.0).clamp(0.0, cfg.max_out)
            } else {
                0.0
            };
            let changed = match st.last_limit {
                None => true,
                Some(last) => (desired - last).abs() >= 25.0 || (desired == 0.0 && last != 0.0),
            };
            // Gerät meldet ein anderes Limit als zuletzt gesendet → nach 60 s erneut senden
            let drift = matches!((i.limit_now, st.last_limit), (Some(now), Some(last)) if (now - last).abs() >= 25.0)
                && i.now - st.last_limit_ts >= 60;
            if (changed && i.now - st.last_limit_ts >= 10) || drift {
                actions.push(Action::SetLimit(desired));
                st.last_limit = Some(desired);
                st.last_limit_ts = i.now;
            }
        }
        Mode::Charge => {
            let main_discharging = i.main_batt_w.map(|w| -w >= cfg.stop_import).unwrap_or(false);
            let full = i.soc >= cfg.charge_until;
            let stop_reason = if full {
                Some(format!("Ladestand {:.0} % erreicht", i.soc))
            } else if i.grid_w >= cfg.stop_import || main_discharging {
                let since = *st.stop_since.get_or_insert(i.now);
                (i.now - since >= cfg.delay_secs).then(|| {
                    if main_discharging {
                        "Hauptsystem entlädt dafür".to_string()
                    } else {
                        format!("{:.0} W Netzbezug", i.grid_w)
                    }
                })
            } else {
                st.stop_since = None;
                None
            };
            if let Some(reason) = stop_reason {
                st.mode = Mode::Discharge;
                st.stop_since = None;
                actions.push(Action::SetReserve(hold_reserve));
                st.last_reserve_ts = i.now;
                note = Some(format!("Laden beendet: {reason}"));
                return (actions, note);
            }
            // Reserve bleibt oben – erneut senden, falls das Gerät sie nicht übernommen hat
            if i.reserve_now.map(|r| r < 99.0).unwrap_or(false) && i.now - st.last_reserve_ts >= 60 {
                actions.push(Action::SetReserve(100.0));
                st.last_reserve_ts = i.now;
            }
        }
    }
    (actions, note)
}

// ---------------------------------------------------------------------------
// Laufzeit

#[derive(Debug, Clone, Default, Serialize)]
pub struct RegStatus {
    pub running: bool,
    pub mode: Option<Mode>,
    pub waiting: Option<String>,
    pub inputs: Option<Inputs>,
    pub last_tick: Option<i64>,
    /// Letzte Befehle: (Zeit, Text)
    pub history: VecDeque<(i64, String)>,
}

pub fn load_config(conn: &rusqlite::Connection) -> RegConfig {
    db::get_setting(conn, "regulator")
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_config(conn: &rusqlite::Connection, cfg: &RegConfig) -> anyhow::Result<()> {
    db::set_setting(conn, "regulator", &serde_json::to_string(cfg)?)
}

fn quotas(conn: &rusqlite::Connection, sn: &str) -> Map<String, Value> {
    match db::get_quotas(conn, sn).ok().flatten() {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

fn stream_cmd(sn: &str, key: &str, value: f64) -> Value {
    json!({
        "sn": sn, "cmdId": 17, "cmdFunc": 254, "dirDest": 1, "dirSrc": 1, "dest": 2, "needAck": true,
        "params": { key: value.round() as i64 }
    })
}

fn push_history(state: &AppState, now: i64, text: String) {
    let mut s = state.regulator.lock().unwrap();
    s.history.push_front((now, text));
    s.history.truncate(40);
}

pub async fn run(state: Arc<AppState>) {
    let mut st = RegState::default();
    let mut active_sn = String::new();
    loop {
        let (cfg, dry_run) = {
            let conn = state.db.lock().unwrap();
            (load_config(&conn), db::load_settings(&conn).map(|s| s.dry_run).unwrap_or(true))
        };
        let wait = Duration::from_secs(cfg.interval_secs.clamp(5, 60));
        if !cfg.enabled || cfg.sn.is_empty() {
            {
                let mut s = state.regulator.lock().unwrap();
                s.running = false;
                s.mode = None;
                s.waiting = Some(if cfg.sn.is_empty() { "kein Speicher gewählt".into() } else { "ausgeschaltet".into() });
            }
            st = RegState::default();
            active_sn.clear();
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = state.regulator_restart.notified() => {}
            }
            continue;
        }
        if active_sn != cfg.sn {
            st = RegState::default();
            active_sn = cfg.sn.clone();
        }

        let now = chrono::Utc::now().timestamp();
        let inputs = gather(&state, &cfg, now);
        match inputs {
            Err(reason) => {
                {
                    let mut s = state.regulator.lock().unwrap();
                    s.running = true;
                    s.waiting = Some(reason.clone());
                    s.last_tick = Some(now);
                }
                // Sicherheit: ohne frische Zählerwerte nichts abgeben
                if st.mode == Mode::Discharge && st.last_limit.unwrap_or(1.0) > 0.0 && reason.contains("Zähler") {
                    send(&state, &cfg.sn, &Action::SetLimit(0.0), dry_run, now).await;
                    st.last_limit = Some(0.0);
                    st.last_limit_ts = now;
                }
            }
            Ok(i) => {
                let prev_mode = st.mode;
                let (actions, note) = decide(&cfg, &mut st, &i);
                {
                    let mut s = state.regulator.lock().unwrap();
                    s.running = true;
                    s.waiting = None;
                    s.mode = Some(st.mode);
                    s.inputs = Some(i.clone());
                    s.last_tick = Some(now);
                }
                if let Some(n) = note {
                    {
                        let conn = state.db.lock().unwrap();
                        let prefix = if dry_run { "[Testmodus] " } else { "" };
                        db::log_event(&conn, &cfg.sn, "info", &format!("{prefix}Regler: {n}"));
                    }
                    push_history(&state, now, n);
                } else if prev_mode != st.mode {
                    push_history(&state, now, format!("Modus: {:?}", st.mode));
                }
                for a in &actions {
                    send(&state, &cfg.sn, a, dry_run, now).await;
                }
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = state.regulator_restart.notified() => {}
        }
    }
}

fn gather(state: &AppState, cfg: &RegConfig, now: i64) -> Result<Inputs, String> {
    let meter = state.meter.lock().unwrap().clone();
    let grid = match (meter.ok, meter.power_w, meter.ts) {
        (true, Some(p), Some(ts)) if now - ts <= 20 => p,
        (false, _, _) | (_, None, _) => return Err("Zähler (Shelly) liefert keine Werte".into()),
        _ => return Err("Zählerwerte sind veraltet".into()),
    };
    let conn = state.db.lock().unwrap();
    let q = quotas(&conn, &cfg.sn);
    let num = |m: &Map<String, Value>, k: &str| m.get(k).and_then(as_f64);
    let ts = num(&q, "quota_mqtt_ts").unwrap_or(0.0) as i64;
    if now - ts > 180 {
        return Err("keine aktuellen MQTT-Daten vom Speicher".into());
    }
    let soc = engine::pick_soc(&q, "").ok_or("Ladestand des Speichers unbekannt")?;
    let main_batt_w = if cfg.main_sn.is_empty() {
        None
    } else {
        num(&quotas(&conn, &cfg.main_sn), "powGetBpCms")
    };
    Ok(Inputs {
        now,
        soc,
        batt_w: num(&q, "powGetBpCms").unwrap_or(0.0),
        reserve_now: num(&q, "backupReverseSoc"),
        limit_now: num(&q, "feedGridModePowLimit"),
        grid_w: grid,
        main_batt_w,
    })
}

async fn send(state: &Arc<AppState>, sn: &str, action: &Action, dry_run: bool, now: i64) {
    let (key, value, text) = match action {
        Action::SetLimit(w) => ("cfgFeedGridModePowLimit", *w, format!("Abgabe-Limit {w:.0} W")),
        Action::SetReserve(r) => ("cfgBackupReverseSoc", *r, format!("Reserve {r:.0} %")),
    };
    if dry_run {
        push_history(state, now, format!("[Testmodus] würde senden: {text}"));
        return;
    }
    let result = match engine::client(state) {
        Ok(c) => c.set_quota(stream_cmd(sn, key, value)).await.map(|_| ()),
        Err(e) => Err(e),
    };
    match result {
        Ok(()) => push_history(state, now, text),
        Err(e) => {
            push_history(state, now, format!("{text} – Fehler: {e}"));
            let conn = state.db.lock().unwrap();
            db::log_event(&conn, sn, "error", &format!("Regler: {text} fehlgeschlagen: {e}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> RegConfig {
        RegConfig { enabled: true, sn: "BAD".into(), ..Default::default() }
    }

    fn inp(now: i64, soc: f64, batt_w: f64, grid_w: f64) -> Inputs {
        Inputs { now, soc, batt_w, reserve_now: Some(60.0), limit_now: None, grid_w, main_batt_w: None }
    }

    #[test]
    fn gibt_ab_was_das_haus_braucht() {
        let c = cfg();
        let mut st = RegState::default();
        // 80 %, gibt nichts ab, Haus bezieht 320 W → Limit 300 W (Ziel +20 W)
        let (a, _) = decide(&c, &mut st, &inp(1000, 80.0, 0.0, 320.0));
        assert!(a.contains(&Action::SetLimit(300.0)));
        // nächster Takt: gibt 300 W ab, Zähler +20 W → keine Änderung
        let (a, _) = decide(&c, &mut st, &inp(1010, 80.0, -300.0, 20.0));
        assert!(a.iter().all(|x| !matches!(x, Action::SetLimit(_))));
        // Haus braucht mehr als 600 W → bei 600 W gedeckelt
        let (a, _) = decide(&c, &mut st, &inp(1020, 80.0, -300.0, 900.0));
        assert!(a.contains(&Action::SetLimit(600.0)));
        // Einspeisung → Limit runter
        let (a, _) = decide(&c, &mut st, &inp(1030, 80.0, -600.0, -250.0));
        assert!(a.contains(&Action::SetLimit(330.0)));
    }

    #[test]
    fn haelt_die_reserve() {
        let c = cfg();
        let mut st = RegState { last_limit: Some(200.0), ..Default::default() };
        let (a, _) = decide(&c, &mut st, &inp(1000, 60.0, -200.0, 300.0));
        assert!(a.contains(&Action::SetLimit(0.0)));
        // Ladestand unter Reserve → Reserve auf den Ladestand, damit nicht aus dem Netz geladen wird
        let mut st = RegState::default();
        let mut i = inp(1000, 34.6, 0.0, 300.0);
        i.reserve_now = Some(60.0);
        let (a, _) = decide(&c, &mut st, &i);
        assert!(a.contains(&Action::SetReserve(34.0)));
    }

    #[test]
    fn laedt_nur_aus_ueberschuss() {
        let c = cfg();
        let mut st = RegState::default();
        // Einspeisung 900 W – erst nach 60 s starten
        let (a, _) = decide(&c, &mut st, &inp(1000, 50.0, 0.0, -900.0));
        assert!(!a.contains(&Action::SetReserve(100.0)));
        let (a, n) = decide(&c, &mut st, &inp(1061, 50.0, 0.0, -900.0));
        assert!(a.contains(&Action::SetReserve(100.0)));
        assert_eq!(st.mode, Mode::Charge);
        assert!(n.unwrap().starts_with("Laden gestartet"));
        // Lädt mit 1050 W, Zähler zeigt jetzt 150 W Bezug → erst nach 60 s stoppen
        let mut i = inp(1100, 52.0, 1050.0, 200.0);
        i.reserve_now = Some(100.0);
        let (a, _) = decide(&c, &mut st, &i);
        assert!(a.is_empty());
        i.now = 1161;
        let (a, n) = decide(&c, &mut st, &i);
        assert!(a.contains(&Action::SetReserve(52.0)));
        assert_eq!(st.mode, Mode::Discharge);
        assert!(n.unwrap().contains("Netzbezug"));
    }

    #[test]
    fn stoppt_wenn_hauptsystem_entlaedt() {
        let c = cfg();
        let mut st = RegState { mode: Mode::Charge, ..Default::default() };
        let mut i = inp(1000, 70.0, 1050.0, 0.0);
        i.reserve_now = Some(100.0);
        i.main_batt_w = Some(-800.0);
        decide(&c, &mut st, &i);
        i.now = 1060;
        let (_, n) = decide(&c, &mut st, &i);
        assert!(n.unwrap().contains("Hauptsystem"));
        assert_eq!(st.mode, Mode::Discharge);
    }

    #[test]
    fn stoppt_wenn_voll() {
        let c = cfg();
        let mut st = RegState { mode: Mode::Charge, ..Default::default() };
        let mut i = inp(1000, 95.0, 1050.0, -500.0);
        i.reserve_now = Some(100.0);
        let (a, n) = decide(&c, &mut st, &i);
        assert!(a.contains(&Action::SetReserve(60.0)));
        assert!(n.unwrap().contains("95 %"));
    }
}
