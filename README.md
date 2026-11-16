# ecoflower-home

Linux-Serveranwendung in Rust mit Weboberfläche und SQLite-Datenbank zur Verwaltung von **EcoFlow-STREAM-Speichern** (Ultra, Ultra X, Pro, AC Pro …) über die offizielle EcoFlow-IoT-API.

Ziel: **jedem Speicher eigene Regeln geben**, z. B. eigene Entladegrenzen pro Gerät. Die EcoFlow-App kennt nur systemweite Einstellungen.

> ⚠️ **Wichtig:** Ob ein Nebengerät in einem STREAM-Verbund eine eigene Entladegrenze annimmt oder ob das System sie wieder überschreibt, entscheidet die EcoFlow-Firmware. ecoflower-home prüft das automatisch: Nach jedem Befehl liest das Programm den Wert zurück und meldet **„nicht übernommen“**, falls das Gerät ihn nicht übernimmt. Für diesen Fall gibt es die Schwellwert-Regeln (z. B. den AC-Ausgang eines Speichers bei X % abschalten).

## Funktionen

- **Geräteverwaltung:** Alle Geräte des EcoFlow-Kontos abrufen, Anzeigenamen vergeben, Abfrage pro Gerät an- und ausschalten, Haupt- und Nebengerät erkennen
- **Systemansicht:** Die Übersicht ist nach EcoFlow-Systemen gruppiert. Jedes System hat Systemwerte (Gesamtladestand, Solar, Netz, Hausverbrauch, Batterieleistung, Betriebsmodus, Grenzen, Backup-Reserve) und darunter seine Geräte, Hauptgerät zuerst. Die Zuordnung erfolgt automatisch über die von EcoFlow gemeldete `systemGroupId` (MQTT) oder von Hand; Systeme lassen sich umbenennen.
- **AC-Ausgänge:** Leistung beider Steckdosen pro Gerät (`offgrid1/2ActivePower`) und **Ein-/Aus-Schalter** direkt auf der Gerätekarte (`cfgRelay2Onoff` = Ausgang 1, `cfgRelay3Onoff` = Ausgang 2; Ausschalten mit Sicherheitsabfrage)
- **Pro Speicher:** Ladestand, **aktueller Zustand (lädt / entlädt / Standby mit Leistung)**, AC-Leistung und AC-Ausgänge
- **Live-Daten über MQTT:** Die HTTP-Schnittstelle von EcoFlow liefert bei STREAM nur wenige Systemwerte. Der Ladestand der einzelnen Speicher und die Rolle (Haupt/Neben) kommen über den MQTT-Broker der Open-API. ecoflower-home verbindet sich automatisch damit.
- **Grafiken** (eigene Seite): Ladestand und Akkuleistung (lädt/entlädt) aller Speicher sowie der Stromzähler über 6 Stunden bis 30 Tage, mit Tooltip beim Überfahren, Speicher ein-/ausblendbar, dazu eine **Energiebilanz** (geladen/entladen je Speicher, Bezug/Einspeisung am Zähler in kWh). Die Akkuleistung pro Speicher wird ab Version 0.0.11 aufgezeichnet.
- **Verlauf pro Gerät:** Ladestand und gemeldete Entladegrenze der letzten 24 Stunden auf der Geräteseite (in SQLite gespeichert, Aufbewahrung einstellbar)
- **Navigation:** Übersicht und Grafiken direkt, alles Weitere (Regler, Regeln, Befehle, Ereignisse, Einstellungen) im Menü
- **Alle Messwerte (Quotas)** eines Geräts durchsuchbar, z. B. um herauszufinden, welche Werte ein Gerät liefert
- **Befehle senden:** über Vorlagen (Entladegrenze, Ladegrenze, Backup-Reserve, AC-Ausgänge) oder als Roh-JSON
- **Regeln pro Gerät:**
  - **Wert halten:** sendet einen Befehl, bis das Gerät den Sollwert zurückmeldet, z. B. „Küche: Entladegrenze 40 %“. Wird der Wert nicht übernommen, erscheint eine Warnung und das Programm versucht es nur noch stündlich.
  - **Schwellwert:** schaltet beim Erreichen eines Ladestands, mit Hysterese, z. B. „bei ≤ 20 % AC-Ausgang aus, bei ≥ 30 % wieder an“
  - optional mit **Zeitfenster** (auch über Mitternacht, z. B. 22:00–06:00)
