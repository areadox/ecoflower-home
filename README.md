# ecoflower-home

Linux-Serveranwendung in Rust mit Weboberfläche und SQLite-Datenbank zur Verwaltung von **EcoFlow-STREAM-Speichern** (Ultra, Ultra X, Pro, AC Pro …) über die offizielle EcoFlow-IoT-API.

Ziel: **jedem Speicher eigene Regeln geben**, z. B. eigene Entladegrenzen pro Gerät. Die EcoFlow-App kennt nur systemweite Einstellungen.

> ⚠️ **Wichtig:** Ob ein Nebengerät in einem STREAM-Verbund eine eigene Entladegrenze annimmt oder ob das System sie wieder überschreibt, entscheidet die EcoFlow-Firmware. ecoflower-home prüft das automatisch: Nach jedem Befehl liest das Programm den Wert zurück und meldet **„nicht übernommen“**, falls das Gerät ihn nicht übernimmt. Für diesen Fall gibt es die Schwellwert-Regeln (z. B. den AC-Ausgang eines Speichers bei X % abschalten).

## Funktionen

- **Geräteverwaltung:** Alle Geräte des EcoFlow-Kontos abrufen, Anzeigenamen vergeben, Abfrage pro Gerät an- und ausschalten, Haupt- und Nebengerät erkennen
- **Übersicht:** Ladestand, Batterieleistung, Entlade- und Ladegrenze sowie Backup-Reserve jedes Speichers
- **Verlauf:** Ladestand und gemeldete Entladegrenze der letzten 24 Stunden als Diagramm (in SQLite gespeichert, Aufbewahrung einstellbar)
- **Alle Messwerte (Quotas)** eines Geräts durchsuchbar, z. B. um herauszufinden, welche Werte ein Gerät liefert
- **Befehle senden:** über Vorlagen (Entladegrenze, Ladegrenze, Backup-Reserve, AC-Ausgänge) oder als Roh-JSON
- **Regeln pro Gerät:**
  - **Wert halten:** sendet einen Befehl, bis das Gerät den Sollwert zurückmeldet, z. B. „Küche: Entladegrenze 40 %“. Wird der Wert nicht übernommen, erscheint eine Warnung und das Programm versucht es nur noch stündlich.
  - **Schwellwert:** schaltet beim Erreichen eines Ladestands, mit Hysterese, z. B. „bei ≤ 20 % AC-Ausgang aus, bei ≥ 30 % wieder an“
  - optional mit **Zeitfenster** (auch über Mitternacht, z. B. 22:00–06:00)
- **Testmodus** (Standard): Regeln protokollieren nur, was sie tun würden, und senden nichts
- **Ereignisprotokoll** aller Aktionen und Fehler
- Optionaler **Passwortschutz** (HTTP Basic Auth)

## Voraussetzungen

