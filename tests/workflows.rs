use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use wallweave::{
    config::{Config, Paths},
    desktop::Desktop,
    library::{self, Wallpaper},
    renderer::{self, Renderer},
    service::{self, Request},
};

fn write(path: impl AsRef<Path>, text: &str) {
    let path = path.as_ref();
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, text).expect("write");
}
fn wallpaper(root: &Path) -> PathBuf {
    let folder = root.join("steamapps/workshop/content/431960/123456");
    write(
        folder.join("project.json"),
        r#"{"title":"Test wallpaper","type":"web","file":"index.html","general":{"properties":{"message":{"type":"textinput","value":"hello"}}}}"#,
    );
    write(
        folder.join("index.html"),
        "<!doctype html><title>fixture</title>",
    );
    folder
}
fn staged_wallpaper(root: &Path) -> PathBuf {
    let installed = wallpaper(root);
    let staged = root.join("steamapps/workshop/downloads/431960/123456");
    fs::create_dir_all(staged.parent().expect("parent")).expect("downloads");
    fs::rename(installed, &staged).expect("stage fixture");
    write(
        root.join("steamapps/workshop/appworkshop_431960.acf"),
        r#""AppWorkshop" {
        "WorkshopItemsInstalled" {}
        "WorkshopItemDetails" { "123456" {
            "manifest" "1" "latest_manifest" "1" "BytesDownloaded" "100" "BytesToDownload" "100"
        } }
    }"#,
    );
    staged
}
fn fake_renderer(root: &Path, fails: bool) -> PathBuf {
    let path = root.join(if fails {
        "failing renderer"
    } else {
        "renderer with spaces"
    });
    let help = "--assets-dir --screen-root --bg --window --fps --set-property --silent --volume --scaling --layer";
    write(
        &path,
        &format!(
            "#!/bin/sh\nif [ \"$1\" = \"--help\" ]; then printf '%s\\n' '{help}'; exit 0; fi\nprintf '%s\\n' \"$@\"\n{}\n",
            if fails {
                "echo intentional-startup-failure >&2; exit 27"
            } else {
                "trap 'exit 0' TERM INT\nsleep 90 &\nwait"
            }
        ),
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}
fn config(root: &Path) -> Config {
    let assets = root.join("steamapps/common/wallpaper_engine/assets");
    fs::create_dir_all(&assets).expect("assets");
    Config {
        discover_steam: false,
        libraries: vec![root.into()],
        assets: Some(assets),
        renderer: Some(fake_renderer(root, false)),
        ..Default::default()
    }
}
fn paths(root: &Path) -> Paths {
    Paths {
        config: root.join("config.toml"),
        state: root.join("state"),
        runtime: root.join("run"),
    }
}

#[test]
fn discovers_external_library_assets_and_keeps_offline_paths() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    wallpaper(&root.join("external library"));
    fs::create_dir_all(root.join("external library/steamapps/common/My Wallpaper Engine/assets"))
        .expect("assets");
    write(
        root.join("external library/steamapps/appmanifest_431960.acf"),
        r#""AppState" { "installdir" "My Wallpaper Engine" }"#,
    );
    write(
        root.join("steam/steamapps/libraryfolders.vdf"),
        &format!(
            "\"libraryfolders\" {{ \"1\" {{ \"path\" \"{}\" }} }}",
            root.join("external library").display()
        ),
    );
    let offline = root.join("offline iscsi");
    let config = Config {
        discover_steam: false,
        libraries: vec![root.join("steam"), offline.clone()],
        ..Default::default()
    };
    let report = library::scan(&config);
    assert_eq!(report.wallpapers.len(), 1);
    assert!(
        report
            .assets
            .expect("assets")
            .ends_with("My Wallpaper Engine/assets")
    );
    assert!(
        report
            .roots
            .iter()
            .any(|r| r.path == offline && !r.available)
    );
    let paths = paths(root);
    paths.save(&config).expect("save");
    assert!(paths.load().expect("load").libraries.contains(&offline));
}

