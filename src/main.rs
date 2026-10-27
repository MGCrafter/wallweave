use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use wallweave::{
    config::{AutoPause, Paths, absolute},
    desktop, library, renderer,
    service::{self, Request},
    workshop,
};

#[derive(Parser)]
#[command(
    version,
    about = "Native Linux control for your Wallpaper Engine library"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Action>,
}

#[derive(Subcommand)]
enum Action {
    /// Open the native desktop app (also the default command).
    Gui {
        #[arg(long)]
        restore_missing_downloads: bool,
    },
    #[command(hide = true)]
    UiSnapshot {
        path: PathBuf,
        #[arg(long)]
        workshop: Option<String>,
        #[arg(long, conflicts_with = "workshop")]
        wallpaper: Option<String>,
        #[arg(long, conflicts_with_all = ["workshop", "wallpaper"])]
        settings: bool,
        #[arg(long, conflicts_with_all = ["workshop", "wallpaper", "settings"])]
        settings_files: bool,
    },
    /// Browse and download Wallpaper Engine items through the running Steam client.
    #[command(subcommand)]
    Workshop(WorkshopAction),
    #[command(hide = true)]
    WorkshopWorker,
    /// Inspect the desktop, libraries and renderer without starting a wallpaper.
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Discover local Workshop content. Offline libraries remain configured.
    Scan {
        #[arg(long)]
        json: bool,
    },
    /// Inspect a Workshop ID or local wallpaper folder.
    Inspect { target: String },
    /// Show a launch plan without starting a renderer.
    Plan {
        target: String,
        #[arg(long, default_value = "")]
        output: String,
        #[arg(long)]
        preview: bool,
        #[arg(long = "property", value_parser = property)]
        properties: Vec<(String, String)>,
    },
    /// Apply to one native monitor connector, e.g. DP-1.
    Apply {
        target: String,
        #[arg(long)]
        output: String,
        #[arg(long = "property", value_parser = property)]
        properties: Vec<(String, String)>,
    },
    /// Open in a renderer window; does not change the desktop background.
    Preview {
        target: String,
        #[arg(long = "property", value_parser = property)]
        properties: Vec<(String, String)>,
    },
    /// Show only the processes managed by this app.
    Status {
        #[arg(long)]
        json: bool,
    },
    Pause {
        #[arg(long)]
        output: Option<String>,
    },
    Resume {
        #[arg(long)]
        output: Option<String>,
    },
    Stop {
        #[arg(long)]
        output: Option<String>,
    },
    /// Stop the supervisor and its own wallpaper processes.
    Shutdown,
    /// Reapply saved assignments for currently reachable wallpapers.
    Restore,
    #[command(subcommand)]
    Config(ConfigAction),
    #[command(hide = true)]
    Daemon,
}

#[derive(Subcommand)]
enum ConfigAction {
    Show,
    AddLibrary {
        path: PathBuf,
    },
    RemoveLibrary {
        path: PathBuf,
    },
    Assets {
        path: PathBuf,
    },
    Renderer {
        path: PathBuf,
    },
    /// Automatically pause for apps/games: focused (default), fullscreen, or off.
    AutoPause {
        #[arg(value_enum)]
        mode: AutoPause,
    },
}

#[derive(Subcommand)]
enum WorkshopAction {
    /// Verify the current Steam session without changing subscriptions.
    Status,
    /// Search by words, Workshop ID or Steam item link.
    Search {
        #[arg(default_value = "")]
        query: String,
        #[arg(long, default_value = "popular")]
        sort: workshop::Order,
        #[arg(long, default_value_t = 1)]
        page: u32,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        nsfw: bool,
    },
    /// Subscribe and download; Steam chooses its existing library location.
    Download { item: String },
}