- **Regler für einen Einzelspeicher** (z. B. AC Pro in einem eigenen System ohne Zähler): misst mit dem Shelly, gibt oberhalb einer Mindestreserve genau so viel ab, wie das Haus braucht, und lädt nur aus Überschuss – stoppt das Laden, sobald dafür Netzstrom oder Strom aus dem Hauptsystem nötig wäre (siehe unten)
- **Testmodus** (Standard): Regeln protokollieren nur, was sie tun würden, und senden nichts
- **Stromzähler Shelly 3EM (Gen1) und Shelly Pro 3EM (Gen2):** Generation wird automatisch erkannt, direkt im Heimnetz abgefragt (IP-Adresse und optionales Passwort in den Einstellungen, mit Verbindungstest), Live-Anzeige von Bezug/Einspeisung und Phasen in der Übersicht, Verlauf in der Datenbank
- **Ereignisprotokoll** aller Aktionen und Fehler
- **Updates per Klick** in der Weboberfläche (GitHub-Release, mit Prüfsummenkontrolle)
- **Daten-Download** zur Fehlersuche (alle Werte, letzte MQTT-Nachrichten, Regeln, Ereignisse – ohne Secret Key)
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

**In einem LXC-Container (z. B. Proxmox)** fehlt oft `/dev/net/tun`. Dann bricht der Build mit `slirp4netns failed: open("/dev/net/tun")` ab. In diesem Fall am einfachsten Variante B (fertige Programmdatei mit systemd) verwenden.

Hinter einem Reverse-Proxy (Zoraxy, Caddy, nginx …) einfach auf Port 8080 weiterleiten.

### Variante B: Fertige Programmdatei (empfohlen für Updates per Weboberfläche)