#[test]
fn malformed_projects_do_not_hide_valid_wallpapers_and_aliases_are_deduplicated() {
    let temp = tempfile::tempdir().expect("temp");
    wallpaper(temp.path());
    write(
        temp.path()
            .join("steamapps/workshop/content/431960/broken/project.json"),
        "{",
    );
    symlink(temp.path(), temp.path().join("alias")).expect("symlink");
    let config = Config {
        discover_steam: false,
        libraries: vec![temp.path().into(), temp.path().join("alias")],
        ..Default::default()
    };
    let report = library::scan(&config);
    assert_eq!(report.wallpapers.len(), 1);
    assert_eq!(report.warnings.len(), 1);
}

#[test]
fn steam_favorites_follow_staging_installation_and_preserved_copies() {
    let temp = tempfile::tempdir().expect("temp");
    let mut item = Wallpaper::load(&wallpaper(temp.path())).expect("wallpaper");
    let key = item.favorite_key();
    assert_eq!(item.workshop_id(), Some(123456));
    assert_eq!(key, "workshop:123456");
    for path in [
        "steamapps/workshop/downloads/431960/123456",
        ".wallweave/wallpapers/431960/123456",
        "other-library/steamapps/workshop/content/431960/123456",
    ] {
        item.folder = temp.path().join(path);
        assert_eq!(item.favorite_key(), key);
    }
    item.folder = temp.path().join("local-projects/123456");
    assert_eq!(
        item.workshop_id(),
        None,
        "a numeric local folder is not a Steam item"
    );
    let local_key = item.favorite_key();
    item.folder = temp.path().join("different-local-projects/123456");
    assert_ne!(item.favorite_key(), local_key);
}

#[test]
fn project_entry_cannot_escape_through_traversal_or_symlinks() {
    let temp = tempfile::tempdir().expect("temp");
    let folder = wallpaper(temp.path());
    write(temp.path().join("outside.html"), "outside");
    assert!(library::contained_file(&folder, "../../../../outside.html").is_err());
    symlink(temp.path().join("outside.html"), folder.join("linked.html")).expect("symlink");
    assert!(library::contained_file(&folder, "linked.html").is_err());
    assert!(library::contained_file(&folder, "/etc/passwd").is_err());
}

