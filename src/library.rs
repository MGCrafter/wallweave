use crate::{config::Config, vdf};
use anyhow::{Context, Result, ensure};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

const MAX_METADATA: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Wallpaper {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub folder: PathBuf,
    pub file: String,
    /// The logical scene entry lives inside scene.pkg rather than beside it.
    #[serde(default)]
    pub packed: bool,
    /// Steam reports a completed download which has not been installed yet.
    #[serde(default)]
    pub staged: bool,
    pub preview: Option<PathBuf>,
    pub properties: BTreeMap<String, Value>,
    /// Labels supplied by the wallpaper author, keyed by language and token.
    #[serde(default)]
    pub localization: Value,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Scan {
    pub wallpapers: Vec<Wallpaper>,
    pub roots: Vec<RootStatus>,
    pub assets: Option<PathBuf>,
    pub warnings: Vec<String>,
    pub downloads: Vec<WorkshopDownloads>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkshopDownloads {
    pub library: PathBuf,
    pub total: usize,
    /// Includes installed items and fully downloaded items awaiting installation.
    pub downloaded: usize,
    pub installed: usize,
    /// Historical Steam counters can outlive the files in its staging folder.
    #[serde(default)]
    pub reported_downloaded: usize,
    #[serde(default)]
    pub missing: Vec<String>,
}

impl WorkshopDownloads {
    pub fn pending(&self) -> bool {
        self.installed < self.total
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RootStatus {
    pub path: PathBuf,
    pub available: bool,
    pub message: String,
}

pub fn read_small(path: &Path) -> Result<String> {
    let file = fs::File::open(path).with_context(|| format!("Open {}", path.display()))?;
    ensure!(
        file.metadata()?.len() <= MAX_METADATA,
        "Metadata exceeds 4 MiB: {}",
        path.display()
    );
    let mut result = String::new();
    file.take(MAX_METADATA + 1).read_to_string(&mut result)?;
    ensure!(
        result.len() as u64 <= MAX_METADATA,
        "Metadata exceeds 4 MiB"
    );
    Ok(result)
}

/// All entry points stay within the selected wallpaper, including symlinks.
pub fn contained_file(folder: &Path, relative: &str) -> Result<PathBuf> {
    let relative = Path::new(relative);
    ensure!(
        !relative.is_absolute()
            && !relative
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_))),
        "Wallpaper path escapes its folder"
    );
    let folder = folder.canonicalize()?;
    let target = folder
        .join(relative)
        .canonicalize()
        .with_context(|| format!("Missing content: {}", relative.display()))?;
    ensure!(
        target.starts_with(&folder),
        "Wallpaper symlink points outside its folder"
    );
    ensure!(target.is_file(), "Wallpaper entry is not a regular file");
    Ok(target)
}

impl Wallpaper {
    pub fn workshop_id(&self) -> Option<u64> {
        workshop_origin(&self.folder)?;
        self.id.parse().ok().filter(|id| *id > 0)
    }

    /// Steam copies share a favorite through staging, installation and backup.
    /// Local projects use their full path, since folder names can repeat.
    pub fn favorite_key(&self) -> String {
        match self.workshop_id() {
            Some(id) => format!("workshop:{id}"),
            None => format!("local:{}", self.folder.display()),
        }
    }

    /// Recognize the same Steam item after staging, installation or preservation,
    /// without reading a path which may have disappeared during Steam's move.
    pub fn matches_folder(&self, folder: &Path) -> bool {
        self.folder == folder
            || self.workshop_id().is_some_and(|id| {
                workshop_origin(folder).is_some()
                    && folder
                        .file_name()
                        .and_then(|v| v.to_str())
                        .and_then(|v| v.parse::<u64>().ok())
                        == Some(id)
            })
    }

    pub fn load(folder: &Path) -> Result<Self> {
        let installed = installed_folder(folder).filter(|p| p.join("project.json").is_file());
        let preserved = staging_location(folder)
            .map(|(apps, id)| preserved_root(&apps).join(id))
            .filter(|p| !folder.exists() && p.join("project.json").is_file());
        let folder = installed
            .as_deref()
            .or(preserved.as_deref())
            .unwrap_or(folder);
        let folder = folder
            .canonicalize()
            .with_context(|| format!("Wallpaper unavailable: {}", folder.display()))?;
        let staged = if let Some((apps, id)) = staging_location(&folder) {
            let state = workshop_downloads(&apps, &apps)?
                .context("Steam has not recorded this download as complete")?;
            ensure!(
                state.complete.contains(&id),
                "Steam is still downloading this wallpaper; try again when its download is complete"
            );
            true
        } else {
            false
        };
        Self::load_metadata(&folder, staged)
    }

    fn load_metadata(folder: &Path, staged: bool) -> Result<Self> {
        let folder = folder
            .canonicalize()
            .with_context(|| format!("Wallpaper unavailable: {}", folder.display()))?;
        let metadata = read_small(&folder.join("project.json"))?;
        let value: Value = serde_json::from_str(metadata.trim_start_matches('\u{feff}'))
            .context("Invalid project.json")?;
        let id = folder
            .file_name()
            .map(|v| v.to_string_lossy().into_owned())
            .unwrap_or_else(|| "wallpaper".into());
        let file = value
            .get("file")
            .and_then(Value::as_str)
            .with_context(|| {
                if let Some(base) = value.get("dependency").and_then(Value::as_str) {
                    format!("Preset project requires base Workshop wallpaper {base}; preset playback is not supported yet")
                } else {
                    "project.json has no file entry".into()
                }
            })?
            .to_string();
        let extension = Path::new(&file)
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let kind = match extension.as_str() {
            "json" | "pkg" | "gif" => "scene".into(),
            "html" => "web".into(),
            "mp4" | "wmv" | "avi" | "m4v" | "mov" | "webm" | "mkv" => "video".into(),
            "exe" => "application".into(),
            _ => value
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_ascii_lowercase(),
        };
        let preview = value
            .get("preview")
            .and_then(Value::as_str)
            .and_then(|v| contained_file(&folder, v).ok());
        let properties = value
            .pointer("/general/properties")
            .and_then(Value::as_object)
            .map(|v| v.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();
        let mut wallpaper = Self {
            title: value
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or(&id)
                .to_string(),
            kind,
            id,
            folder,
            file,
            packed: false,
            staged,
            preview,
            properties,
            localization: value
                .pointer("/general/localization")
                .cloned()
                .unwrap_or_default(),
        };
        wallpaper.packed = wallpaper
            .content_path()?
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("pkg"));
        Ok(wallpaper)
    }

    /// Keep project.json's logical filename for the renderer, but validate the
    /// physical package when Steam ships scene.json only inside scene.pkg.
    pub fn content_path(&self) -> Result<PathBuf> {
        let entry = contained_file(&self.folder, &self.file);
        if entry.is_ok() {
            return entry;
        }
        let logical = Path::new(&self.file);
        if self.kind == "scene"
            && logical.components().count() == 1
            && matches!(logical.components().next(), Some(Component::Normal(_)))
            && logical
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
            && self
                .folder
                .join(logical)
                .symlink_metadata()
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        {
            return contained_file(&self.folder, "scene.pkg");
        }
        entry
    }

    pub fn script_description(&self) -> &'static str {
        match self.kind.as_str() {
            "web" => "HTML / JavaScript; requires browser and Wallpaper Engine web APIs",
            "scene" if self.packed => {
                "Packed scene; SceneScript usage requires renderer inspection"
            }
            "scene" => "Scene; requires native renderer and SceneScript APIs when used",
            "video" => "Video; no SceneScript runtime needed",
            "application" => "Windows application wallpaper; not supported by Wallweave",
            _ => "Unknown wallpaper type; inspect before running",
        }
    }
}

