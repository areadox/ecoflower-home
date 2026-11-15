### Neu in 0.0.8
- **Regler für einen Einzelspeicher** (neuer Menüpunkt *Regler*) – gedacht für einen Speicher in einem eigenen EcoFlow-System ohne Zähler, z. B. den AC Pro Bad:
  - **gibt ab**, was das Haus braucht: oberhalb der Mindestreserve (Standard 60 %) wird die Abgabe über das Einspeiselimit nachgeführt, bis der Shelly ca. +20 W zeigt (max. 600 W)
  - **lädt nur aus Überschuss**: ab 800 W Einspeisung (60 s lang) wird das Laden gestartet; es stoppt bei Netzbezug oder wenn das Hauptsystem dafür entladen müsste
  - **kein Nachladen aus dem Netz**: unter der Mindestreserve wird die Reserve auf den aktuellen Ladestand gesetzt
  - ohne Zählerwerte wird die Abgabe aus Sicherheitsgründen auf 0 W gesetzt
  - alle Schwellen einstellbar, Live-Status und die letzten Aktionen auf der Seite, Testmodus wird beachtet
- Regler-Einstellungen und -Status im Daten-Download

### Update
In der Weboberfläche unter *Einstellungen → Update*.

### Download
- `ecoflower-home-linux-x86_64`: Linux x86_64 (PC/Server), glibc ≥ 2.28
- `ecoflower-home-linux-aarch64`: Linux ARM64 (z. B. Raspberry Pi 4/5), glibc ≥ 2.28

🤖 Generated with [Claude Code](https://claude.com/claude-code)
