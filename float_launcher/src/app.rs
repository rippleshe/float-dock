mod runtime;
mod state;
mod style;
mod ui;

use crate::config::{AppConfig, PinnedLaunchMeta};
use crate::events::{IconRequest, UserEvent};
use crate::system::get_auto_start_status;
use eframe::egui;
use state::{DropAnim, PinnedApp};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Instant;
use tray_icon::{menu::MenuItem, Icon, TrayIcon};

pub const WINDOW_WIDTH: f32 = 320.0;
pub const WINDOW_HEIGHT: f32 = 640.0;
pub const MIN_WINDOW_WIDTH: f32 = 260.0;
pub const MIN_WINDOW_HEIGHT: f32 = 380.0;
const MAX_PINNED_APPS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResizeEdge {
    Left,
    Right,
    Bottom,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ResizeDragState {
    pub edge: ResizeEdge,
    pub start_window_pos: egui::Pos2,
    pub start_window_size: egui::Vec2,
    pub start_global_mouse: egui::Pos2,
}

pub struct MyApp {
    tray_icon: TrayIcon,
    rx: Receiver<UserEvent>,
    icon_req_tx: Sender<IconRequest>,
    is_visible: bool,
    pinned_apps: Vec<PinnedApp>,
    config: AppConfig,
    auto_start_enabled: bool,
    toggle_item: MenuItem,
    icon_awake: Icon,
    icon_sleep: Icon,
    is_dragging_window: bool,
    drag_start_window_pos: Option<egui::Pos2>,
    drag_start_global_mouse: Option<egui::Pos2>,
    resize_drag: Option<ResizeDragState>,
    flash_start_time: Option<Instant>,
    fade_in_start: Option<Instant>,
    fade_out_start: Option<Instant>,
    hide_after_fade: bool,
    dragging_app: Option<usize>,
    drag_target: Option<usize>,
    grid_drag_target: Option<(usize, usize)>,
    selected_app: Option<usize>,
    press_candidate: Option<(usize, Instant, egui::Pos2)>,
    panel_frac: f32,
    panel_anim: Option<(f32, f32, Instant)>,
    drop_anim: Option<DropAnim>,
    warning_message: Option<(String, Instant)>,
}

impl MyApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut config = AppConfig::load();
        let (migrated_paths, migrated_meta) =
            migrate_config_paths(&config.pinned_apps, &config.pinned_launch_meta);
        if config.pinned_apps != migrated_paths || config.pinned_launch_meta != migrated_meta {
            config.pinned_apps = migrated_paths;
            config.pinned_launch_meta = migrated_meta;
            config.save();
        }

        if let Some((x, y)) = config.last_pos {
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x, y)));
        }
        if let Some((w, h)) = config.last_size {
            let restored = sanitize_window_size(egui::vec2(w, h));
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::InnerSize(restored));
        }

        let runtime = runtime::build_runtime(&cc.egui_ctx);
        let launch_meta_by_path: HashMap<String, &PinnedLaunchMeta> = config
            .pinned_launch_meta
            .iter()
            .map(|meta| (meta.key(), meta))
            .collect();
        let pinned_apps = config
            .pinned_apps
            .iter()
            .cloned()
            .map(|path| {
                if let Some(meta) = launch_meta_by_path.get(&normalize_path_key(&path)) {
                    PinnedApp::new(
                        path,
                        meta.display_name.clone(),
                        meta.args.clone(),
                        meta.working_dir.clone(),
                    )
                } else {
                    PinnedApp::from_path(path)
                }
            })
            .collect();

        Self {
            tray_icon: runtime.tray_icon,
            rx: runtime.rx,
            icon_req_tx: runtime.icon_req_tx,
            is_visible: true,
            pinned_apps,
            config,
            auto_start_enabled: get_auto_start_status(),
            toggle_item: runtime.toggle_item,
            icon_awake: runtime.icon_awake,
            icon_sleep: runtime.icon_sleep,
            is_dragging_window: false,
            drag_start_window_pos: None,
            drag_start_global_mouse: None,
            resize_drag: None,
            flash_start_time: None,
            fade_in_start: None,
            fade_out_start: None,
            hide_after_fade: false,
            dragging_app: None,
            drag_target: None,
            grid_drag_target: None,
            selected_app: None,
            press_candidate: None,
            panel_frac: 1.0,
            panel_anim: None,
            drop_anim: None,
            warning_message: None,
        }
    }

    fn start_hide_transition(&mut self) {
        if self.is_visible {
            self.is_visible = false;
            self.fade_out_start = None;
            self.hide_after_fade = false;
            self.toggle_item.set_text("Show");
            let _ = self.tray_icon.set_icon(Some(self.icon_sleep.clone()));
        }
    }

    fn start_show_transition(&mut self, ctx: &egui::Context) {
        if !self.is_visible {
            self.fade_in_start = Some(Instant::now());
            self.fade_out_start = None;
            self.hide_after_fade = false;
            self.is_visible = true;
            self.toggle_item.set_text("Hide");
            let _ = self.tray_icon.set_icon(Some(self.icon_awake.clone()));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    fn sync_config_pins(&mut self) {
        self.config.pinned_apps = self
            .pinned_apps
            .iter()
            .map(|app| app.path.clone())
            .collect();
        self.config.pinned_launch_meta = self
            .pinned_apps
            .iter()
            .filter_map(|app| {
                let args = app.launch_args.clone().and_then(normalize_text_opt);
                let working_dir = app.working_dir.clone();
                let display_name = normalize_text_opt(app.name.clone())
                    .filter(|name| Some(name) != default_display_name(&app.path).as_ref());
                if args.is_none() && working_dir.is_none() && display_name.is_none() {
                    None
                } else {
                    Some(PinnedLaunchMeta {
                        path: app.path.clone(),
                        display_name,
                        args,
                        working_dir,
                    })
                }
            })
            .collect();
        self.config.save();
    }

    fn show_warning<S: Into<String>>(&mut self, message: S) {
        self.warning_message = Some((message.into(), Instant::now()));
    }

    fn save_window_geometry(&mut self, pos: egui::Pos2, size: egui::Vec2) {
        let size = sanitize_window_size(size);
        self.config.last_pos = Some((pos.x, pos.y));
        self.config.last_size = Some((size.x, size.y));
        self.config.save();
    }
}

pub(super) fn sanitize_window_size(size: egui::Vec2) -> egui::Vec2 {
    let width = if size.x.is_finite() {
        size.x
    } else {
        WINDOW_WIDTH
    };
    let height = if size.y.is_finite() {
        size.y
    } else {
        WINDOW_HEIGHT
    };
    egui::vec2(width.max(MIN_WINDOW_WIDTH), height.max(MIN_WINDOW_HEIGHT))
}

pub(super) fn ease_out_elastic(t: f32) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }
    let c4 = (2.0 * std::f32::consts::PI) / 3.0;
    (2.0_f32).powf(-10.0 * t) * ((t * 10.0 - 0.75) * c4).sin() + 1.0
}