#[test]
fn packed_scenes_keep_the_logical_entry_and_validate_the_physical_package() {
    let temp = tempfile::tempdir().expect("temp");
    let config = config(temp.path());
    let folder = temp.path().join("steamapps/workshop/content/431960/packed");
    let metadata = folder.join("project.json");
    write(
        &metadata,
        "\u{feff}{\"type\":\"Scene\",\"file\":\"scene.json\",\"title\":\"Packed scene\"}",
    );
    write(folder.join("scene.pkg"), "package fixture");
    let scene = Wallpaper::load(&folder).expect("scene.json may live inside scene.pkg");
    assert_eq!(scene.file, "scene.json");
    assert!(scene.packed);
    assert!(scene.script_description().starts_with("Packed scene"));
    let engine = Renderer::detect(&config).expect("renderer");
    let plan = renderer::plan(
        &engine,
        &config,
        scene.clone(),
        "DP-1",
        false,
        &BTreeMap::new(),
    )
    .expect("packed scene launch plan");
    assert!(plan.arguments.iter().any(|arg| arg == folder.as_os_str()));

    write(temp.path().join("outside.json"), "{}");
    symlink(temp.path().join("outside.json"), folder.join("scene.json")).expect("symlink");
    assert!(
        Wallpaper::load(&folder).is_err(),
        "package fallback must not hide a symlink escape"
    );
    fs::remove_file(folder.join("scene.json")).expect("remove symlink");
    write(&metadata, r#"{"type":"scene","file":"../outside.json"}"#);
    assert!(
        Wallpaper::load(&folder).is_err(),
        "package fallback must not hide traversal"
    );
    fs::remove_file(folder.join("scene.pkg")).expect("remove package");
    assert!(renderer::plan(&engine, &config, scene, "DP-1", false, &BTreeMap::new()).is_err());
    write(&metadata, r#"{"type":"scene","file":"scene.json"}"#);
    symlink(temp.path().join("outside.json"), folder.join("scene.pkg")).expect("package symlink");
    assert!(Wallpaper::load(&folder).is_err());
}

#[test]
fn completed_steam_downloads_are_available_before_installation_alongside_local_projects() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    let apps = root.join("steamapps");
    let config = Config {
        discover_steam: false,
        libraries: vec![root.into()],
        ..Default::default()
    };
    write(
        apps.join("appmanifest_431960.acf"),
        r#""AppState" { "installdir" "Custom Install" }"#,
    );
    for collection in ["defaultprojects", "myprojects"] {
        let folder = apps
            .join("common/Custom Install/projects")
            .join(collection)
            .join("local");
        write(folder.join("project.json"), r#"{"file":"scene.json"}"#);
        write(folder.join("scene.json"), "{}");
    }
    let staged = apps.join("workshop/downloads/431960/100");
    write(
        staged.join("project.json"),
        r#"{"type":"web","file":"index.html"}"#,
    );
    write(staged.join("index.html"), "download fixture");
    for id in ["200", "300"] {
        let incomplete = apps.join("workshop/downloads/431960").join(id);
        write(
            incomplete.join("project.json"),
            r#"{"type":"web","file":"index.html"}"#,
        );
        write(
            incomplete.join("index.html"),
            "preallocated file is not proof of completion",
        );
        assert!(Wallpaper::load(&incomplete).is_err());
    }
    let manifest = apps.join("workshop/appworkshop_431960.acf");
    let details = r#""WorkshopItemDetails" {
        "100" { "BytesDownloaded" "100" "BytesToDownload" "100" }
        "200" { "BytesDownloaded" "50" "BytesToDownload" "100" }
        "300" { "BytesDownloaded" "0" "BytesToDownload" "0" }
    }"#;
    write(
        &manifest,
        &format!(r#""AppWorkshop" {{ "WorkshopItemsInstalled" {{}} {details} }}"#),
    );
    let report = library::scan(&config);
    assert_eq!(
        report.wallpapers.len(),
        3,
        "local projects and the completed download are available"
    );
    assert!(
        report
            .wallpapers
            .iter()
            .filter(|w| !w.staged)
            .all(|w| w.kind == "scene"),
        "legacy bundled metadata may omit the type key"
    );
    assert!(report.wallpapers.iter().any(|w| w.id == "100" && w.staged));
    assert!(
        !report
            .wallpapers
            .iter()
            .any(|w| w.id == "200" || w.id == "300")
    );
    assert_eq!(report.downloads[0].total, 3);
    assert_eq!(
        report.downloads[0].downloaded, 1,
        "zero-byte/partial downloads are not complete"
    );
    assert_eq!(report.downloads[0].installed, 0);
    assert!(report.downloads[0].pending());
    assert!(report.warnings.is_empty());

    let installed = apps.join("workshop/content/431960/100");
    fs::create_dir_all(installed.parent().expect("parent")).expect("content directory");
    fs::rename(&staged, &installed).expect("simulate Steam installing the completed download");
    write(
        &manifest,
        &format!(
            r#""AppWorkshop" {{ "WorkshopItemsInstalled" {{ "100" {{ "manifest" "1" }} }} {details} }}"#
        ),
    );
    let report = library::scan(&config);
    assert_eq!(report.wallpapers.len(), 3);
    assert!(
        report
            .wallpapers
            .iter()
            .any(|w| w.id == "100" && w.folder == installed)
    );
    assert_eq!(
        report.downloads[0].downloaded, 1,
        "installed items are counted once"
    );
    assert_eq!(report.downloads[0].installed, 1);
    assert!(report.wallpapers.iter().all(|w| !w.staged));
    assert_eq!(
        library::resolve(&staged.to_string_lossy(), &config)
            .expect("old path follows Steam's move")
            .folder,
        installed
    );
    assert!(report.downloads[0].pending());
    write(
        &manifest,
        r#""AppWorkshop" { "WorkshopItemsInstalled" { "100" { "manifest" "1" } } "WorkshopItemDetails" { "100" { "BytesDownloaded" "0" "BytesToDownload" "100" } } }"#,
    );
    let report = library::scan(&config);
    assert_eq!(
        report.downloads[0].downloaded, 1,
        "installed files remain complete after Steam clears download counters"
    );
    assert!(!report.downloads[0].pending());
}

#[test]
fn preserved_downloads_survive_steam_removing_the_whole_staging_tree() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let config = Config {
        discover_steam: false,
        libraries: vec![root.into()],
        ..Default::default()
    };
    let staged = staged_wallpaper(root);
    assert_eq!(
        library::preserve_downloads(&config).expect("preserve staging"),
        1
    );
    assert!(
        staged.join("project.json").is_file(),
        "preserving must leave Steam's files intact"
    );
    fs::remove_dir_all(root.join("steamapps/workshop/downloads/431960"))
        .expect("simulate Steam cleanup after an unrelated single-item commit");
    write(
        root.join(".wallweave/wallpapers/431960/.tmp-incomplete/project.json"),
        r#"{"type":"web","file":"missing.html"}"#,
    );
    let report = library::scan(&config);
    assert_eq!(report.wallpapers.len(), 1);
    let saved = &report.wallpapers[0];
    assert!(
        report.warnings.is_empty(),
        "unpublished preservation copies must not enter the scan"
    );
    assert!(saved.folder.starts_with(root.join(".wallweave")));
    assert_eq!(
        fs::read_to_string(saved.folder.join("index.html")).expect("preserved entry"),
        "<!doctype html><title>fixture</title>"
    );
    assert_eq!(
        library::resolve(&staged.to_string_lossy(), &config)
            .expect("old path resolves to preserved copy")
            .folder,
        saved.folder
    );
    assert_eq!(report.downloads[0].downloaded, 1);
    assert!(report.downloads[0].missing.is_empty());
    write(
        root.join("steamapps/workshop/appworkshop_431960.acf"),
        r#""AppWorkshop" { "WorkshopItemDetails" { "123456" { "BytesDownloaded" "0" "BytesToDownload" "100" } } }"#,
    );
    assert_eq!(
        library::scan(&config).downloads[0].downloaded,
        1,
        "a preserved project stays complete when Steam resets its old counters"
    );
    let installed = wallpaper(root);
    let report = library::scan(&config);
    assert_eq!(
        report.wallpapers.len(),
        1,
        "installed and preserved copies must not create duplicate Workshop IDs"
    );
    assert_eq!(report.wallpapers[0].folder, installed);
}

#[test]
fn missing_download_files_are_not_counted_as_available_from_stale_steam_counters() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let config = Config {
        discover_steam: false,
        libraries: vec![root.into()],
        ..Default::default()
    };
    let staged = staged_wallpaper(root);
    fs::remove_dir_all(staged).expect("simulate a stale completion counter");
    let report = library::scan(&config);
    assert!(report.wallpapers.is_empty());
    assert_eq!(report.downloads[0].reported_downloaded, 1);
    assert_eq!(report.downloads[0].downloaded, 0);
    assert_eq!(report.downloads[0].missing, ["123456"]);
}

