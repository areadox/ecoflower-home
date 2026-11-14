Erste Version von **ecoflower-home**: Linux-Server mit Weboberfläche und SQLite zur Verwaltung von EcoFlow-STREAM-Speichern über die EcoFlow-IoT-API.

### Enthalten
- Geräte vom EcoFlow-Konto abrufen, Anzeigenamen vergeben, Haupt- und Nebengerät erkennen
- Übersicht mit Ladestand, Batterieleistung, Entlade- und Ladegrenze sowie Backup-Reserve
- Verlauf des Ladestands (24 h) und alle Messwerte durchsuchbar
- Befehle senden (Vorlagen oder Roh-JSON)
- Regeln pro Gerät: **Wert halten** mit Rücklese-Prüfung („nicht übernommen“-Warnung) und **Schwellwert** mit Hysterese, optional mit Zeitfenster
- Testmodus (Standard), Ereignisprotokoll, optionaler Passwortschutz
- Dockerfile, docker-compose und systemd-Unit

### Download
- `ecoflower-home-linux-x86_64`: Linux x86_64 (PC/Server), glibc ≥ 2.28 (Debian 10+, Ubuntu 20.04+)
- `ecoflower-home-linux-aarch64`: Linux ARM64 (z. B. Raspberry Pi 4/5 mit 64-Bit-OS), glibc ≥ 2.28
- Für Docker: aus dem Quellcode bauen, siehe README

⚠️ Ob Nebengeräte im STREAM-Verbund eigene Grenzen annehmen, ist noch nicht an echter Hardware getestet. Das Programm zeigt es nach dem Senden eines Befehls an.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
