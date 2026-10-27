# Wallweave unter Linux installieren

[English →](INSTALL.md) · [Zur README](../README.md)

## Voraussetzungen

- **Linux x86_64 mit glibc 2.35 oder neuer**, beispielsweise Ubuntu 22.04 oder eine neuere kompatible Distribution.
- Eine grafische Wayland- oder X11-Sitzung mit funktionierenden EGL/OpenGL-Treibern.
- Eine eigene Wallpaper-Engine-Installation mit dem Ordner `assets`.
- Ein separater `linux-wallpaperengine`-Renderer; Einrichtung unten.
- Für Workshop-Suche und Downloads: den laufenden **nativen Linux-Steam-Client**, angemeldet mit einem Konto, das Wallpaper Engine besitzt.
- **Python 3.10+** für die optionalen Installationsskripte.

Die Wiedergabe wurde unter Hyprland geprüft. Andere Desktop-Umgebungen sind noch nicht entsprechend getestet. GNOME benötigt zusätzlich die zum Renderer passende Shell-Erweiterung. Nicht jedes Wallpaper und jedes Skript ist kompatibel.

## Arch Linux, EndeavourOS und CachyOS

Die Datei `wallweave-0.2.6-1-x86_64.pkg.tar.zst` aus [Releases](https://github.com/MGCrafter/wallweave/releases/latest) herunterladen:

```sh
sudo pacman -U ./wallweave-0.2.6-1-x86_64.pkg.tar.zst
wallweave
```

Pacman installiert das Programm samt Abhängigkeiten, Menüeintrag und Icon. Den Renderer anschließend separat installieren:

```sh
sudo pacman -S --needed python
python3 /usr/share/wallweave/install-renderer.py --directory "$HOME/.local/share/wallweave/renderer"
```

Den ausgegebenen Programmpfad in **Einstellungen → Dateien & Renderer → Renderer** auswählen. Für den Workshop muss der native Steam-Client laufen; Steam ist unter Arch nach Aktivierung des multilib-Repositories über `sudo pacman -S steam` installierbar. Du benötigst weiterhin Wallpaper Engine und dessen Assets.

Alternativ das `PKGBUILD` aus demselben Release in einen leeren Ordner herunterladen, prüfen und das native Paket selbst erstellen:

```sh
sudo pacman -S --needed base-devel
makepkg -si
```

Dabei wird das passende vorkompilierte Archiv heruntergeladen und anhand seiner festgelegten SHA-256 geprüft. Ein lokaler Rust-Build ist nicht nötig. Das PKGBUILD wird über GitHub angeboten; ein AUR-Eintrag besteht damit noch nicht.

**Aktualisieren:** Oberfläche schließen, `wallweave shutdown` ausführen und das neue Paket erneut mit `sudo pacman -U ./DATEINAME.pkg.tar.zst` installieren.

**Entfernen:**

```sh
wallweave shutdown
sudo pacman -R wallweave
```

Persönliche Einstellungen, Logs, Steam-Dateien und separat installierte Renderer bleiben erhalten.

## AppImage: herunterladen und starten

Die Datei `Wallweave-0.2.6-x86_64.AppImage` unter [Releases](https://github.com/MGCrafter/wallweave/releases/latest) herunterladen, beispielsweise in `~/Applications` ablegen und starten:

```sh
chmod +x Wallweave-0.2.6-x86_64.AppImage
./Wallweave-0.2.6-x86_64.AppImage
```

Falls eine FUSE-Fehlermeldung erscheint:

```sh
./Wallweave-0.2.6-x86_64.AppImage --appimage-extract-and-run
```

Auch CLI-Befehle funktionieren direkt über die Datei:

```sh
./Wallweave-0.2.6-x86_64.AppImage doctor
```

Die App legt beim ersten Start eine versionsbezogene Kopie unter `~/.cache/wallweave/appimage/` an. Dadurch können Wallpaper weiterlaufen, wenn das AppImage und die Oberfläche geschlossen werden. Bei gesetztem `XDG_CACHE_HOME` liegt der Cache dort. Für einen automatisch installierten Menüeintrag die Archiv-Variante verwenden.

## Archiv: optional mit Eintrag im Anwendungsmenü

Die Datei `wallweave-0.2.6-linux-x86_64.tar.gz` aus demselben Release herunterladen:

```sh
tar -xzf wallweave-0.2.6-linux-x86_64.tar.gz
cd wallweave-0.2.6-linux-x86_64
python3 install.py
```

Das installiert Wallweave **nur für deinen Benutzer, ohne sudo**. Danach im Anwendungsmenü **Wallweave** öffnen oder `~/.local/bin/wallweave` ausführen. Der Programmordner liegt unter `~/.local/share/wallweave/app/`; `XDG_DATA_HOME` wird berücksichtigt.

Alternativ lässt sich das entpackte Archiv mit `./run.sh` direkt starten. `bin/wallweave` und `bin/libsteam_api.so` müssen zusammenbleiben.

Falls `wallweave` im Terminal nicht gefunden wird, `~/.local/bin/wallweave` verwenden oder `export PATH="$HOME/.local/bin:$PATH"` im Shell-Profil ergänzen.

## Renderer installieren

`install-renderer.py` ebenfalls aus dem Release herunterladen und ausführen:

```sh
python3 install-renderer.py --directory "$HOME/.local/share/wallweave/renderer"
```

Das Skript lädt den festgelegten Renderer **v0.9.4 mit Wallweaves Auto-Pause-Fix** herunter und prüft seine SHA-256-Prüfsumme. Es zeigt anschließend den Pfad zur ausführbaren Datei. Diesen unter **Einstellungen → Dateien & Renderer → Renderer** auswählen. Es wird dabei noch kein Wallpaper gestartet.

Bei einer Archiv-Installation lässt sich derselbe Renderer auch über die CLI konfigurieren:

```sh
~/.local/bin/wallweave config renderer "$HOME/.local/share/wallweave/renderer/v0.9.4-wallweave-pause1/wallweave-renderer-v0.9.4-pause1-linux-x86_64/linux-wallpaperengine"
~/.local/bin/wallweave doctor
```

Beim AppImage die Programmdatei statt `~/.local/bin/wallweave` verwenden. Ein vorhandener passender Renderer kann direkt in Settings ausgewählt werden.

## Steam verbinden und Wallpaper auswählen

1. Steam starten und Wallpaper Engine in einer Steam-Bibliothek installieren.
2. Wallweave öffnen. Falls die Bibliothek nicht automatisch erkannt wird, unter **Settings → Add Steam library** den Ordner auswählen, der `steamapps` enthält.
3. Falls nötig den Assets-Ordner angeben, normalerweise `steamapps/common/wallpaper_engine/assets`.
4. In der Monitorauswahl den gewünschten Bildschirm auswählen.
5. Ein Wallpaper auswählen und **Apply to …** drücken oder die Kachel doppelt anklicken. **Open window preview** öffnet zunächst eine Fenstervorschau.
6. Unter **Workshop** suchen und über **Download & subscribe** herunterladen. Steam verwaltet Abonnement und Updates.

Bei externen Laufwerken den tatsächlichen Linux-Mountpfad verwenden. Nicht erreichbare Pfade bleiben gespeichert. Nach dem Einhängen **Rescan** drücken.

Beim Schließen der Oberfläche laufen Wallpaper weiter. Beim nächsten Öffnen werden gespeicherte Zuweisungen wiederhergestellt. Autostart bei der Anmeldung wird nicht automatisch eingerichtet.

## Downloads prüfen

`SHA256SUMS` neben die heruntergeladenen Release-Dateien legen:

```sh
sha256sum --ignore-missing -c SHA256SUMS
```

Die vorhandenen Dateien sollten `OK` melden.

## Aktualisieren und entfernen

Vor einem Update oder dem Entfernen die Oberfläche schließen und den bisherigen Supervisor beenden:

```sh
~/.local/bin/wallweave shutdown
```

Beim AppImage entsprechend `./Wallweave-0.2.6-x86_64.AppImage shutdown` verwenden.

**App aktualisieren:** Neues AppImage herunterladen und ausführbar machen oder das neue Archiv entpacken und erneut `python3 install.py` ausführen. Beim Arch-Paket `sudo pacman -U ./wallweave-0.2.6-1-x86_64.pkg.tar.zst` verwenden. Einstellungen, Favoriten und gespeicherte Wallpaper bleiben erhalten.

**Neu in v0.2.6:** Unter **Einstellungen → Wiedergabe → Beim Spielen** die Option **Grafikspeicher beim Spielen freigeben** einschalten und speichern. Sie ist standardmäßig ausgeschaltet. Bei erkannten Spielen werden die Wallpaper auf allen Bildschirmen entladen und danach automatisch wiederhergestellt. Der Renderer aus v0.2.5 bleibt kompatibel und muss nicht erneut installiert werden. Den bisherigen Supervisor vor dem App-Update einmal mit `wallweave shutdown` beenden.

**Zusätzlich beim Update von v0.2.4 oder älter:** Den neuen Renderer installieren, damit der Auto-Pause-Fix verfügbar ist. `install-renderer.py` aus dem neuen Release herunterladen:

```sh
python3 install-renderer.py --directory "$HOME/.local/share/wallweave/renderer" --configure wallweave
```

Bei einer Benutzerinstallation ohne passenden `PATH` hinter `--configure` den Pfad `"$HOME/.local/bin/wallweave"` angeben. Beim AppImage stattdessen `--configure ./Wallweave-0.2.6-x86_64.AppImage` verwenden. Du musst nichts selbst kompilieren. Der Installer prüft die SHA-256-Prüfsumme und die Pause-Unterstützung, erhält den bisherigen Renderer und setzt den neuen Programmpfad in Wallweave.

Danach Wallweave öffnen. Die Wallpaper-Zuweisungen werden wiederhergestellt. Die bisherige Auto-Pause-Einstellung bleibt erhalten; zum Einschalten **Einstellungen → Wiedergabe → Automatisch pausieren** öffnen. Ein automatischer App-Updater ist noch nicht enthalten.

**Archiv-Installation entfernen:**

```sh
python3 "$HOME/.local/share/wallweave/app/scripts/install-app.py" --uninstall
```

Bei gesetztem `XDG_DATA_HOME` den Pfad anpassen. Einstellungen, Logs und separat installierte Renderer bleiben bestehen.

**AppImage entfernen:** Nach `shutdown` die AppImage-Datei und gegebenenfalls ihren Cache unter `~/.cache/wallweave/appimage/` löschen. Bei portabler Nutzung den entpackten Ordner entfernen.

Konfiguration und Logs liegen unter `~/.config/wallweave/` und `~/.local/state/wallweave/`. Steam-Abonnements und Wallpaper in Steam-Bibliotheken werden durch die Deinstallation nicht gelöscht.

Bei Problemen helfen `doctor`, das Startprotokoll `~/.local/state/wallweave/gui.log` und die Renderer-Logs unter **Activity**. Weitere Hinweise stehen in der [ausführlichen Anleitung](INSTALL.md#troubleshooting) und im [Testleitfaden](TESTEN.md).
