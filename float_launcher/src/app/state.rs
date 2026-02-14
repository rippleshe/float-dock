use eframe::egui;
use std::path::PathBuf;
use std::time::Instant;

pub struct PinnedApp {
    pub name: String,
    pub path: PathBuf,
    pub launch_args: Option<String>,
    pub working_dir: Option<PathBuf>,
    pub texture: Option<egui::TextureHandle>,
    pub icon_requested: bool,
}

impl PinnedApp {
    pub fn from_path(path: PathBuf) -> Self {
        Self::new(path, None, None, None)
    }

    pub fn new(
        path: PathBuf,
        name_override: Option<String>,
        launch_args: Option<String>,
        working_dir: Option<PathBuf>,
    ) -> Self {
        let name = name_override
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                path.file_stem()
                    .map(|stem| stem.to_string_lossy().to_string())
                    .filter(|s| !s.trim().is_empty())
            })
            .unwrap_or_else(|| "Unknown".to_string());

        Self {
            name,
            path,
            launch_args,
            working_dir,
            texture: None,
            icon_requested: false,
        }
    }
}

pub struct DropAnim {
    pub item: PinnedApp,
    pub insert_at: usize,
    pub start: Instant,
    pub start_y: f32,
    pub end_y: f32,
}