/// Locate a Workshop item without touching the filesystem, including after
/// Steam has moved the old directory out of downloads/431960.
fn staging_location(folder: &Path) -> Option<(PathBuf, String)> {
    let id = folder.file_name()?.to_str()?;
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let app = folder.parent()?;
    let downloads = app.parent()?;
    let workshop = downloads.parent()?;
    if app.file_name()? != "431960"
        || downloads.file_name()? != "downloads"
        || workshop.file_name()? != "workshop"
    {
        return None;
    }
    Some((workshop.parent()?.to_path_buf(), id.into()))
}

pub fn installed_folder(folder: &Path) -> Option<PathBuf> {
    let (apps, id) = staging_location(folder)?;
    Some(apps.join("workshop/content/431960").join(id))
}

fn steamapps(root: &Path) -> PathBuf {
    if root.file_name().is_some_and(|p| p == "steamapps") {
        root.to_path_buf()
    } else {
        root.join("steamapps")
    }
}

fn preserved_root(apps: &Path) -> PathBuf {
    apps.parent()
        .unwrap_or(apps)
        .join(".wallweave/wallpapers/431960")
}

fn is_preserved(folder: &Path) -> bool {
    folder
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .is_some_and(|p| p.file_name().is_some_and(|name| name == ".wallweave"))
}

