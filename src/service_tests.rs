use super::*;
use crate::config::Config;

fn fixture() -> (tempfile::TempDir, Supervisor, String) {
    let dir = tempfile::tempdir().expect("fixture");
    let root = dir.path();
    let paths = Paths {
        config: root.join("config.toml"),
        state: root.join("state"),
        runtime: root.join("run"),
    };
    fs::create_dir_all(&paths.runtime).expect("game lifecycle fixture");
    let wallpaper = root.join("wallpaper");
    fs::create_dir_all(&wallpaper).expect("game lifecycle fixture");
    fs::write(wallpaper.join("project.json"), r#"{"title":"Game test","type":"web","file":"index.html","general":{"properties":{"message":{"type":"textinput","value":"hello"}}}}"#).expect("game lifecycle fixture");
    fs::write(wallpaper.join("index.html"), "fixture").expect("game lifecycle fixture");
    let renderer = root.join("renderer");
    fs::write(&renderer, r#"#!/usr/bin/env python3
import pathlib, sys, time
args = sys.argv[1:]
if '--help' in args:
    print('--assets-dir --screen-root --window --fps --set-property --silent --volume --scaling --layer --pause-control-file --no-fullscreen-pause')
    sys.exit(0)
print(repr(args), flush=True)
control = pathlib.Path(args[args.index('--pause-control-file') + 1])
print('INITIAL ' + control.read_text().strip(), flush=True)
while True:
    time.sleep(.03)
"#).expect("game lifecycle fixture");
    fs::set_permissions(&renderer, fs::Permissions::from_mode(0o755))
        .expect("game lifecycle fixture");
    let assets = root.join("assets");
    fs::create_dir(&assets).expect("game lifecycle fixture");
    paths
        .save(&Config {
            discover_steam: false,
            libraries: vec![root.into()],
            assets: Some(assets),
            renderer: Some(renderer),
            ..Default::default()
        })
        .expect("game lifecycle fixture");
    let supervisor = Supervisor {
        paths,
        active: BTreeMap::new(),
        failures: vec![],
        relocation_at: Instant::now(),
        visibility: None,
        visibility_error: None,
        gaming: None,
        gaming_mode: GamingMode::Unload,
        game_active: false,
        game_quiet_since: None,
        gaming_error: None,
        unloaded: BTreeMap::new(),
        restore_at: None,
    };
    (dir, supervisor, wallpaper.to_string_lossy().into_owned())
}

fn game_exited(supervisor: &mut Supervisor) {
    supervisor.game_quiet_since = Some(Instant::now() - Duration::from_secs(4));
    supervisor.update_gaming(Some(false));
}

#[test]
fn game_unloads_real_children_and_restores_properties_and_manual_pause() {
    let (_dir, mut supervisor, wallpaper) = fixture();
    let properties = BTreeMap::from([("message".into(), "preserved value".into())]);
    for output in ["DP-1", "DP-2", "preview"] {
        supervisor
            .apply(&wallpaper, output, output == "preview", &properties)
            .expect("game lifecycle fixture");
    }
    supervisor
        .active
        .get_mut("DP-1")
        .expect("game lifecycle fixture")
        .pause(true)
        .expect("game lifecycle fixture");
    let old_pids: Vec<_> = supervisor
        .active
        .values()
        .filter(|r| r.state.output != "preview")
        .map(|r| r.child.id())
        .collect();
    let preview = supervisor.active["preview"].child.id();
    let assignments = fs::read(&supervisor.paths.config).expect("game lifecycle fixture");
    supervisor.update_gaming(Some(true));
    assert_eq!(supervisor.active.len(), 1);
    assert_eq!(supervisor.active["preview"].child.id(), preview);
    assert!(
        supervisor
            .active
            .get_mut("preview")
            .expect("game lifecycle fixture")
            .child
            .try_wait()
            .expect("game lifecycle fixture")
            .is_none()
    );
    for pid in old_pids {
        assert!(
            !PathBuf::from(format!("/proc/{pid}")).exists(),
            "renderer must be reaped, not suspended"
        );
    }
    assert_eq!(supervisor.unloaded.len(), 2);
    assert!(
        supervisor
            .unloaded
            .values()
            .all(|r| r.state.pid == 0 && r.state.unloaded)
    );
    assert_eq!(
        fs::read(&supervisor.paths.config).expect("game lifecycle fixture"),
        assignments
    );
    supervisor.update_gaming(Some(false));
    assert_eq!(
        supervisor.active.len(),
        1,
        "loading gaps must not restart renderers"
    );
    supervisor.update_gaming(Some(true));
    assert!(supervisor.game_quiet_since.is_none());
    game_exited(&mut supervisor);
    assert!(supervisor.unloaded.is_empty());
    assert_eq!(supervisor.active.len(), 3);
    assert!(supervisor.active["DP-1"].state.paused);
    assert!(!supervisor.active["DP-2"].state.paused);
    for output in ["DP-1", "DP-2"] {
        let run = &supervisor.active[output];
        assert!(!run.state.unloaded);
        assert_eq!(run.properties, properties);
        let log = fs::read_to_string(&run.state.log).expect("game lifecycle fixture");
        assert!(log.contains("preserved value"));
        assert!(log.contains(if output == "DP-1" {
            "INITIAL pause"
        } else {
            "INITIAL play"
        }));
    }
    assert!(supervisor.failures.is_empty(), "{:?}", supervisor.failures);
}

#[test]
fn applying_during_a_game_queues_and_stop_cancels_only_that_output() {
    let (_dir, mut supervisor, wallpaper) = fixture();
    supervisor.update_gaming(Some(true));
    for output in ["DP-1", "DP-2"] {
        supervisor
            .apply(&wallpaper, output, false, &BTreeMap::new())
            .expect("game lifecycle fixture");
    }
    assert!(supervisor.active.is_empty());
    assert!(
        !supervisor.paths.state.join("logs").exists(),
        "queued wallpaper must never launch a renderer"
    );
    supervisor
        .dispatch(Request::Stop {
            output: Some("DP-1".into()),
        })
        .expect("game lifecycle fixture");
    assert_eq!(supervisor.unloaded.len(), 1);
    assert_eq!(
        supervisor
            .paths
            .load()
            .expect("game lifecycle fixture")
            .assignments
            .len(),
        2
    );
    game_exited(&mut supervisor);
    assert_eq!(supervisor.active.keys().collect::<Vec<_>>(), vec!["DP-2"]);
    supervisor.update_gaming(Some(true));
    supervisor
        .dispatch(Request::Stop { output: None })
        .expect("game lifecycle fixture");
    game_exited(&mut supervisor);
    assert!(supervisor.states().is_empty());
}

#[test]
fn unknown_detection_holds_unloaded_and_disabling_restores_immediately() {
    let (_dir, mut supervisor, wallpaper) = fixture();
    supervisor.update_gaming(None);
    supervisor
        .apply(&wallpaper, "DP-1", false, &BTreeMap::new())
        .expect("game lifecycle fixture");
    supervisor.game_quiet_since = Some(Instant::now() - Duration::from_secs(10));
    supervisor.update_gaming(None);
    assert!(supervisor.active.is_empty());
    assert!(supervisor.game_quiet_since.is_none());
    supervisor.gaming_mode = GamingMode::Off;
    supervisor.update_gaming(Some(true));
    assert!(!supervisor.game_active);
    assert!(supervisor.unloaded.is_empty());
    assert_eq!(supervisor.active.len(), 1);
}

#[test]
fn failed_restore_retains_assignment_and_retries_without_duplicates() {
    let (_dir, mut supervisor, wallpaper) = fixture();
    supervisor.update_gaming(Some(true));
    supervisor
        .apply(&wallpaper, "DP-1", false, &BTreeMap::new())
        .expect("game lifecycle fixture");
    let folder = PathBuf::from(&wallpaper);
    let offline = folder.with_extension("offline");
    fs::rename(&folder, &offline).expect("game lifecycle fixture");
    game_exited(&mut supervisor);
    assert!(supervisor.active.is_empty());
    assert_eq!(supervisor.unloaded.len(), 1);
    assert_eq!(supervisor.failures.len(), 1);
    assert_eq!(
        supervisor
            .paths
            .load()
            .expect("game lifecycle fixture")
            .assignments["DP-1"]
            .wallpaper,
        folder
    );
    fs::rename(offline, folder).expect("game lifecycle fixture");
    supervisor.update_gaming(Some(false));
    assert!(
        supervisor.active.is_empty(),
        "retry interval must be respected"
    );
    supervisor.restore_at = None;
    supervisor.update_gaming(Some(false));
    assert_eq!(supervisor.active.len(), 1);
    assert!(supervisor.unloaded.is_empty());
    let pid = supervisor.active["DP-1"].child.id();
    supervisor.update_gaming(Some(false));
    assert_eq!(supervisor.active["DP-1"].child.id(), pid);
}
