use anyhow::{Context, Result, ensure};
use directories::{BaseDirs, ProjectDirs};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct Paths {
    pub config: PathBuf,
    pub state: PathBuf,
    pub runtime: PathBuf,
}

impl Paths {
    pub fn discover() -> Result<Self> {
        if let Some(root) = env::var_os("WALLWEAVE_HOME") {
            let root = PathBuf::from(root);
            ensure!(
                root.is_absolute(),
                "WALLWEAVE_HOME must be an absolute path"
            );
            return Ok(Self {
                config: root.join("config.toml"),
                state: root.join("state"),
                runtime: root.join("run"),
            });
        }
        let dirs = ProjectDirs::from("", "", "wallweave")
            .context("Cannot determine application directories")?;
        let base = BaseDirs::new().context("Cannot determine home directory")?;
        let runtime = env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                env::temp_dir().join(format!("wallweave-{}", nix::unistd::geteuid()))
            });
        Ok(Self {
            config: dirs.config_dir().join("config.toml"),
            state: base
                .state_dir()
                .unwrap_or(dirs.data_local_dir())
                .join("wallweave"),
            runtime: runtime.join("wallweave"),
        })
    }

    pub fn socket(&self) -> PathBuf {
        self.runtime.join("control.sock")
    }

    pub fn load(&self) -> Result<Config> {
        if !self.config.exists() {
            return Ok(Config::default());
        }
        let text = fs::read_to_string(&self.config)
            .with_context(|| format!("Read {}", self.config.display()))?;
        let value: Config = toml::from_str(&text)
            .context("Invalid Wallweave configuration; the file was not changed")?;
        value.validate()?;
        Ok(value)
    }

    pub fn save(&self, config: &Config) -> Result<()> {
        config.validate()?;
        let parent = self
            .config
            .parent()
            .context("Configuration has no parent")?;
        fs::create_dir_all(parent)?;
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        use std::io::Write;
        tmp.write_all(toml::to_string_pretty(config)?.as_bytes())?;
        tmp.as_file().sync_all()?;
        tmp.persist(&self.config).map_err(|e| e.error)?;
        Ok(())
    }

    /// GUI preferences must not erase assignments saved by a running supervisor.
    pub fn save_preferences(&self, config: &Config) -> Result<()> {
        let mut next = config.clone();
        next.assignments = self.load()?.assignments;
        self.save(&next)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub discover_steam: bool,
    /// Steam roots, steamapps folders, or workshop/content/431960 folders.
    pub libraries: Vec<PathBuf>,
    /// Wallpaper Engine's assets directory; may be temporarily offline.
    pub assets: Option<PathBuf>,
    /// A renderer executable or its portable launcher. Never a shell command.
    pub renderer: Option<PathBuf>,
    pub fps: u16,
    pub volume: u8,
    pub scaling: String,
    /// Preferences only. Applying does not enable autostart.
    pub assignments: BTreeMap<String, Assignment>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            discover_steam: true,
            libraries: vec![],
            assets: None,
            renderer: None,
            fps: 30,
            volume: 0,
            scaling: "fill".into(),
            assignments: BTreeMap::new(),
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=240).contains(&self.fps),
            "FPS must be between 1 and 240"
        );
        ensure!(self.volume <= 100, "Volume must be between 0 and 100");
        ensure!(
            ["fill", "fit", "stretch", "default"].contains(&self.scaling.as_str()),
            "Unknown scaling mode"
        );
        for path in self
            .libraries
            .iter()
            .chain(self.assets.iter())
            .chain(self.renderer.iter())
        {
            ensure!(
                path.is_absolute(),
                "Use an absolute path: {}",
                path.display()
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Assignment {
    pub wallpaper: PathBuf,
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
}

pub fn absolute(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(env::current_dir()?.join(path))
    }
}
