use log::warn;
use serde::{Deserialize, Deserializer, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowShape {
    Circle,
    Square,
    RoundedRect,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TwoColumnEntry {
    pub path: PathBuf,
    #[serde(default)]
    pub args: Option<String>,
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
}

impl TwoColumnEntry {
    pub fn from_launch(path: PathBuf, args: Option<String>, working_dir: Option<PathBuf>) -> Self {
        Self {
            path,
            args,
            working_dir,
        }
    }

    pub fn key(&self) -> String {
        normalize_launch_key(
            &self.path,
            self.args.as_deref(),
            self.working_dir.as_deref(),
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TwoColumnLayout {
    #[serde(default)]
    pub left: Vec<TwoColumnEntry>,
    #[serde(default)]
    pub right: Vec<TwoColumnEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default, deserialize_with = "deserialize_pinned_apps")]
    pub pinned_apps: Vec<PathBuf>,
    #[serde(default)]
    pub pinned_launch_meta: Vec<PinnedLaunchMeta>,
    pub shape: WindowShape,
    pub last_pos: Option<(f32, f32)>,
    #[serde(default)]
    pub last_size: Option<(f32, f32)>,
    #[serde(default)]
    pub quick_launch_app: Option<PathBuf>,
    #[serde(default)]
    pub two_column_mode: bool,
    #[serde(default)]
    pub two_column_layout: Option<TwoColumnLayout>,
    #[serde(default = "default_icon_size")]
    pub icon_size: u32,
    #[serde(default = "default_grid_cols")]
    pub grid_cols: u32,
    #[serde(default = "default_grid_rows")]
    pub grid_rows: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PinnedLaunchMeta {
    pub path: PathBuf,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub args: Option<String>,
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
}

impl PinnedLaunchMeta {
    pub fn key(&self) -> String {
        self.path.to_string_lossy().to_ascii_lowercase()
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum PinnedAppCompat {
    Path(PathBuf),
    Entry(PinnedLaunchMeta),
}

fn deserialize_pinned_apps<'de, D>(deserializer: D) -> Result<Vec<PathBuf>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Vec::<PinnedAppCompat>::deserialize(deserializer)?;
    Ok(raw
        .into_iter()
        .map(|item| match item {
            PinnedAppCompat::Path(path) => path,
            PinnedAppCompat::Entry(entry) => entry.path,
        })
        .collect())
}

fn default_icon_size() -> u32 {
    48
}

fn default_grid_cols() -> u32 {
    3
}

fn default_grid_rows() -> u32 {
    3
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            pinned_apps: Vec::new(),
            pinned_launch_meta: Vec::new(),
            shape: WindowShape::Circle,
            last_pos: None,
            last_size: None,
            quick_launch_app: None,
            two_column_mode: false,
            two_column_layout: None,
            icon_size: default_icon_size(),
            grid_cols: default_grid_cols(),
            grid_rows: default_grid_rows(),
        }
    }
}

impl AppConfig {
    pub fn config_dir() -> Option<PathBuf> {
        directories::ProjectDirs::from("com", "float_launcher", "float_launcher")
            .map(|dirs| dirs.config_dir().to_path_buf())
    }

    pub fn load() -> Self {
        if let Some(proj_dirs) =
            directories::ProjectDirs::from("com", "float_launcher", "float_launcher")
        {
            let config_path = proj_dirs.config_dir().join("config.json");
            if config_path.exists() {
                if let Ok(file) = std::fs::File::open(config_path) {
                    if let Ok(config) = serde_json::from_reader(file) {
                        return config;
                    } else {
                        warn!("Failed to parse config, using default");
                    }
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) {
        if let Some(proj_dirs) =
            directories::ProjectDirs::from("com", "float_launcher", "float_launcher")
        {
            let config_dir = proj_dirs.config_dir();
            if std::fs::create_dir_all(config_dir).is_ok() {
                let config_path = config_dir.join("config.json");
                if let Ok(file) = std::fs::File::create(config_path) {
                    let _ = serde_json::to_writer_pretty(file, self);
                }
            }
        }
    }
}

fn normalize_path_key(path: &Path) -> String {
    path.to_string_lossy().to_ascii_lowercase()
}

fn normalize_launch_key(path: &Path, args: Option<&str>, working_dir: Option<&Path>) -> String {
    let normalized_args = args.map(str::trim).unwrap_or_default();
    let normalized_wd = working_dir.map(normalize_path_key).unwrap_or_default();
    format!(
        "{}|{}|{}",
        normalize_path_key(path),
        normalized_args,
        normalized_wd
    )
}