#[test]
fn preservation_never_promotes_an_incomplete_or_outdated_staging_project() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let config = Config {
        discover_steam: false,
        libraries: vec![root.into()],
        ..Default::default()
    };
    staged_wallpaper(root);
    let manifest = root.join("steamapps/workshop/appworkshop_431960.acf");
    for details in [
        r#""BytesDownloaded" "50" "BytesToDownload" "100""#,
        r#""manifest" "1" "latest_manifest" "2" "BytesDownloaded" "100" "BytesToDownload" "100""#,
    ] {
        write(
            &manifest,
            &format!(r#""AppWorkshop" {{ "WorkshopItemDetails" {{ "123456" {{ {details} }} }} }}"#),
        );
        assert_eq!(library::preserve_downloads(&config).expect("preserve"), 0);
        assert!(!root.join(".wallweave/wallpapers/431960/123456").exists());
    }
}

#[test]
fn staging_launches_recheck_completion_and_prefer_installed_copies() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    let config = config(root);
    let staged = staged_wallpaper(root);
    let downloaded = Wallpaper::load(&staged).expect("completed staging item");
    assert!(downloaded.staged);
    let engine = Renderer::detect(&config).expect("engine");
    let plan = renderer::plan(
        &engine,
        &config,
        downloaded.clone(),
        "DP-1",
        false,
        &BTreeMap::new(),
    )
    .expect("direct staging launch");
    assert!(plan.arguments.iter().any(|arg| arg == staged.as_os_str()));
    let manifest = root.join("steamapps/workshop/appworkshop_431960.acf");
    let complete = fs::read_to_string(&manifest).expect("manifest");
    write(
        &manifest,
        &complete.replace("\"BytesDownloaded\" \"100\"", "\"BytesDownloaded\" \"50\""),
    );
    assert!(
        renderer::plan(
            &engine,
            &config,
            downloaded.clone(),
            "DP-1",
            false,
            &BTreeMap::new()
        )
        .is_err(),
        "a stale GUI snapshot must not launch a now-incomplete download"
    );
    write(
        &manifest,
        &complete.replace("\"latest_manifest\" \"1\"", "\"latest_manifest\" \"2\""),
    );
    assert!(
        Wallpaper::load(&staged).is_err(),
        "completed bytes for an older requested revision are insufficient"
    );
    assert!(library::scan(&config).wallpapers.is_empty());
    write(&manifest, &complete);

    let installed = wallpaper(root);
    let report = library::scan(&config);
    assert_eq!(
        report.wallpapers.len(),
        1,
        "installed and staged copies must not be duplicated"
    );
    assert!(!report.wallpapers[0].staged);
    assert_eq!(
        Wallpaper::load(&staged)
            .expect("installed preferred")
            .folder,
        installed
    );
    fs::remove_dir_all(&staged).expect("Steam clears staging");
    let plan = renderer::plan(
        &engine,
        &config,
        downloaded,
        "DP-1",
        false,
        &BTreeMap::new(),
    )
    .expect("follow moved entry at launch");
    assert_eq!(plan.wallpaper.folder, installed);
    assert!(!plan.wallpaper.staged);
}

