# Wallweave testen

Steam-Bibliotheken und mitgelieferte Projekte werden automatisch erkannt. Video-, Szene- und Web-Hintergründe wurden unter Hyprland geprüft; Details und bekannte Grenzen stehen in [VALIDATION.md](VALIDATION.md).

## 1. Programm öffnen

Im Projektordner:

```sh
./run.sh
```

Der installierte App-Launcher-Eintrag **Wallweave** öffnet dieselbe Version. Ab 0.1.1 wird blockierendes EGL-VSync in Wayland-Sitzungen abgeschaltet, damit auch ein ausgeblendetes Fenster auf den Compositor antwortet.

Bei Startproblemen liegt das Protokoll normalerweise unter `~/.local/state/wallweave/gui.log`, der vorherige Start unter `gui-previous.log` daneben. Bei gesetztem `XDG_STATE_HOME` liegt es dort im Unterordner `wallweave`.

Unter **Settings → Add Steam library** den Ordner auswählen, der `steamapps` enthält. Ein noch nicht erreichbarer Pfad bleibt gespeichert. Nach dem Anschließen **Rescan** drücken.

**Installed** zeigt Steams Download-Status an und prüft alle zehn Sekunden nach. Ab 0.1.3 kannst du vollständig heruntergeladene Projekte unter `steamapps/workshop/downloads/431960` bereits verwenden. Sie tragen den Hinweis **Ready download**. Wähle eines aus und starte **Open window preview** oder **Set as wallpaper**, während Steam den Rest herunterlädt. Die Fertigstellung wird anhand der Download-Zähler und Manifest-Version geprüft; unvollständige Dateien bleiben ausgeblendet.

Links lässt sich nach Typ, Quelle und Kompatibilität filtern. Rot markierte Projekte zeigen rechts einen konkreten Inkompatibilitätsgrund; die Start-Schaltflächen sind dafür gesperrt. Unmarkierte Projekte haben keinen bekannten Blocker, sind aber nicht automatisch vollständig kompatibel.

Ab 0.2.4 gibt es unter **Installed → Sort** zusätzlich **Subscribed: newest first** und **Subscribed: oldest first**. Beim ersten Auswählen werden die tatsächlichen Steam-Abonnementdaten über alle Ergebnisseiten gelesen. Steam muss dafür laufen und online sein; danach bleiben die Daten offline verfügbar. **Sync Steam dates** aktualisiert sie. Wallpaper ohne bekanntes Datum stehen in beiden Richtungen am Ende.

Mit dem Stern oben rechts auf einer Kachel oder **Add favorite** im Detailbereich setzt du einen lokalen Favoriten. **Favorites** links zeigt nur die Favoriten und lässt sich mit den anderen Filtern kombinieren. Erneutes Anklicken des Sterns entfernt ihn. Favoriten und Sortierung bleiben nach einem Neustart erhalten; ein Stern setzt das Wallpaper nicht als Hintergrund. Zum Prüfen auch einen Favoriten in einem fertigen Download setzen und nach der Steam-Installation erneut kontrollieren.

Verschiebt Steam ein laufendes Wallpaper später nach `steamapps/workshop/content/431960`, startet Wallweave den Renderer von dort neu und erhält die Einstellungen sowie den Pausenstatus. Wallweave verschiebt oder kopiert dabei keine Steam-Dateien. Presets mit einem separaten Basis-Wallpaper werden derzeit noch nicht unterstützt.

Alternativ über die CLI, mit deinem tatsächlichen Mount-Pfad:

```sh
./run.sh config add-library "/mnt/DEIN-LAUFWERK/SteamLibrary"
./run.sh doctor
./run.sh scan
```

Die gemeinsamen Dateien sollten unter `steamapps/common/wallpaper_engine/assets` gefunden werden. Falls nicht, in Settings den Assets-Ordner angeben.

## 2. Steam Workshop verwenden (ab 0.2.0)

