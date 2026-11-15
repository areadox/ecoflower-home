### Neu in 0.0.5
- **Systemansicht:** Die Übersicht zeigt jedes EcoFlow-System mit seinen Systemwerten (Gesamtladestand, Solar, Netz, Hausverbrauch, Batterieleistung, Betriebsmodus, Grenzen, Backup-Reserve) und darunter seine Geräte, Hauptgerät zuerst
- **Automatische Zuordnung** der Geräte zu Systemen über die `systemGroupId` aus den MQTT-Live-Daten; neue Systeme werden selbst angelegt
- **Zuordnung von Hand** auf jeder Gerätekarte und auf der Geräteseite (automatisch / System wählen / kein System); „Neues System“, Umbenennen und Entfernen in der Übersicht
- Rolle Haupt/Neben wird jetzt auch erkannt, wenn EcoFlow sie als Zahl meldet
- Bestehende Datenbanken werden beim Start automatisch erweitert

### Update
In der Weboberfläche unter *Einstellungen → Update*.

### Download
- `ecoflower-home-linux-x86_64`: Linux x86_64 (PC/Server), glibc ≥ 2.28
- `ecoflower-home-linux-aarch64`: Linux ARM64 (z. B. Raspberry Pi 4/5), glibc ≥ 2.28

🤖 Generated with [Claude Code](https://claude.com/claude-code)