fn migrate_config_paths(
    paths: &[PathBuf],
    launch_meta: &[PinnedLaunchMeta],
) -> (Vec<PathBuf>, Vec<PinnedLaunchMeta>) {
    let launch_meta_by_path: HashMap<String, &PinnedLaunchMeta> =
        launch_meta.iter().map(|meta| (meta.key(), meta)).collect();

    let mut migrated = Vec::with_capacity(paths.len());
    let mut seen = HashSet::with_capacity(paths.len());
    let mut migrated_meta: Vec<PinnedLaunchMeta> = Vec::new();

    for path in paths {
        let key_before = normalize_path_key(path);
        let mut resolved_path = path.clone();
        let mut display_name = launch_meta_by_path
            .get(&key_before)
            .and_then(|m| m.display_name.clone())
            .and_then(normalize_text_opt);
        let mut args = launch_meta_by_path
            .get(&key_before)
            .and_then(|m| m.args.clone())
            .and_then(normalize_text_opt);
        let mut working_dir = launch_meta_by_path
            .get(&key_before)
            .and_then(|m| m.working_dir.clone());

        let is_shortcut = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("lnk"))
            .unwrap_or(false);
        if is_shortcut {
            display_name = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .and_then(normalize_text_opt);
        }

        if let Some(shortcut) = crate::system::resolve_shortcut(path) {
            if shortcut.target_path.exists() {
                resolved_path = shortcut.target_path;
                if let Some(v) = shortcut.arguments.and_then(normalize_text_opt) {
                    args = Some(v);
                }
                if let Some(v) = shortcut.working_dir {
                    working_dir = Some(v);
                }
            }
        }

        let key = normalize_path_key(&resolved_path);
        if seen.insert(key) {
            if let Some(default_name) = default_display_name(&resolved_path) {
                if display_name.as_ref() == Some(&default_name) {
                    display_name = None;
                }
            }
            if args.is_some() || working_dir.is_some() || display_name.is_some() {
                migrated_meta.push(PinnedLaunchMeta {
                    path: resolved_path.clone(),
                    display_name,
                    args,
                    working_dir,
                });
            }
            migrated.push(resolved_path);
        }
    }

    (migrated, dedupe_launch_meta(migrated_meta))
}