Den nativen Linux-Steam-Client starten und mit deinem Wallpaper-Engine-Konto anmelden. In Wallweave **Workshop** öffnen: Die Verbindung nutzt die laufende Anmeldung, ohne Passwort oder API-Schlüssel in Wallweave. Unter **Sort by** gibt es Popular Today, Week, Month, Year und All Time sowie Newest und My subscriptions. **Show NSFW** blendet Mature-/Questionable-Inhalte ein; die Steam-Kontoeinstellungen gelten weiterhin. Beide Einstellungen werden gespeichert. Suche nach Begriffen, Workshop-IDs oder einem Workshop-Link; links stehen Typ, Auflösung, Kategorie und **My subscriptions** zur Auswahl.

Eine Kachel auswählen und rechts **Download & subscribe** drücken. Steam abonniert das Wallpaper und lädt es mit normaler Priorität in seine Bibliothek. Die Warteschlange rechts zeigt Fortschritt und Installationsstatus. Danach über **Show in Installed** oder **Open in Installed** zur bestehenden Monitor-Auswahl wechseln. Bei Verbindungsfehlern Steam prüfen und **Connect to Steam** drücken; fehlgeschlagene Downloads haben einen **Retry**-Knopf.

```sh
./run.sh workshop status
./run.sh workshop search "forest"
./run.sh workshop search --sort subscriptions --json
```

Der Download-Knopf verändert deine Steam-Abonnements. Die Übertragung nutzt normale Priorität. Steams Einstellung für Downloads während eines laufenden Spiels kann andere Downloads während der Workshop-Verbindung pausieren. Steam offen lassen; bei geschlossenem Wallweave bleiben die Abonnements erhalten. Während der Workshop-Verbindung kann Steam Wallpaper Engine als laufend anzeigen.

Für eine aktualisierte lokale Programmdatei samt Steam-Runtime:

```sh
python3 scripts/build-app.py
```

`bin/wallweave` und `bin/libsteam_api.so` gehören zusammen. Der bestehende Desktop-Launcher verwendet diese Version.

Fehlende Dateien trotz alter Steam-Zähler werden ab 0.2.1 gesondert angezeigt. **Restore missing downloads** lädt diese IDs erneut; die Warteschlange wird gespeichert und beim nächsten Öffnen fortgesetzt. Fertige Zwischenstände werden zuvor unter `<SteamLibrary>/.wallweave/wallpapers/431960` gesichert, damit Steams Aufräumen sie nicht wieder entfernt.

## 3. Renderer kontrollieren

```sh
./run.sh doctor --json
```

Die vorbereitete portable Version hat Vorrang vor dem bereits systemweit installierten `linux-wallpaperengine`, sofern in Settings kein eigener Renderer festgelegt ist. Die Ausgabe nennt den tatsächlich verwendeten Pfad. Die V8-Unterstützung kommt aus diesem Renderer, nicht aus dem Rust-Frontend.

## 4. Als Hintergrund setzen

Beim Start erscheint ab 0.2.3 oben eine animierte Monitorauswahl mit der tatsächlichen Anordnung. Einen Monitor anklicken, danach eine Kachel unter **Installed** auswählen und **Set on DP-2** (mit dem gewählten Anschluss) drücken. Über **Display: …** oben oder **Display → Choose…** rechts lässt sich die Übersicht erneut öffnen. Die Auswahl allein verändert noch kein Wallpaper. **Open window preview** öffnet eine getrennte Fenstervorschau. Deine erkannten Hyprland-Anschlüsse sind `HDMI-A-1`, `DP-1` und `DP-2`; das Programm fragt sie beim Start erneut ab.

```sh
./run.sh preview WORKSHOP-ID
./run.sh apply WORKSHOP-ID --output DP-2
./run.sh stop --output DP-2
```

`WORKSHOP-ID` ersetzen. Auch der vollständige Ordnerpfad eines Wallpapers wird akzeptiert. Vorschauen lassen sich mit `./run.sh stop --output preview` schließen.

