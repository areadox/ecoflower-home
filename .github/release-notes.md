### Neu in 0.0.9
- **Anzeige lädt/entlädt auf den Gerätekarten korrigiert:** Der Zustand wird jetzt aus der Energiebilanz berechnet (Solar − Abgabe ans Haus − Steckdosen), die alle ~2 Sekunden aktualisiert wird. Bisher kam er aus dem BMS-Status, der seltener und teils falsch gemeldet wird – z. B. „Standby“, während ein AC Pro 300 W ans Haus abgab, oder „entlädt“ bei einem Gerät, das eigentlich lud.
- Auch die angezeigte **Leistung pro Gerät** stimmt jetzt (z. B. Ultra: 678 W statt 222 W)
- Mauszeiger auf die Anzeige zeigt weiterhin die verwendete Quelle

### Update
In der Weboberfläche unter *Einstellungen → Update*.

### Download
- `ecoflower-home-linux-x86_64`: Linux x86_64 (PC/Server), glibc ≥ 2.28
- `ecoflower-home-linux-aarch64`: Linux ARM64 (z. B. Raspberry Pi 4/5), glibc ≥ 2.28

🤖 Generated with [Claude Code](https://claude.com/claude-code)