fn workshop_cli(paths: &Paths, action: WorkshopAction) -> Result<()> {
    use std::time::Duration;
    use workshop::{Command, Event, Phase, Search, Session};
    let session = Session::spawn(paths);
    loop {
        match session.recv_timeout(Duration::from_secs(35))? {
            Event::Connected => break,
            Event::Disconnected { message } | Event::Error { message, .. } => {
                anyhow::bail!(message)
            }
            _ => {}
        }
    }
    match action {
        WorkshopAction::Status => println!(
            "Connected to the running Steam client. Wallpaper Engine Workshop is available."
        ),
        WorkshopAction::Search {
            query,
            sort,
            page,
            json,
            nsfw,
        } => {
            session.send(Command::Search {
                request: 1,
                search: Search {
                    text: query,
                    order: sort,
                    page,
                    show_nsfw: nsfw,
                    ..Default::default()
                },
            })?;
            loop {
                match session.recv_timeout(Duration::from_secs(35))? {
                    Event::Results { page, .. } => {
                        if json {
                            println!("{}", serde_json::to_string_pretty(&page)?);
                        } else {
                            println!("Page {} · {} results", page.page, page.total);
                            for item in page.items {
                                println!("{}  {}  {}", item.id, item.kind(), item.title);
                            }
                        }
                        break;
                    }
                    Event::Disconnected { message } | Event::Error { message, .. } => {
                        anyhow::bail!(message)
                    }
                    _ => {}
                }
            }
        }
        WorkshopAction::Download { item } => {
            let id = workshop::item_id(&item)
                .context("Use a numeric Workshop ID or a Steam Workshop item link")?;
            session.send(Command::Download { id })?;
            loop {
                match session.recv_timeout(Duration::from_secs(35)) {
                    Ok(Event::Download { download }) => {
                        println!(
                            "{}  {:?}  {} / {} bytes",
                            id, download.phase, download.downloaded, download.total
                        );
                        if download.phase == Phase::Complete {
                            if let Some(folder) = download.folder {
                                println!("{}", folder.display());
                            }
                            break;
                        }
                        if let Some(error) = download.error {
                            anyhow::bail!(error);
                        }
                    }
                    Ok(Event::Disconnected { message } | Event::Error { message, .. }) => {
                        anyhow::bail!(message)
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(error) => return Err(error.into()),
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

fn property(value: &str) -> std::result::Result<(String, String), String> {
    let (name, value) = value.split_once('=').ok_or("Use --property name=value")?;
    if name.is_empty() {
        return Err("Property name cannot be empty".into());
    }
    Ok((name.into(), value.into()))
}

fn print_response(paths: &Paths, request: Request, start: bool) -> Result<()> {
    if start {
        service::ensure_running(paths)?;
    }
    let response = service::client(paths, &request)?;
    println!("{}", response.message);
    for state in &response.states {
        println!(
            "{}  {}  {}  pid={}  log={}",
            state.output,
            state.playback_label(),
            state.title,
            state.pid,
            state.log.display()
        );
    }
    for error in &response.failures {
        eprintln!("Previous renderer exit: {error}");
    }
    ensure!(response.ok, "Request failed");
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let paths = Paths::discover()?;
    match cli.command.unwrap_or(Action::Gui {
        restore_missing_downloads: false,
    }) {
        Action::Gui {
            restore_missing_downloads,
        } => {
            #[cfg(feature = "gui")]
            wallweave::gui::run(paths, restore_missing_downloads)?;
            #[cfg(not(feature = "gui"))]
            {
                let _ = restore_missing_downloads;
                anyhow::bail!(
                    "This build has no GUI. Build with default features, or use --help for CLI commands."
                );
            }
        }
        Action::WorkshopWorker => workshop::worker()?,
        Action::Workshop(action) => workshop_cli(&paths, action)?,
        Action::UiSnapshot {
            path,
            workshop,
            wallpaper,
            settings,
            settings_files,
        } => {
            #[cfg(feature = "gui")]
            wallweave::gui::snapshot(paths, path, workshop, wallpaper, settings, settings_files)?;
            #[cfg(not(feature = "gui"))]
            {
                let _ = (path, workshop, wallpaper, settings, settings_files);
                anyhow::bail!("UI snapshots require the GUI feature");
            }
        }
        Action::Daemon => service::run(paths)?,
        Action::Doctor { json } => {
            let config = paths.load()?;
            let scan = library::scan(&config);
            let engine = renderer::Renderer::detect(&config);
            let report = serde_json::json!({ "desktop": desktop::Desktop::detect(), "wayland_socket": desktop::wayland_socket_available(), "monitors": desktop::monitors(), "renderer": engine.as_ref().ok(), "renderer_error": engine.err().map(|e| format!("{e:#}")), "assets": scan.assets, "libraries": scan.roots, "wallpapers": scan.wallpapers.len(), "downloads": scan.downloads, "warnings": scan.warnings, "configuration": paths.config });
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "Desktop: {} ({})",
                    report["desktop"]["name"].as_str().unwrap_or("unknown"),
                    report["desktop"]["integration"]
                        .as_str()
                        .unwrap_or("unknown")
                );
                println!(
                    "Renderer: {}",
                    report["renderer"]["executable"]
                        .as_str()
                        .unwrap_or("not found")
                );
                println!(
                    "Assets: {}",
                    report["assets"]
                        .as_str()
                        .unwrap_or("not available; reconnect the drive and add its Steam library")
                );
                println!("Wallpapers: {}", report["wallpapers"]);
                println!("{}", report["desktop"]["note"].as_str().unwrap_or_default());
                if let Some(error) = report["renderer_error"].as_str() {
                    println!("Renderer: {error}");
                }
                for root in scan.roots {
                    println!("{} — {}", root.path.display(), root.message);
                }
                for warning in scan.warnings {
                    println!("Note: {warning}");
                }
            }
        }
        Action::Scan { json } => {
            let scan = library::scan(&paths.load()?);
            if json {
                println!("{}", serde_json::to_string_pretty(&scan)?);
            } else {
                for wallpaper in scan.wallpapers {
                    println!("{}\t{}\t{}", wallpaper.id, wallpaper.kind, wallpaper.title);
                }
                for root in scan.roots {
                    eprintln!("{}: {}", root.path.display(), root.message);
                }
                for warning in scan.warnings {
                    eprintln!("Note: {warning}");
                }
            }
        }
        Action::Inspect { target } => {
            let wallpaper = library::resolve(&target, &paths.load()?)?;
            println!("{}", serde_json::to_string_pretty(&wallpaper)?);
            println!("{}", wallpaper.script_description());
        }
        Action::Plan {
            target,
            output,
            preview,
            properties,
        } => {
            let config = paths.load()?;
            let engine = renderer::Renderer::detect(&config)?;
            let mut plan = renderer::plan(
                &engine,
                &config,
                library::resolve(&target, &config)?,
                &output,
                preview,
                &properties.into_iter().collect(),
            )?;
            plan.auto_pause(
                &engine,
                AutoPause::load(&paths)?,
                wallweave::visibility::socket_path().is_some(),
            );
            println!("{}", plan.display());
        }
        Action::Apply {
            target,
            output,
            properties,
        } => print_response(
            &paths,
            Request::Apply {
                target,
                output,
                preview: false,
                properties: properties.into_iter().collect(),
            },
            true,
        )?,
        Action::Preview { target, properties } => print_response(
            &paths,
            Request::Apply {
                target,
                output: String::new(),
                preview: true,
                properties: properties.into_iter().collect(),
            },
            true,
        )?,
        Action::Status { json } => {
            let response =
                service::client(&paths, &Request::Status).unwrap_or_else(|_| service::Response {
                    ok: true,
                    message: "Supervisor not running".into(),
                    ..Default::default()
                });
            if json {
                println!("{}", serde_json::to_string_pretty(&response)?);
            } else {
                println!("{}", response.message);
                for state in response.states {
                    println!(
                        "{}  {}  {}",
                        state.output,
                        state.playback_label(),
                        state.title
                    );
                }
                for failure in response.failures {
                    println!("Renderer exit: {failure}");
                }
            }
        }
        Action::Pause { output } => print_response(&paths, Request::Pause { output }, false)?,
        Action::Resume { output } => print_response(&paths, Request::Resume { output }, false)?,
        Action::Stop { output } => print_response(&paths, Request::Stop { output }, false)?,
        Action::Shutdown => print_response(&paths, Request::Shutdown, false)?,
        Action::Restore => {
            let config = paths.load()?;
            let mut errors = 0;
            for (output, assignment) in config.assignments {
                let request = Request::Apply {
                    target: assignment.wallpaper.to_string_lossy().into_owned(),
                    output,
                    preview: false,
                    properties: assignment.properties,
                };
                if let Err(error) = print_response(&paths, request, true) {
                    errors += 1;
                    eprintln!("{error:#}");
                }
            }
            ensure!(
                errors == 0,
                "{errors} saved wallpaper(s) could not be restored; assignments were retained"
            );
        }
        Action::Config(action) => {
            let mut config = paths.load()?;
            match action {
                ConfigAction::Show => {
                    println!("{}", toml::to_string_pretty(&config)?);
                    return Ok(());
                }
                ConfigAction::AddLibrary { path } => {
                    let path = absolute(&path)?;
                    if !config.libraries.contains(&path) {
                        config.libraries.push(path);
                    }
                }
                ConfigAction::RemoveLibrary { path } => {
                    let path = absolute(&path)?;
                    config.libraries.retain(|v| v != &path);
                }
                ConfigAction::Assets { path } => config.assets = Some(absolute(&path)?),
                ConfigAction::Renderer { path } => config.renderer = Some(absolute(&path)?),
                ConfigAction::AutoPause { mode } => {
                    mode.save(&paths)?;
                    println!("Automatic pause: {}", mode.label());
                    if service::client(&paths, &Request::Status).is_ok() {
                        print_response(&paths, Request::ReloadPlayback, false)?;
                    }
                    return Ok(());
                }
            }
            paths.save(&config).context("Save configuration")?;
            println!("Saved {}", paths.config.display());
        }
    }
    Ok(())
}