#[test]
fn launch_plan_preserves_literal_arguments_and_requires_mounted_assets() {
    let temp = tempfile::tempdir().expect("temp");
    let folder = wallpaper(temp.path());
    let mut config = config(temp.path());
    let engine = Renderer::detect(&config).expect("renderer");
    let literal = "$(touch /tmp/wallweave-should-never-exist); 'literal'";
    let properties = BTreeMap::from([("message".into(), literal.into())]);
    let plan = renderer::plan(
        &engine,
        &config,
        Wallpaper::load(&folder).expect("wallpaper"),
        "DP-1",
        false,
        &properties,
    )
    .expect("plan");
    assert!(
        plan.arguments
            .iter()
            .any(|a| a == std::ffi::OsStr::new(&format!("message={literal}")))
    );
    assert!(plan.arguments.iter().any(|a| a == folder.as_os_str()));
    config.assets = Some(temp.path().join("disconnected"));
    assert!(
        renderer::plan(
            &engine,
            &config,
            Wallpaper::load(&folder).expect("wallpaper"),
            "DP-1",
            false,
            &properties
        )
        .is_err()
    );
}

#[test]
fn desktop_routes_do_not_treat_gnome_as_layer_shell() {
    assert_eq!(
        Desktop::from_environment("Hyprland", "wayland", true).integration,
        "layer-shell"
    );
    assert_eq!(
        Desktop::from_environment("GNOME", "wayland", false).integration,
        "gnome-extension"
    );
    assert_eq!(
        Desktop::from_environment("KDE", "wayland", false).integration,
        "plasma-shell"
    );
}

