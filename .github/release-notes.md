### Neu in 0.0.2
- **Live-Daten über MQTT:** Ladestand jedes einzelnen Speichers und Rolle (Haupt/Neben) direkt vom MQTT-Broker der EcoFlow-Open-API. Die HTTP-Schnittstelle liefert diese Werte bei STREAM nicht.
- **Neue Übersicht:** Systemkarte mit Gesamtladestand, Solar, Netz, Hausverbrauch, Batterieleistung und den (systemweiten) Grenzen; Speicherkarten zeigen Ladestand, AC-Leistung und AC-Ausgänge pro Gerät
- Haupt- und Nebengerät werden auch ohne MQTT erkannt
- HTTP- und MQTT-Werte werden zusammengeführt statt überschrieben
- MQTT-Status oben rechts, MQTT in den Einstellungen abschaltbar
- Nebengeräte ohne eigenen Ladestand zeigen „–“ statt eines falschen Werts

### Update
```bash
wget -O /usr/local/bin/ecoflower-home https://github.com/areadox/ecoflower-home/releases/download/v0.0.2/ecoflower-home-linux-x86_64
systemctl restart ecoflower-home
```
(Für ARM64 `ecoflower-home-linux-aarch64` verwenden.) Die Datenbank wird automatisch übernommen.

### Download
- `ecoflower-home-linux-x86_64`: Linux x86_64 (PC/Server), glibc ≥ 2.28 (Debian 10+, Ubuntu 20.04+)
- `ecoflower-home-linux-aarch64`: Linux ARM64 (z. B. Raspberry Pi 4/5 mit 64-Bit-OS), glibc ≥ 2.28

🤖 Generated with [Claude Code](https://claude.com/claude-code)
