### Neu in 0.0.6
- **Stromzähler Shelly Pro 3EM:** unter *Einstellungen* IP-Adresse und (falls im Shelly eingeschaltet) Passwort eintragen, mit **Verbindungstest**. Abfrage direkt im Heimnetz, Standard alle 2 Sekunden – deutlich schneller als über die EcoFlow-Cloud.
- **Live-Anzeige in der Übersicht:** Bezug bzw. Einspeisung und die drei Phasen, aktualisiert alle 3 Sekunden; Verlauf wird alle 30 Sekunden gespeichert
- **Rolle Haupt/Neben korrigiert:** Nebengeräte wurden als „ELECTION“ angezeigt, jetzt wird `seriesConnectDeviceStatus` mit ausgewertet
- **Befehlsvorlagen korrigiert:** Entlade- und Ladegrenze verwenden jetzt `cfgCmsMinDsgSoc`/`cfgCmsMaxChgSoc` (gleiches Muster wie die funktionierende Backup-Reserve); bestehende Vorlagen werden automatisch angepasst
- **Neue Vorlage „STREAM Einspeiselimit“** (`cfgFeedGridModePowLimit`) zum Testen
- Stromzähler im Daten-Download (ohne Passwort)

### Update
In der Weboberfläche unter *Einstellungen → Update*.

### Download
- `ecoflower-home-linux-x86_64`: Linux x86_64 (PC/Server), glibc ≥ 2.28
- `ecoflower-home-linux-aarch64`: Linux ARM64 (z. B. Raspberry Pi 4/5), glibc ≥ 2.28

🤖 Generated with [Claude Code](https://claude.com/claude-code)