struct Daemon {
    child: Child,
    paths: Paths,
}
impl Daemon {
    fn start(paths: Paths, root: &Path) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_wallweave"))
            .arg("daemon")
            .env("WALLWEAVE_HOME", root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("daemon");
        let deadline = Instant::now();
        while service::client(&paths, &Request::Status).is_err() {
            assert!(
                deadline.elapsed() < Duration::from_secs(5),
                "daemon startup"
            );
            thread::sleep(Duration::from_millis(30));
        }
        Self { child, paths }
    }
    fn send(&self, request: Request) -> service::Response {
        service::client(&self.paths, &request).expect("IPC")
    }
}
impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = service::client(&self.paths, &Request::Shutdown);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn supervisor_follows_steam_installation_for_staging_playback_and_keeps_pause_and_properties() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    let config = config(root);
    let staged = staged_wallpaper(root);
    let paths = paths(root);
    paths.save(&config).expect("config");
    let daemon = Daemon::start(paths.clone(), root);
    let properties = BTreeMap::from([("message".into(), "kept after installation".into())]);
    for preview in [false, true] {
        let response = daemon.send(Request::Apply {
            target: staged.display().to_string(),
            output: if preview {
                String::new()
            } else {
                "DP-1".into()
            },
            preview,
            properties: properties.clone(),
        });
        assert!(response.ok, "{}", response.message);
    }
    let response = daemon.send(Request::Pause {
        output: Some("DP-1".into()),
    });
    assert!(response.ok);
    let previous: Vec<_> = response.states.iter().map(|s| s.pid).collect();
    let installed = root.join("steamapps/workshop/content/431960/123456");
    fs::create_dir_all(installed.parent().expect("parent")).expect("content");
    fs::rename(&staged, &installed).expect("Steam commits the download");
    write(
        root.join("steamapps/workshop/appworkshop_431960.acf"),
        r#""AppWorkshop" { "WorkshopItemsInstalled" { "123456" { "manifest" "1" } } }"#,
    );
    let start = Instant::now();
    let response = loop {
        let response = daemon.send(Request::Status);
        if response.states.len() == 2
            && response
                .states
                .iter()
                .all(|s| s.wallpaper == installed && !previous.contains(&s.pid))
        {
            break response;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "did not follow Steam's directory move: {response:?}"
        );
        thread::sleep(Duration::from_millis(60));
    };
    assert!(response.failures.is_empty(), "{:?}", response.failures);
    assert!(
        response
            .states
            .iter()
            .find(|s| s.output == "DP-1")
            .expect("monitor")
            .paused
    );
    assert!(
        !response
            .states
            .iter()
            .find(|s| s.output == "preview")
            .expect("preview")
            .paused
    );
    for state in &response.states {
        assert!(service::tail(&state.log, 5000).contains("message=kept after installation"));
    }
    let saved = paths.load().expect("saved config");
    assert_eq!(saved.assignments["DP-1"].wallpaper, installed);
    assert_eq!(saved.assignments["DP-1"].properties, properties);
    assert!(!saved.assignments.contains_key("preview"));
}

#[test]
fn supervisor_isolates_monitors_survives_mount_loss_and_rejects_failed_replacement() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    let folder = wallpaper(root);
    let config = config(root);
    let paths = paths(root);
    paths.save(&config).expect("config");
    let daemon = Daemon::start(paths.clone(), root);
    let mut unrelated = Command::new("sleep").arg("30").spawn().expect("unrelated");
    let apply = |output: &str| Request::Apply {
        target: folder.display().to_string(),
        output: output.into(),
        preview: false,
        properties: BTreeMap::new(),
    };
    let first = daemon.send(apply("DP-1"));
    assert!(first.ok, "{}", first.message);
    let first_pid = first.states[0].pid;
    let second = daemon.send(apply("DP-2"));
    assert!(second.ok, "{}", second.message);
    assert_eq!(second.states.len(), 2);
    let pause = daemon.send(Request::Pause {
        output: Some("DP-1".into()),
    });
    assert!(pause.ok);
    assert!(
        pause
            .states
            .iter()
            .find(|s| s.output == "DP-1")
            .expect("DP-1")
            .paused
    );
    assert!(
        !pause
            .states
            .iter()
            .find(|s| s.output == "DP-2")
            .expect("DP-2")
            .paused
    );
    assert!(
        daemon
            .send(Request::Resume {
                output: Some("DP-1".into())
            })
            .ok
    );
    let mut broken = paths.load().expect("current preferences");
    broken.renderer = Some(fake_renderer(root, true));
    paths.save(&broken).expect("save");
    let failed = daemon.send(apply("DP-1"));
    assert!(!failed.ok);
    assert!(failed.message.contains("intentional-startup-failure"));
    assert_eq!(
        failed
            .states
            .iter()
            .find(|s| s.output == "DP-1")
            .expect("original retained")
            .pid,
        first_pid
    );
    let mut restored = paths.load().expect("latest assignments");
    restored.renderer = config.renderer.clone();
    paths.save(&restored).expect("restore renderer");
    fs::rename(root.join("steamapps"), root.join("disconnected-steamapps")).expect("disconnect");
    let offline = daemon.send(apply("DP-1"));
    assert!(!offline.ok);
    assert_eq!(offline.states.len(), 2);
    assert_eq!(paths.load().expect("config retained").assignments.len(), 2);
    let stopped = daemon.send(Request::Stop {
        output: Some("DP-1".into()),
    });
    assert!(stopped.ok);
    assert_eq!(stopped.states.len(), 1);
    assert_eq!(stopped.states[0].output, "DP-2");
    assert!(
        daemon
            .send(Request::Stop { output: None })
            .states
            .is_empty()
    );
    assert!(
        unrelated.try_wait().expect("status").is_none(),
        "Unrelated processes must not be signalled"
    );
    let _ = unrelated.kill();
    let _ = unrelated.wait();
}