Aus dem [Release](https://github.com/areadox/ecoflower-home/releases) herunterladen:

- `ecoflower-home-linux-x86_64` für PCs und Server
- `ecoflower-home-linux-aarch64` für ARM64, z. B. Raspberry Pi 4/5 mit 64-Bit-OS

Beide laufen auf jedem Linux mit glibc ≥ 2.28 (Debian 10+, Ubuntu 20.04+).

```bash
sudo useradd --system --home /opt/ecoflower-home --create-home ecoflower
sudo -u ecoflower wget -O /opt/ecoflower-home/ecoflower-home \
  https://github.com/areadox/ecoflower-home/releases/latest/download/ecoflower-home-linux-x86_64
sudo chmod 755 /opt/ecoflower-home/ecoflower-home
sudo cp ecoflower-home.service /etc/systemd/system/   # vorher WEB_PASSWORD anpassen
sudo systemctl daemon-reload
sudo systemctl enable --now ecoflower-home
```

#### Updates über die Weboberfläche

Unter **Einstellungen → Update** prüft ecoflower-home, ob es auf GitHub eine neue Version gibt. Das passiert auch automatisch alle 6 Stunden; ein Hinweis erscheint dann oben rechts. Ein Klick auf „Update installieren“ lädt die passende Datei herunter, prüft die SHA-256-Prüfsumme, ersetzt die Programmdatei und startet das Programm neu. Die vorherige Version bleibt als `ecoflower-home.alt` im selben Ordner liegen.

Dafür muss der Dienstbenutzer in den Ordner der Programmdatei schreiben dürfen. Das ist bei der Installation oben (`/opt/ecoflower-home`, gehört `ecoflower`) der Fall. **Umzug einer älteren Installation** aus `/usr/local/bin`:

```bash
sudo systemctl stop ecoflower-home
sudo mkdir -p /opt/ecoflower-home/data
sudo mv /usr/local/bin/ecoflower-home /opt/ecoflower-home/
sudo cp /var/lib/ecoflower-home/ecoflower.db* /opt/ecoflower-home/data/ 2>/dev/null
sudo chown -R ecoflower: /opt/ecoflower-home
sudo cp ecoflower-home.service /etc/systemd/system/   # WEB_PASSWORD wieder eintragen
sudo systemctl daemon-reload
sudo systemctl start ecoflower-home
```

In Docker/Podman-Containern ist das Selbst-Update abgeschaltet; dort das Image neu bauen.

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

Alles Weitere stellen Sie in der Weboberfläche unter **Einstellungen** ein: API-Zugangsdaten, API-Region (Europa: `api-e.ecoflow.com`), Abfrage-Intervall, Aufbewahrungsdauer, MQTT-Live-Daten und Testmodus.

Für MQTT braucht der Server eine ausgehende Verbindung zum EcoFlow-Broker (Port 8883, TLS). Die Zugangsdaten dafür holt das Programm selbst über die API. Ob die Verbindung steht, zeigt oben rechts das Abzeichen „MQTT verbunden“.

## Erste Schritte

1. Unter **Einstellungen** Access Key und Secret Key eintragen und speichern.
2. In der **Übersicht** auf „Geräte von EcoFlow abrufen“ klicken.
3. Ein Gerät öffnen und die **Messwerte** ansehen. Interessant sind vor allem `cmsMinDsgSoc` (Entladegrenze), `cmsMaxChgSoc` (Ladegrenze), `backupReverseSoc`, `bmsBattSoc`/`soc` (Ladestand) und `distributedDeviceStatus` (MASTER/SLAVE).
4. **Test, ob ein Nebengerät eigene Grenzen annimmt:** Beim Nebengerät unter „Befehl senden“ den Befehl „STREAM Entladegrenze“ mit z. B. `30` senden, etwa 30 Sekunden warten und dann auf „Jetzt abfragen“ klicken. Prüfen, ob `cmsMinDsgSoc` beim Nebengerät 30 zeigt und ob sich die Werte beim Hauptgerät mitverändert haben.
5. Je nach Ergebnis unter **Regeln** eine „Wert halten“- oder eine „Schwellwert“-Regel anlegen.
6. Erst wenn alles passt, unter **Einstellungen** den **Testmodus ausschalten**.

## Was die EcoFlow-API bei STREAM liefert

Erkenntnisse aus dem Betrieb mit Ultra X (Hauptgerät), Ultra und zwei AC Pro:

- **HTTP** (`quota/all`) liefert pro Gerät nur 15 Werte. Fast alle davon sind **Systemwerte**, die nur das Hauptgerät sinnvoll meldet (`cmsBattSoc`, `powGetPvSum`, `powGetSysGrid`, `powGetSysLoad`, `powGetBpCms`). Nebengeräte melden dort 0. **Pro Gerät** sind nur `gridConnectionPower` sowie die AC-Ausgänge `relay2Onoff`/`relay3Onoff` dabei.
- **Haupt- oder Nebengerät:** ohne MQTT daran erkannt, dass nur das Hauptgerät einen `cmsBattSoc` über 0 meldet
- **Ladestand pro Speicher** (`bmsBattSoc`/`soc`) kommt nur über **MQTT**
- **Systemzugehörigkeit** (`systemGroupId`) und Rolle kommen über MQTT. Bei Nebengeräten steht `distributedDeviceStatus` oft auf `ELECTION`; verlässlich ist `seriesConnectDeviceStatus` (`MASTER`/`SLAVE`).
- **Einzelsystem ohne Zähler** (z. B. ein AC Pro allein): Liegt die Backup-Reserve über dem Ladestand, lädt das Gerät mit voller Leistung (`powSysAcInMax`, beim AC Pro ca. 1050 W) **aus dem Netz** bis zur Reserve. Als Hausverbrauch nimmt es ohne Zähler einen festen Wert an (`emsSetRatedPower`, z. B. 600 W).
- **Backup-Reserve, Grenzen und Betriebsmodus** gelten pro EcoFlow-System. Ein Speicher, der eigene Werte braucht, muss in der EcoFlow-App in ein eigenes System. Ein Smart Meter (z. B. Shelly 3EM) lässt sich dabei nur einem System zuordnen.
- **Zustand pro Speicher** (lädt/entlädt/Standby) aus der **Energiebilanz** der MQTT-Werte (alle ~2 s): Akku = Solar (`powGetPv`…`powGetPv4`) − Netzanschluss (`gridConnectionPower`, positiv = Abgabe ans Haus) − Steckdosen (`offgrid1/2ActivePower`). Die BMS-Werte (`bmsChgDsgState`, `amp`) sind dafür ungeeignet: Sie kommen seltener und erfassen teils nur einen Akkublock – im Betrieb meldeten sie „Standby“, während ein AC Pro 300 W abgab. Sie dienen nur noch als Ersatz. Ohne MQTT ist der Zustand nur beim Hauptgerät bekannt (Vorzeichen von `powGetBpCms`, positiv = Laden).
- **Backup-Reserve** gilt immer für das ganze System. Wird sie an einem Gerät gesetzt, übernehmen alle Geräte den Wert.
- Lade- und Entladegrenze melden alle Geräte gleich. Ob ein Nebengerät eine eigene Grenze annimmt, zeigt die Regel „Wert halten“ (Status „übernommen“ oder „nicht übernommen“).

## Befehlsvorlagen

Befehle sind JSON-Vorlagen mit den Platzhaltern `{{sn}}` (Seriennummer) und `{{value}}` (Wert). Mitgeliefert werden Vorlagen für STREAM-Geräte im Format der EcoFlow-API:

```json
{"sn":"{{sn}}","cmdId":17,"cmdFunc":254,"dirDest":1,"dirSrc":1,"dest":2,"needAck":true,"params":{"cfgMinDsgSoc":{{value}}}}
```

| Vorlage                | Parameter              | Rücklese-Quota      |
|------------------------|------------------------|---------------------|
| STREAM Entladegrenze   | `cfgCmsMinDsgSoc`      | `cmsMinDsgSoc`      |
| STREAM Ladegrenze      | `cfgCmsMaxChgSoc`      | `cmsMaxChgSoc`      |
| STREAM Backup-Reserve  | `cfgBackupReverseSoc`  | `backupReverseSoc`  |
| STREAM AC-Ausgang 1    | `cfgRelay2Onoff`       | `relay2Onoff`       |
| STREAM AC-Ausgang 2    | `cfgRelay3Onoff`       | `relay3Onoff`       |
| STREAM Einspeiselimit  | `cfgFeedGridModePowLimit` | `feedGridModePowLimit` |

Bestätigt funktionieren Backup-Reserve, AC-Ausgänge und das Einspeiselimit (Test: Limit 200 W → Gerät gibt 200 W ab). Die Feldnamen folgen dem Muster „`cfg` + Quota-Name“; Entladegrenze, Ladegrenze und Einspeiselimit sind danach gebildet, aber noch nicht an echten Geräten bestätigt – die Regel „Wert halten“ zeigt, ob ein Gerät sie übernimmt.

Die Backup-Reserve muss laut Gerät mindestens 3 % über der Entladegrenze liegen. Weitere Vorlagen lassen sich unter **Befehle** anlegen.

## Technik

- Rust (axum, tokio, reqwest mit rustls, rumqttc für MQTT, rusqlite mit gebündeltem SQLite)
- Weboberfläche: reines HTML/CSS/JavaScript, in die Programmdatei eingebettet, kein Build-Schritt nötig
- Die gesamte Anwendung ist eine einzige Datei; alle Daten liegen in einer SQLite-Datei
- API-Signatur: HMAC-SHA256 nach EcoFlow-Dokumentation
- Releases: Jeder Push auf `main` startet den GitHub-Actions-Workflow. Er baut die Programmdateien und legt das Release `v<Version aus Cargo.toml>` an oder aktualisiert es.

## Hinweis

Dies ist ein unabhängiges Projekt und steht in keiner Verbindung zu EcoFlow. Die Nutzung erfolgt auf eigene Verantwortung. Falsche Einstellungen an Speichern können den Betrieb beeinträchtigen.

## Lizenz

MIT

## Stromzähler (Shelly)

| Gerät | Erkennung (`/shelly`) | Abfrage | Anmeldung |
|-------|-----------------------|---------|-----------|
| Shelly 3EM (Gen1) | `"type":"SHEM-3"` | `/status` → `emeters[].power`, `total_power` | HTTP-Basic, Benutzer `admin` (oder `benutzer:passwort` ins Passwortfeld) |
| Shelly Pro 3EM (Gen2+) | `"gen":2` | `/rpc/EM.GetStatus?id=0` → `total_act_power` (einphasiges Profil: `EM1.GetStatus`) | HTTP-Digest SHA-256, Benutzer `admin` |

Positive Werte bedeuten Bezug aus dem Netz, negative Einspeisung.

## Regler für einen Einzelspeicher

Für einen Speicher, der in der EcoFlow-App ein **eigenes System ohne Zähler** bildet (z. B. damit er eine eigene Backup-Reserve bekommt). Ohne Zähler würde er nur einen festen Hausverbrauch annehmen und unter der Reserve mit voller Leistung aus dem Netz laden. Der Regler übernimmt mit dem Shelly die Messung:

| Situation | Was der Regler tut | Befehl |
|-----------|--------------------|--------|
| Ladestand über der Mindestreserve | Abgabe so nachführen, dass der Zähler auf dem Zielwert (Standard +20 W) steht, höchstens „Max. Abgabe“ | Einspeiselimit (`cfgFeedGridModePowLimit`) |
| Ladestand an/unter der Mindestreserve | Abgabe 0 W, Reserve auf min(Mindestreserve, Ladestand) – damit das Gerät **nicht aus dem Netz** nachlädt | Einspeiselimit, Backup-Reserve |
| Einspeisung ≥ „Laden ab Einspeisung“ für die Verzögerungszeit | Laden starten (das Gerät lädt dann mit voller Leistung, AC Pro ca. 1050 W) | Backup-Reserve 100 % |
| Beim Laden: Netzbezug ≥ „Laden stoppen ab Bezug“ oder Hauptsystem entlädt dafür | Laden stoppen | Backup-Reserve zurück |
| „Laden bis“ erreicht | Laden stoppen | Backup-Reserve zurück |
| Zähler liefert keine Werte | Abgabe 0 W (Sicherheit) | Einspeiselimit |

Die Ladeleistung selbst lässt sich über die API nicht einstellen; deshalb startet der Regler das Laden erst bei ausreichend Überschuss. Der Regler beachtet den **Testmodus** – dort zeigt er unter „Letzte Aktionen“ nur, was er senden würde.