fn workshop_origin(folder: &Path) -> Option<PathBuf> {
    if is_preserved(folder) {
        return folder.ancestors().nth(4).map(Path::to_path_buf);
    }
    let app = folder.parent()?;
    if app.file_name()? != "431960" {
        return None;
    }
    let section = app.parent()?;
    if !["content", "downloads"]
        .iter()
        .any(|name| section.file_name().is_some_and(|v| v == *name))
    {
        return None;
    }
    let workshop = section.parent()?;
    (workshop.file_name()? == "workshop")
        .then(|| workshop.parent()?.parent().map(Path::to_path_buf))
        .flatten()
}

/// Preserve verified staging projects outside Steam's disposable download tree
/// before entering the SDK. On Linux, fs::copy uses copy_file_range, which can
/// share extents on the library's Btrfs filesystem. Publish each copy atomically.
pub fn preserve_downloads(config: &Config) -> Result<usize> {
    let report = scan(config);
    let mut count = 0;
    let mut projects = Vec::new();
    for summary in &report.downloads {
        let apps = steamapps(&summary.library);
        if let Some(state) = workshop_downloads(&apps, &summary.library)? {
            for id in state.complete {
                let source = apps.join("workshop/downloads/431960").join(&id);
                if source.join("project.json").is_file() {
                    projects.push((source.canonicalize()?, preserved_root(&apps).join(id)));
                }
            }
        }
    }
    projects.sort();
    projects.dedup();
    for (source, destination) in projects {
        let id = destination
            .file_name()
            .context("Missing Workshop ID")?
            .to_string_lossy();
        let root = destination
            .parent()
            .context("Missing preservation directory")?;
        fs::create_dir_all(root)?;
        // Refresh the fallback on every preservation pass, including updates.
        let temporary = tempfile::tempdir_in(root)?;
        copy_tree(&source, temporary.path(), &source)?;
        let previous = root.join(format!(".previous-{id}"));
        if previous.exists() {
            fs::remove_dir_all(&previous)?;
        }
        if destination.exists() {
            fs::rename(&destination, &previous)?;
        }
        let publish = fs::rename(temporary.path(), &destination);
        if let Err(error) = publish {
            if previous.exists() {
                let _ = fs::rename(&previous, &destination);
            }
            return Err(error.into());
        }
        if previous.exists() {
            fs::remove_dir_all(previous)?;
        }
        count += 1;
    }
    Ok(count)
}

fn copy_tree(source: &Path, destination: &Path, project: &Path) -> Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let path = entry.path();
        let target = destination.join(entry.file_name());
        let canonical = path.canonicalize()?;
        ensure!(
            canonical.starts_with(project),
            "Wallpaper link escapes its folder"
        );
        if entry.file_type()?.is_dir() {
            fs::create_dir(&target)?;
            copy_tree(&path, &target, project)?;
        } else {
            ensure!(
                canonical.is_file(),
                "Wallpaper contains an unsupported link or file"
            );
            fs::copy(&path, &target).with_context(|| format!("Preserve {}", path.display()))?;
        }
    }
    Ok(())
}

fn standard_roots() -> Vec<PathBuf> {
    BaseDirs::new()
        .map(|base| {
            [
                ".local/share/Steam",
                ".steam/steam",
                ".var/app/com.valvesoftware.Steam/.local/share/Steam",
                "snap/steam/common/.local/share/Steam",
            ]
            .into_iter()
            .map(|p| base.home_dir().join(p))
            .filter(|p| p.exists())
            .collect()
        })
        .unwrap_or_default()
}

struct WorkshopState {
    summary: WorkshopDownloads,
    complete: BTreeSet<String>,
}