#[test]
fn saved_wallpapers_restore_after_a_session_restart_and_keep_live_renderers() {
    use wallweave::config::Assignment;
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    let paths = paths(root);
    let config = config(root);
    paths.save(&config).expect("config");
    let folder = wallpaper(root);
    {
        let daemon = Daemon::start(paths.clone(), root);
        for (output, message) in [("DP-1", "first display"), ("DP-2", "second display")] {
            assert!(
                daemon
                    .send(Request::Apply {
                        target: folder.display().to_string(),
                        output: output.into(),
                        preview: false,
                        properties: BTreeMap::from([("message".into(), message.into())]),
                    })
                    .ok
            );
        }
    }
    let mut saved = paths.load().expect("saved assignments");
    saved.assignments.insert(
        "disconnected".into(),
        Assignment {
            wallpaper: folder.clone(),
            properties: BTreeMap::new(),
        },
    );
    saved.assignments.insert(
        "preview".into(),
        Assignment {
            wallpaper: folder.clone(),
            properties: BTreeMap::new(),
        },
    );
    paths.save(&saved).expect("config");
    let daemon = Daemon::start(paths.clone(), root);
    assert!(daemon.send(Request::Status).states.is_empty());
    let outputs = vec!["DP-1".into(), "DP-2".into(), "preview".into()];
    let restored = service::restore_saved(&paths, &outputs).expect("restore");
    assert!(restored.ok, "{}", restored.message);
    assert_eq!(restored.states.len(), 2);
    let pids: BTreeMap<_, _> = restored
        .states
        .iter()
        .map(|s| (s.output.clone(), s.pid))
        .collect();
    for (output, message) in [("DP-1", "first display"), ("DP-2", "second display")] {
        let state = restored
            .states
            .iter()
            .find(|s| s.output == output)
            .expect("restored display");
        assert!(
            service::tail(&state.log, 4000).contains(&format!("message={message}")),
            "Saved properties reach the renderer"
        );
    }
    assert!(
        daemon
            .send(Request::Pause {
                output: Some("DP-1".into())
            })
            .ok
    );
    // Two concurrent GUI openings must keep the same renderer processes.
    thread::scope(|scope| {
        let a = scope.spawn(|| service::restore_saved(&paths, &outputs).expect("first open"));
        let b = scope.spawn(|| service::restore_saved(&paths, &outputs).expect("second open"));
        for response in [
            a.join().expect("first response"),
            b.join().expect("second response"),
        ] {
            assert!(response.ok);
            for state in &response.states {
                assert_eq!(state.pid, pids[&state.output]);
            }
            assert!(
                response
                    .states
                    .iter()
                    .find(|s| s.output == "DP-1")
                    .expect("DP-1")
                    .paused
            );
        }
    });
    let atomic = daemon.send(Request::Restore { outputs });
    assert!(atomic.ok);
    for state in atomic.states {
        assert_eq!(state.pid, pids[&state.output]);
    }
    let after = paths.load().expect("assignments retained");
    assert_eq!(
        serde_json::to_value(after.assignments).expect("assignments"),
        serde_json::to_value(saved.assignments).expect("saved")
    );
}

#[test]
fn restore_keeps_unavailable_assignments_and_continues_on_other_displays() {
    use wallweave::config::Assignment;
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    let paths = paths(root);
    let mut config = config(root);
    let folder = wallpaper(root);
    config.assignments.insert(
        "DP-1".into(),
        Assignment {
            wallpaper: root.join("offline/wallpaper"),
            properties: BTreeMap::new(),
        },
    );
    config.assignments.insert(
        "DP-2".into(),
        Assignment {
            wallpaper: folder.clone(),
            properties: BTreeMap::from([("message".into(), "kept".into())]),
        },
    );
    config.assignments.insert(
        "HDMI-A-1".into(),
        Assignment {
            wallpaper: folder,
            properties: BTreeMap::new(),
        },
    );
    paths.save(&config).expect("config");
    let _daemon = Daemon::start(paths.clone(), root);
    let response =
        service::restore_saved(&paths, &["DP-1".into(), "DP-2".into()]).expect("restore report");
    assert!(!response.ok);
    assert!(response.message.contains("DP-1"));
    assert_eq!(response.states.len(), 1);
    assert_eq!(response.states[0].output, "DP-2");
    assert_eq!(
        serde_json::to_value(paths.load().expect("saved").assignments).expect("assignments"),
        serde_json::to_value(config.assignments).expect("original")
    );
}