## 5. Bei einem fehlerhaften JS-Wallpaper festhalten

- Workshop-Link oder ID und Projekttyp aus `./run.sh inspect WORKSHOP-ID`.
- Was im Original passiert und was unter Linux fehlt.
- Renderer-Pfad und Desktop aus `doctor --json`.
- Renderer-Log aus **Activity → Renderer log**.
- Ob der Fehler auch in der Fenstervorschau auftritt.

Damit lässt sich unterscheiden, ob Browser-APIs, SceneScript, der Szenenrenderer oder die Desktop-Anbindung fehlen. Genau diese Beispiele sind die Grundlage für die nächsten Kompatibilitätsänderungen.

## Automatische Pause für Apps und Spiele

Unter **Einstellungen → Wiedergabe → Automatisch pausieren** stehen drei Regeln zur Wahl:

- **Wenn eine App im Fokus ist**: pausieren bei einer fokussierten App, einem Vollbildspiel oder einem maximierten Fenster.
- **Nur bei Vollbild oder maximierten Apps**: normale Fenster lösen keine Pause aus.
- **Nie automatisch pausieren** (Standard): diese automatische Pause ausschalten.

Die manuelle Pause und die Hyprland-Automatik benötigen einen Renderer mit `--pause-control-file` (siehe [Build-Anleitung](USAGE.md#automatic-performance-pause)). Ein älterer Renderer läuft unter Hyprland ohne Automatik weiter; manuelle Pause wird abgewiesen. Ganze Renderer-Prozesse werden nicht mehr eingefroren, damit Audio-Threads weiterarbeiten. Die gespeicherte Einstellung bleibt bei Updates erhalten.

Unter Hyprland prüft der Supervisor alle 250 ms die sichtbaren Arbeitsflächen pro Monitor. Das Ausblenden einer Sonderarbeitsfläche (zum Beispiel mit Super+M oder Super+S) gibt die automatische Pause wieder frei. Schlägt die Erkennung fehl, werden nur automatische Pausen aufgehoben. Auf anderen Desktops übernimmt die native Renderer-Erkennung. Beides funktioniert auch bei geschlossenem Wallweave-Fenster. Bei normalen Fenstern zählt der Fokus, nicht jedes teilweise überlappende Fenster. Die Fenstervorschau bleibt unabhängig. Eine manuelle Pause bleibt bestehen, bis du selbst **Resume** drückst; anschließend gelten wieder die automatischen Regeln.

**Änderungen speichern** startet die betroffenen Wallpaper mit der neuen Regel neu. Einstellungen und manuelle Pausen bleiben erhalten. Bei einem fehlgeschlagenen Start bleibt der alte Renderer aktiv. **Activity** zeigt unter Hyprland auch **Paused automatically** an. Bei nativer Renderer-Erkennung auf anderen Desktops ist nur die gewählte Regel bekannt.

Zum Prüfen ein normales Fenster fokussieren, Vollbild ein-/ausschalten und auf einen leeren Desktop wechseln. Andere Monitore und eine laufende Vorschau mitprüfen. Danach manuell pausieren, die Regel ändern und kontrollieren, dass die manuelle Pause erhalten bleibt.

```sh
./run.sh config auto-pause focused
./run.sh config auto-pause fullscreen
./run.sh config auto-pause off
```

Ein alter, noch laufender Supervisor muss nach dem Update einmal mit `./run.sh shutdown` beendet werden. Beim erneuten Öffnen stellt Wallweave die gespeicherten Wallpaper mit der neuen Regel wieder her. Die separate Datei `auto-pause.json` im Wallweave-Zustandsordner hält die bisherige `config.toml` kompatibel.

## 6. Alles beenden

```sh
./run.sh shutdown
```

Dies beendet nur den Wallweave-Supervisor und seine eigenen Wallpaper-Prozesse. Andere Wallpaper-Programme bleiben unberührt. Autostart wird nicht eingerichtet.