fn workshop_downloads(apps: &Path, library: &Path) -> Result<Option<WorkshopState>> {
    let manifest = apps.join("workshop/appworkshop_431960.acf");
    if !manifest.is_file() {
        return Ok(None);
    }
    let value = vdf::parse(&read_small(&manifest)?)?;
    let workshop = value
        .get("AppWorkshop")
        .context("Missing AppWorkshop section")?;
    let ids = |section: &str| -> BTreeSet<String> {
        workshop
            .get(section)
            .map(vdf::Value::pairs)
            .unwrap_or_default()
            .iter()
            .filter(|(id, _)| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
            .map(|(id, _)| id.clone())
            .collect()
    };
    let installed = ids("WorkshopItemsInstalled");
    let mut total = ids("WorkshopItemDetails");
    total.extend(installed.iter().cloned());
    let mut downloaded = installed.clone();
    let mut complete = BTreeSet::new();
    if let Some(details) = workshop.get("WorkshopItemDetails") {
        for (id, item) in details.pairs() {
            let bytes = |key| {
                item.get(key)
                    .and_then(vdf::Value::text)
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0)
            };
            let size = bytes("BytesToDownload");
            if total.contains(id) && size > 0 && bytes("BytesDownloaded") >= size {
                downloaded.insert(id.clone());
                // A newer requested manifest must not reuse an old download's
                // completed byte counter.
                let revision = |key| item.get(key).and_then(vdf::Value::text);
                if !matches!(
                    (revision("manifest"), revision("latest_manifest")),
                    (Some(current), Some(latest)) if current != latest
                ) {
                    complete.insert(id.clone());
                }
            }
        }
    }
    let reported_downloaded = downloaded.len();
    let preserved = |id: &str| preserved_root(apps).join(id).join("project.json").is_file();
    // A published preserved copy remains complete even after Steam resets its
    // old per-item counters when committing a different download.
    downloaded.extend(total.iter().filter(|id| preserved(id)).cloned());
    let available = |id: &str| {
        [
            apps.join("workshop/content/431960").join(id),
            apps.join("workshop/downloads/431960").join(id),
            preserved_root(apps).join(id),
        ]
        .iter()
        .any(|folder| folder.join("project.json").is_file())
    };
    let missing: Vec<_> = downloaded
        .iter()
        .filter(|id| (installed.contains(*id) || complete.contains(*id)) && !available(id))
        .cloned()
        .collect();
    downloaded.retain(|id| {
        (installed.contains(id) || complete.contains(id) || preserved(id)) && available(id)
    });
    Ok(Some(WorkshopState {
        summary: WorkshopDownloads {
            library: library.to_path_buf(),
            total: total.len(),
            downloaded: downloaded.len(),
            installed: installed.len(),
            reported_downloaded,
            missing,
        },
        complete,
    }))
}

fn project_entries(collection: &Path, entries: &mut Vec<PathBuf>, warnings: &mut Vec<String>) {
    if collection.join("project.json").is_file() {
        entries.push(collection.to_path_buf());
        return;
    }
    match fs::read_dir(collection) {
        Ok(children) => {
            for child in children.take(100_000) {
                match child {
                    Ok(child)
                        if child.path().join("project.json").is_file()
                            && !(is_preserved(&child.path())
                                && child.file_name().to_string_lossy().starts_with('.')) =>
                    {
                        entries.push(child.path());
                    }
                    Ok(_) => (),
                    Err(e) => warnings.push(format!("{}: {e}", collection.display())),
                }
            }
        }
        Err(e) => warnings.push(format!("{}: {e}", collection.display())),
    }
}

