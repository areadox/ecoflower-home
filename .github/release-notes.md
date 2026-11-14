### Neu in 0.0.3
- **Update per Klick:** Unter *Einstellungen → Update* nach neuen Versionen suchen und installieren – mit SHA-256-Prüfung, automatischem Neustart und Sicherung der alten Version (`ecoflower-home.alt`). Alle 6 Stunden wird automatisch geprüft, ein Hinweis erscheint oben rechts.
- **Daten-Download zur Fehlersuche:** Unter *Einstellungen* alle Geräte, Messwerte, die letzten MQTT-Nachrichten, Regeln, Befehle, Verlauf (24 h) und Ereignisse als JSON-Datei – ohne Secret Key und Passwörter.
- systemd-Unit: Programm liegt jetzt in `/opt/ecoflower-home` (gehört dem Dienstbenutzer), `Restart=always`

### Einmalig nötig: Umzug nach /opt
Damit sich das Programm selbst aktualisieren kann, muss es in einem Ordner liegen, der dem Dienstbenutzer gehört. Anleitung in der README unter **„Updates über die Weboberfläche“**. Danach sind keine Konsolenbefehle für Updates mehr nötig.

### Download
- `ecoflower-home-linux-x86_64`: Linux x86_64 (PC/Server), glibc ≥ 2.28
- `ecoflower-home-linux-aarch64`: Linux ARM64 (z. B. Raspberry Pi 4/5), glibc ≥ 2.28

🤖 Generated with [Claude Code](https://claude.com/claude-code)