1. Ein Entwicklerkonto auf **[developer-eu.ecoflow.com](https://developer-eu.ecoflow.com/)**. Dort einen **Access Key** und einen **Secret Key** erstellen. Die Freischaltung kann einige Tage dauern.
2. Die Geräte müssen im selben EcoFlow-Konto eingebunden sein.

## Installation

### Variante A: Docker (empfohlen)

```bash
git clone https://github.com/areadox/ecoflower-home.git
cd ecoflower-home
# In docker-compose.yml WEB_PASSWORD ändern!
docker compose up -d --build
```

Danach ist die Oberfläche unter `http://<server>:8080` erreichbar. Die Datenbank liegt in `./data/`.

**Mit Podman statt Docker:** Der alte `docker-compose` 1.29 (Python) läuft unter Python 3.12 nicht mehr (Fehler `No module named 'distutils'`). Stattdessen `podman-compose` verwenden:

```bash
sudo apt install podman-compose
podman-compose up -d --build
```

Damit der Container nach einem Neustart des Servers wieder startet, zusätzlich einmal `sudo systemctl enable --now podman-restart.service` ausführen.

Hinter einem Reverse-Proxy (Zoraxy, Caddy, nginx …) einfach auf Port 8080 weiterleiten.

### Variante B: Fertige Programmdatei

Aus dem [Release](https://github.com/areadox/ecoflower-home/releases) herunterladen:

- `ecoflower-home-linux-x86_64` für PCs und Server
- `ecoflower-home-linux-aarch64` für ARM64, z. B. Raspberry Pi 4/5 mit 64-Bit-OS

Beide laufen auf jedem Linux mit glibc ≥ 2.28 (Debian 10+, Ubuntu 20.04+). In den Befehlen unten den Dateinamen entsprechend anpassen.

```bash
sudo install -m 755 ecoflower-home-linux-x86_64 /usr/local/bin/ecoflower-home
sudo useradd --system --home /var/lib/ecoflower-home --create-home ecoflower
sudo cp ecoflower-home.service /etc/systemd/system/   # vorher WEB_PASSWORD anpassen
sudo systemctl daemon-reload
sudo systemctl enable --now ecoflower-home
```

### Variante C: Selbst bauen

```bash
cargo build --release
./target/release/ecoflower-home
```

## Konfiguration

Über Umgebungsvariablen:

| Variable        | Standard              | Bedeutung                                         |
|-----------------|-----------------------|---------------------------------------------------|
| `DATABASE_PATH` | `data/ecoflower.db`   | Pfad zur SQLite-Datenbank                         |
| `BIND`          | `0.0.0.0:8080`        | Adresse und Port der Weboberfläche                |
| `WEB_USER`      | `admin`               | Benutzername für die Anmeldung                    |
| `WEB_PASSWORD`  | *(leer = kein Schutz)* | Passwort für die Anmeldung                        |
| `TZ`            | Systemzeit            | Zeitzone für die Zeitfenster der Regeln           |
| `RUST_LOG`      | `info`                | Protokollstufe (`debug` für mehr Details)         |

Alles Weitere stellen Sie in der Weboberfläche unter **Einstellungen** ein: API-Zugangsdaten, API-Region (Europa: `api-e.ecoflow.com`), Abfrage-Intervall, Aufbewahrungsdauer und Testmodus.

## Erste Schritte

1. Unter **Einstellungen** Access Key und Secret Key eintragen und speichern.
2. In der **Übersicht** auf „Geräte von EcoFlow abrufen“ klicken.
3. Ein Gerät öffnen und die **Messwerte** ansehen. Interessant sind vor allem `cmsMinDsgSoc` (Entladegrenze), `cmsMaxChgSoc` (Ladegrenze), `backupReverseSoc`, `bmsBattSoc`/`soc` (Ladestand) und `distributedDeviceStatus` (MASTER/SLAVE).
4. **Test, ob ein Nebengerät eigene Grenzen annimmt:** Beim Nebengerät unter „Befehl senden“ den Befehl „STREAM Entladegrenze“ mit z. B. `30` senden, etwa 30 Sekunden warten und dann auf „Jetzt abfragen“ klicken. Prüfen, ob `cmsMinDsgSoc` beim Nebengerät 30 zeigt und ob sich die Werte beim Hauptgerät mitverändert haben.
5. Je nach Ergebnis unter **Regeln** eine „Wert halten“- oder eine „Schwellwert“-Regel anlegen.
6. Erst wenn alles passt, unter **Einstellungen** den **Testmodus ausschalten**.

## Befehlsvorlagen

Befehle sind JSON-Vorlagen mit den Platzhaltern `{{sn}}` (Seriennummer) und `{{value}}` (Wert). Mitgeliefert werden Vorlagen für STREAM-Geräte im Format der EcoFlow-API:

```json
{"sn":"{{sn}}","cmdId":17,"cmdFunc":254,"dirDest":1,"dirSrc":1,"dest":2,"needAck":true,"params":{"cfgMinDsgSoc":{{value}}}}
```

| Vorlage                | Parameter              | Rücklese-Quota      |
|------------------------|------------------------|---------------------|
| STREAM Entladegrenze   | `cfgMinDsgSoc`         | `cmsMinDsgSoc`      |
| STREAM Ladegrenze      | `cfgMaxChgSoc`         | `cmsMaxChgSoc`      |
| STREAM Backup-Reserve  | `cfgBackupReverseSoc`  | `backupReverseSoc`  |
| STREAM AC-Ausgang 1    | `cfgRelay2Onoff`       | `relay2Onoff`       |
| STREAM AC-Ausgang 2    | `cfgRelay3Onoff`       | `relay3Onoff`       |

Die Backup-Reserve muss laut Gerät mindestens 3 % über der Entladegrenze liegen. Weitere Vorlagen lassen sich unter **Befehle** anlegen.

## Technik

- Rust (axum, tokio, reqwest mit rustls, rusqlite mit gebündeltem SQLite)
- Weboberfläche: reines HTML/CSS/JavaScript, in die Programmdatei eingebettet, kein Build-Schritt nötig
- Die gesamte Anwendung ist eine einzige Datei; alle Daten liegen in einer SQLite-Datei
- API-Signatur: HMAC-SHA256 nach EcoFlow-Dokumentation
- Releases: Jeder Push auf `main` startet den GitHub-Actions-Workflow. Er baut die Programmdateien und legt das Release `v<Version aus Cargo.toml>` an oder aktualisiert es.

## Hinweis

Dies ist ein unabhängiges Projekt und steht in keiner Verbindung zu EcoFlow. Die Nutzung erfolgt auf eigene Verantwortung. Falsche Einstellungen an Speichern können den Betrieb beeinträchtigen.

## Lizenz

MIT