pub fn scan(config: &Config) -> Scan {
    let mut result = Scan::default();
    let mut roots = config.libraries.clone();
    if config.discover_steam {
        roots.extend(standard_roots());
    }
    let mut seen = BTreeSet::new();
    let mut index = 0;
    while index < roots.len() && index < 128 {
        let path = roots[index].clone();
        index += 1;
        let identity = path.canonicalize().unwrap_or_else(|_| path.clone());
        if !seen.insert(identity) {
            continue;
        }
        if !path.is_dir() {
            result.roots.push(RootStatus {
                path,
                available: false,
                message: "Unavailable; reconnect or mount the library, then rescan".into(),
            });
            continue;
        }
        let apps = steamapps(&path);
        let manifest = apps.join("libraryfolders.vdf");
        if manifest.is_file() {
            match read_small(&manifest).and_then(|text| vdf::library_paths(&text)) {
                Ok(paths) => roots.extend(
                    paths
                        .into_iter()
                        .map(PathBuf::from)
                        .filter(|p| p.is_absolute()),
                ),
                Err(e) => result.warnings.push(format!("{}: {e}", manifest.display())),
            }
        }
        let completed = match workshop_downloads(&apps, &path) {
            Ok(Some(state)) => {
                result.downloads.push(state.summary);
                state.complete
            }
            Ok(None) => BTreeSet::new(),
            Err(e) => {
                result.warnings.push(format!(
                    "{}: {e:#}",
                    apps.join("workshop/appworkshop_431960.acf").display()
                ));
                BTreeSet::new()
            }
        };
        let mut installations = vec![
            apps.join("common/wallpaper_engine"),
            apps.join("common/Wallpaper Engine"),
        ];
        if let Ok(text) = read_small(&apps.join("appmanifest_431960.acf"))
            && let Ok(value) = vdf::parse(&text)
            && let Some(dir) = value
                .get("AppState")
                .and_then(|s| s.get("installdir"))
                .and_then(vdf::Value::text)
            && !Path::new(dir)
                .components()
                .any(|c| matches!(c, Component::ParentDir))
            && !Path::new(dir).is_absolute()
        {
            installations.insert(0, apps.join("common").join(dir));
        }
        if config.assets.is_none() && result.assets.is_none() {
            let mut candidates: Vec<_> = installations.iter().map(|p| p.join("assets")).collect();
            candidates.push(path.join("assets"));
            result.assets = candidates
                .into_iter()
                .find(|p| p.is_dir())
                .and_then(|p| p.canonicalize().ok());
        }
        let mut collections = vec![
            apps.join("workshop/content/431960"),
            path.join("workshop/content/431960"),
            preserved_root(&apps),
        ];
        for installation in &installations {
            for name in ["defaultprojects", "myprojects"] {
                collections.push(installation.join("projects").join(name));
            }
        }
        if path.join("project.json").is_file()
            || path
                .file_name()
                .is_some_and(|v| v == "431960" || v == "defaultprojects" || v == "myprojects")
        {
            collections.push(path.clone());
        }
        collections.retain(|p| p.is_dir());
        collections.sort();
        collections.dedup();
        let mut entries = Vec::new();
        for collection in collections {
            project_entries(&collection, &mut entries, &mut result.warnings);
        }
        entries.sort();
        entries.dedup();
        let before = result.wallpapers.len();
        for entry in entries {
            match Wallpaper::load(&entry) {
                Ok(wallpaper) => result.wallpapers.push(wallpaper),
                Err(e) => result.warnings.push(format!("{}: {e:#}", entry.display())),
            }
        }
        // Read Steam's manifest once per library. Directory entries or
        // preallocated files alone do not make a download eligible for playback.
        for id in completed {
            let staged = apps.join("workshop/downloads/431960").join(&id);
            let installed = apps.join("workshop/content/431960").join(&id);
            if installed.join("project.json").is_file() || !staged.is_dir() {
                continue;
            }
            match Wallpaper::load_metadata(&staged, true) {
                Ok(wallpaper) => result.wallpapers.push(wallpaper),
                Err(e) => result.warnings.push(format!("{}: {e:#}", staged.display())),
            }
        }
        let mut message = format!("{} wallpaper(s) found", result.wallpapers.len() - before);
        if let Some(downloads) = result
            .downloads
            .last()
            .filter(|d| d.library == path && d.pending())
        {
            message.push_str(&format!(
                "; Steam: {}/{} downloaded, {} installed; completed downloads are available directly",
                downloads.downloaded, downloads.total, downloads.installed
            ));
        }
        result.roots.push(RootStatus {
            path,
            available: true,
            message,
        });
    }
    if let Some(assets) = &config.assets {
        if assets.is_dir() {
            result.assets = Some(assets.clone());
        } else {
            result.warnings.push(format!(
                "Assets unavailable: {}. Reconnect the drive before playback.",
                assets.display()
            ));
        }
    }
    result.wallpapers.sort_by(|a, b| a.folder.cmp(&b.folder));
    result.wallpapers.dedup_by(|a, b| a.folder == b.folder);
    // A preserved copy is a fallback for the same library's Steam item.
    let originals: BTreeSet<_> = result
        .wallpapers
        .iter()
        .filter(|w| !is_preserved(&w.folder))
        .filter_map(|w| workshop_origin(&w.folder).map(|origin| (origin, w.id.clone())))
        .collect();
    result.wallpapers.retain(|w| {
        !is_preserved(&w.folder)
            || workshop_origin(&w.folder)
                .is_none_or(|origin| !originals.contains(&(origin, w.id.clone())))
    });
    result.wallpapers.sort_by_key(|w| w.title.to_lowercase());
    result
}

pub fn resolve(target: &str, config: &Config) -> Result<Wallpaper> {
    let path = Path::new(target);
    if path.is_dir() || installed_folder(path).is_some() {
        return Wallpaper::load(path);
    }
    let report = scan(config);
    let mut matches = report.wallpapers.into_iter().filter(|w| w.id == target);
    let wallpaper = matches.next().with_context(|| {
        format!("Wallpaper {target} not found. Mount your library and run wallweave scan.")
    })?;
    ensure!(
        matches.next().is_none(),
        "Workshop ID is present in multiple libraries; select its full folder path"
    );
    Ok(wallpaper)
}
