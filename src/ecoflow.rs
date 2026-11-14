//! Client für die offizielle EcoFlow-IoT-Open-API (developer.ecoflow.com).
//!
//! Jede Anfrage wird mit HMAC-SHA256 signiert:
//! 1. Parameter rekursiv „flach“ machen (`a.b=1`, `liste[0]=x`), nach ASCII sortieren, mit `&` verbinden
//! 2. `accessKey=…&nonce=…&timestamp=…` anhängen
//! 3. HMAC-SHA256 mit dem Secret Key, als Hex-String in den Header `sign`

use anyhow::{anyhow, bail, Context, Result};
use hmac::{Hmac, Mac};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::Sha256;
use std::time::Duration;

#[derive(Clone)]
pub struct EcoFlowClient {
    http: reqwest::Client,
    base_url: String,
    access_key: String,
    secret_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiDevice {
    pub sn: String,
    #[serde(default, rename = "deviceName")]
    pub device_name: Option<String>,
    #[serde(default, rename = "productName")]
    pub product_name: Option<String>,
    #[serde(default)]
    pub online: Option<i64>,
}

impl EcoFlowClient {
    pub fn new(base_url: &str, access_key: &str, secret_key: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            access_key: access_key.to_string(),
            secret_key: secret_key.to_string(),
        })
    }

    fn sign_headers(&self, params: Option<&Value>) -> (String, Vec<(&'static str, String)>) {
        let nonce = rand::thread_rng().gen_range(100_000..1_000_000).to_string();
        let timestamp = chrono::Utc::now().timestamp_millis().to_string();
        let query = params.map(flatten_params).unwrap_or_default();
        let sign = sign(&query, &self.access_key, &nonce, &timestamp, &self.secret_key);
        let headers = vec![
            ("accessKey", self.access_key.clone()),
            ("nonce", nonce),
            ("timestamp", timestamp),
            ("sign", sign),
        ];
        (query, headers)
    }

    async fn send(&self, method: reqwest::Method, path: &str, params: Option<Value>) -> Result<Value> {
        let (query, headers) = self.sign_headers(params.as_ref());
        let mut url = format!("{}{}", self.base_url, path);
        let mut req = if method == reqwest::Method::GET {
            if !query.is_empty() {
                url.push('?');
                url.push_str(&query);
            }
            self.http.get(&url)
        } else {
            let body = params.clone().unwrap_or(Value::Object(Map::new()));
            self.http
                .request(method, &url)
                .header("Content-Type", "application/json;charset=UTF-8")
                .body(serde_json::to_string(&body)?)
        };
        for (k, v) in headers {
            req = req.header(k, v);
        }
        let resp = req.send().await.with_context(|| format!("Anfrage an {path} fehlgeschlagen"))?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            bail!("HTTP {status}: {text}");
        }
        let json: Value = serde_json::from_str(&text).with_context(|| format!("Ungültige Antwort: {text}"))?;
        Ok(json)
    }

    fn check_code(json: &Value) -> Result<()> {
        let code = match &json["code"] {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        };
        if code != "0" {
            let msg = json["message"].as_str().unwrap_or("unbekannter Fehler");
            bail!("EcoFlow-Fehler {code}: {msg}");
        }
        Ok(())
    }

    /// Alle Geräte des Kontos.
    pub async fn device_list(&self) -> Result<Vec<ApiDevice>> {
        let json = self.send(reqwest::Method::GET, "/iot-open/sign/device/list", None).await?;
        Self::check_code(&json)?;
        let list = json["data"].clone();
        let devices: Vec<ApiDevice> =
            serde_json::from_value(list).map_err(|e| anyhow!("Geräteliste nicht lesbar: {e}"))?;
        Ok(devices)
    }

    /// Alle Messwerte („Quotas“) eines Geräts.
    pub async fn quota_all(&self, sn: &str) -> Result<Map<String, Value>> {
        let params = serde_json::json!({ "sn": sn });
        let json = self
            .send(reqwest::Method::GET, "/iot-open/sign/device/quota/all", Some(params))
            .await?;
        Self::check_code(&json)?;
        match json["data"].clone() {
            Value::Object(m) => Ok(m),
            _ => bail!("Antwort enthält keine Daten"),
        }
    }

    /// Befehl an ein Gerät senden. Gibt die rohe Antwort zurück.
    pub async fn set_quota(&self, command: Value) -> Result<Value> {
        if command.get("sn").and_then(|v| v.as_str()).unwrap_or("").is_empty() {
            bail!("Befehl enthält keine Seriennummer (sn)");
        }
        let json = self
            .send(reqwest::Method::PUT, "/iot-open/sign/device/quota", Some(command))
            .await?;
        Self::check_code(&json)?;
        Ok(json)
    }
}

/// Parameter nach EcoFlow-Regeln flach machen und sortieren.
pub fn flatten_params(value: &Value) -> String {
    let mut out = Vec::new();
    if let Value::Object(map) = value {
        for (k, v) in map {
            flatten_into(k, v, &mut out);
        }
    }
    out.sort();
    out.join("&")
}

fn flatten_into(prefix: &str, value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                flatten_into(&format!("{prefix}.{k}"), v, out);
            }
        }
        Value::Array(arr) => {
            for (i, v) in arr.iter().enumerate() {
                flatten_into(&format!("{prefix}[{i}]"), v, out);
            }
        }
        Value::String(s) => out.push(format!("{prefix}={s}")),
        Value::Number(n) => out.push(format!("{prefix}={n}")),
        Value::Bool(b) => out.push(format!("{prefix}={b}")),
        Value::Null => {}
    }
}

pub fn sign(query: &str, access_key: &str, nonce: &str, timestamp: &str, secret_key: &str) -> String {
    let mut target = format!("accessKey={access_key}&nonce={nonce}&timestamp={timestamp}");
    if !query.is_empty() {
        target = format!("{query}&{target}");
    }
    let mut mac = Hmac::<Sha256>::new_from_slice(secret_key.as_bytes()).expect("HMAC akzeptiert jede Schlüssellänge");
    mac.update(target.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Zahl aus einem Quota-Wert lesen (Zahl, Bool oder Text).
pub fn as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn flatten_wie_in_der_ecoflow_doku() {
        let v = json!({
            "name": "demo1",
            "ids": [1, 2, 3],
            "deviceInfo": {"id": 1},
            "deviceList": [{"id": 1}, {"id": 2}]
        });
        assert_eq!(
            flatten_params(&v),
            "deviceInfo.id=1&deviceList[0].id=1&deviceList[1].id=2&ids[0]=1&ids[1]=2&ids[2]=3&name=demo1"
        );
    }

    #[test]
    fn signatur_ohne_parameter() {
        // HMAC-SHA256("accessKey=&nonce=123456&timestamp=1671171709428", "secret")
        let s = sign("", "", "123456", "1671171709428", "secret");
        let mut mac = Hmac::<Sha256>::new_from_slice(b"secret").unwrap();
        mac.update(b"accessKey=&nonce=123456&timestamp=1671171709428");
        assert_eq!(s, hex::encode(mac.finalize().into_bytes()));
    }

    #[test]
    fn befehl_wird_korrekt_flach() {
        let v = json!({"sn":"X","cmdId":17,"cmdFunc":254,"dirDest":1,"dirSrc":1,"dest":2,"needAck":true,"params":{"cfgMinDsgSoc":20}});
        assert_eq!(
            flatten_params(&v),
            "cmdFunc=254&cmdId=17&dest=2&dirDest=1&dirSrc=1&needAck=true&params.cfgMinDsgSoc=20&sn=X"
        );
    }
}
