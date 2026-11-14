### Neu in 0.0.4
- **Zustand pro Speicher in der Übersicht:** ▲ lädt / ▼ entlädt / ● Standby, jeweils mit Batterieleistung
  - Quelle pro Gerät über MQTT: `bmsChgDsgState`, ersatzweise Akkustrom × Spannung
  - ohne MQTT nur beim Hauptgerät (über die Systemleistung `powGetBpCms`), sonst „unbekannt“
  - Mauszeiger auf die Anzeige zeigt, aus welchem Wert der Zustand stammt

### Update
In der Weboberfläche unter *Einstellungen → Update* (ab 0.0.3 nach dem einmaligen Umzug nach `/opt/ecoflower-home`, siehe README).

### Download
- `ecoflower-home-linux-x86_64`: Linux x86_64 (PC/Server), glibc ≥ 2.28
- `ecoflower-home-linux-aarch64`: Linux ARM64 (z. B. Raspberry Pi 4/5), glibc ≥ 2.28

🤖 Generated with [Claude Code](https://claude.com/claude-code)