#[test]
fn restore_works_with_an_older_supervisor_without_replacing_its_wallpaper() {
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::net::UnixListener,
    };
    use wallweave::{
        config::Assignment,
        service::{Response, RunState},
    };
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    let paths = paths(root);
    let mut config = config(root);
    let folder = wallpaper(root);
    for output in ["DP-1", "DP-2"] {
        config.assignments.insert(
            output.into(),
            Assignment {
                wallpaper: folder.clone(),
                properties: BTreeMap::new(),
            },
        );
    }
    paths.save(&config).expect("config");
    fs::create_dir_all(&paths.runtime).expect("runtime");
    let listener = UnixListener::bind(paths.socket()).expect("old supervisor");
    listener.set_nonblocking(true).expect("nonblocking");
    let server = thread::spawn(move || {
        let mut states = vec![RunState {
            output: "DP-1".into(),
            title: "Already playing".into(),
            wallpaper: folder,
            pid: 1234,
            paused: true,
            log: "/fixture/log".into(),
        }];
        let deadline = Instant::now() + Duration::from_secs(4);
        let mut applied = vec![];
        while applied.is_empty() && Instant::now() < deadline {
            if let Ok((mut stream, _)) = listener.accept() {
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .expect("timeout");
                let mut line = String::new();
                BufReader::new(&stream)
                    .read_line(&mut line)
                    .expect("request");
                let request: Request = serde_json::from_str(&line).expect("request json");
                let mut response = Response {
                    ok: true,
                    states: states.clone(),
                    ..Default::default()
                };
                match request {
                    Request::Restore { .. } => {
                        response.ok = false;
                        response.message =
                            "unknown variant `restore`, expected `apply` or `status`".into();
                    }
                    Request::Status => (),
                    Request::Apply { output, .. } => {
                        applied.push(output.clone());
                        states.push(RunState {
                            output,
                            ..states[0].clone()
                        });
                        response.states = states.clone();
                    }
                    _ => panic!("Unexpected startup action"),
                }
                let mut bytes = serde_json::to_vec(&response).expect("response");
                bytes.push(b'\n');
                stream.write_all(&bytes).expect("reply");
            } else {
                thread::sleep(Duration::from_millis(5));
            }
        }
        applied
    });
    let response = service::restore_saved(&paths, &["DP-1".into(), "DP-2".into()])
        .expect("compatible restore");
    assert!(response.ok);
    assert_eq!(response.states.len(), 2);
    assert_eq!(server.join().expect("old supervisor"), vec!["DP-2"]);
    assert!(
        response
            .states
            .iter()
            .any(|s| s.output == "DP-1" && s.pid == 1234 && s.paused)
    );
}

#[test]
fn invalid_config_is_not_silently_rewritten() {
    let temp = tempfile::tempdir().expect("temp");
    let paths = paths(temp.path());
    write(&paths.config, "fps = 0\n");
    assert!(paths.load().is_err());
    assert_eq!(fs::read_to_string(paths.config).expect("read"), "fps = 0\n");
}

#[test]
fn stale_gui_preferences_keep_new_supervisor_assignments() {
    let temp = tempfile::tempdir().expect("temp");
    let paths = paths(temp.path());
    let stale = Config::default();
    let mut current = stale.clone();
    current.assignments.insert(
        "DP-1".into(),
        wallweave::config::Assignment {
            wallpaper: temp.path().join("123"),
            properties: BTreeMap::new(),
        },
    );
    paths.save(&current).expect("supervisor save");
    paths.save_preferences(&stale).expect("GUI save");
    assert_eq!(paths.load().expect("load").assignments.len(), 1);
}