fn normalize_path_key(path: &Path) -> String {
    path.to_string_lossy().to_ascii_lowercase()
}

fn normalize_text_opt(text: String) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn dedupe_launch_meta(items: Vec<PinnedLaunchMeta>) -> Vec<PinnedLaunchMeta> {
    let mut out = Vec::with_capacity(items.len());
    let mut seen = HashSet::with_capacity(items.len());
    for item in items {
        if seen.insert(item.key()) {
            out.push(item);
        }
    }
    out
}

fn default_display_name(path: &Path) -> Option<String> {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .and_then(normalize_text_opt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn ps_quote(path: &Path) -> String {
        path.to_string_lossy().replace('\'', "''")
    }

    fn norm(path: &Path) -> String {
        path.to_string_lossy()
            .replace('/', "\\")
            .to_ascii_lowercase()
    }

    #[test]
    fn migrate_shortcut_to_target_with_meta() {
        let uniq = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time error")
            .as_nanos();
        let base = std::env::temp_dir().join(format!("float_launcher_migrate_test_{uniq}"));
        std::fs::create_dir_all(&base).expect("create temp dir");

        let target = base.join("dummy.exe");
        std::fs::write(&target, b"MZ").expect("write exe");
        let shortcut = base.join("dummy.lnk");

        let script = format!(
            "$w=New-Object -ComObject WScript.Shell; \
             $s=$w.CreateShortcut('{shortcut}'); \
             $s.TargetPath='{target}'; \
             $s.Arguments='--migrated'; \
             $s.WorkingDirectory='{workdir}'; \
             $s.Save()",
            shortcut = ps_quote(&shortcut),
            target = ps_quote(&target),
            workdir = ps_quote(&base)
        );
        let status = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .status()
            .expect("run powershell");
        assert!(status.success(), "powershell failed to create shortcut");

        let (paths, meta) = migrate_config_paths(&[shortcut.clone()], &[]);
        assert_eq!(paths.len(), 1);
        assert_eq!(norm(&paths[0]), norm(&target));
        assert_eq!(meta.len(), 1);
        assert_eq!(norm(&meta[0].path), norm(&target));
        assert_eq!(meta[0].args.as_deref(), Some("--migrated"));
        assert_eq!(
            meta[0].working_dir.as_ref().map(|p| norm(p)),
            Some(norm(&base))
        );

        let _ = std::fs::remove_file(&shortcut);
        let _ = std::fs::remove_file(&target);
        let _ = std::fs::remove_dir_all(&base);
    }
}
