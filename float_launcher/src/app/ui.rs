use super::state::{DropAnim, PinnedApp};
use super::style::{
    rounding, LauncherTheme, CONTENT_PADDING, DROP_SHADOW, HEADER_HEIGHT, ICON_SIDE, ROW_HEIGHT,
};
use super::{
    ease_out_elastic, sanitize_window_size, MyApp, ResizeDragState, ResizeEdge, MAX_PINNED_APPS,
    MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH,
};
use crate::branding::APP_DISPLAY_NAME;
use crate::config::{TwoColumnEntry, TwoColumnLayout};
use crate::events::{IconRequest, UserEvent};
use crate::system::set_auto_start;
use eframe::egui;
use log::info;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const REORDER_HOLD_MS: u64 = 260;
const REORDER_MOVE_TOLERANCE: f32 = 18.0;
const RESIZE_EDGE_THICKNESS: f32 = 6.0;
const RESIZE_CORNER_SIZE: f32 = 14.0;
const MIN_VISIBLE_WIDTH: f32 = 72.0;

impl eframe::App for MyApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_runtime_events(ctx);
        self.update_panel_animation(ctx);
        self.update_drop_animation(ctx);
        self.handle_dropped_files(ctx);

        if self.handle_fade_out(ctx) {
            return;
        }
        if !self.is_visible {
            return;
        }

        let app_to_remove = self.draw_main_panel(ctx);

        if let Some(index) = app_to_remove {
            if index < self.pinned_apps.len() {
                self.pinned_apps.remove(index);
                if self.config.two_column_mode {
                    self.sync_two_column_layout_from_current();
                }
                self.sync_config_pins();

                if let Some(sel) = self.selected_app {
                    self.selected_app = if sel == index {
                        None
                    } else if sel > index {
                        Some(sel - 1)
                    } else {
                        Some(sel)
                    };
                }
            }
        }
    }
}

impl MyApp {
    fn handle_runtime_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                UserEvent::Show => self.start_show_transition(ctx),
                UserEvent::Hide => self.start_hide_transition(),
                UserEvent::Quit => {
                    info!("Exiting application...");
                    std::process::exit(0);
                }
                UserEvent::IconReady(result) => {
                    for app in &mut self.pinned_apps {
                        if app.path == result.path {
                            if let Some(img) = &result.image {
                                let tex_name = format!("icon:{}", app.path.to_string_lossy());
                                app.texture = Some(ctx.load_texture(
                                    tex_name,
                                    img.clone(),
                                    egui::TextureOptions::LINEAR,
                                ));
                            }
                            break;
                        }
                    }
                }
            }
        }
    }

    fn update_panel_animation(&mut self, ctx: &egui::Context) {
        if let Some((from, to, start)) = self.panel_anim {
            let elapsed = start.elapsed();
            let duration = Duration::from_millis(220);
            let t = (elapsed.as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0);
            let eased = ease_out_elastic(t);
            self.panel_frac = from + (to - from) * eased;
            if t >= 1.0 {
                self.panel_anim = None;
                self.panel_frac = to;
            } else {
                ctx.request_repaint();
            }
        }
    }

    fn update_drop_animation(&mut self, ctx: &egui::Context) {
        if let Some(anim) = &self.drop_anim {
            let elapsed = anim.start.elapsed();
            let duration = Duration::from_millis(200);
            let t = (elapsed.as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0);
            if t >= 1.0 {
                if let Some(done) = self.drop_anim.take() {
                    let insert_at = done.insert_at.min(self.pinned_apps.len());
                    self.pinned_apps.insert(insert_at, done.item);
                    self.sync_config_pins();
                    self.selected_app = Some(insert_at);
                }
            } else {
                ctx.request_repaint();
            }
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.raw.dropped_files.is_empty()) {
            return;
        }

        let dropped_files = ctx.input(|i| i.raw.dropped_files.clone());
        let mut changed = false;

        for file in dropped_files {
            if let Some(path) = file.path {
                match self.try_add_pin(path) {
                    AddPinResult::Added => changed = true,
                    AddPinResult::Duplicate => self.show_warning("Already pinned"),
                    AddPinResult::Unsupported => {
                        self.show_warning("Only .exe/.lnk/folder is supported")
                    }
                    AddPinResult::ShortcutUnresolved => {
                        self.show_warning("Shortcut target not found")
                    }
                    AddPinResult::Missing => self.show_warning("File not found"),
                    AddPinResult::LimitReached => {
                        self.show_warning(format!("Max {} apps", MAX_PINNED_APPS));
                        break;
                    }
                }
            }
        }

        if changed {
            if self.config.two_column_mode {
                self.sync_two_column_layout_from_current();
            }
            self.sync_config_pins();
        }
    }

    fn try_add_pin(&mut self, path: PathBuf) -> AddPinResult {
        if self.pinned_apps.len() >= MAX_PINNED_APPS {
            return AddPinResult::LimitReached;
        }
        if !path.exists() {
            return AddPinResult::Missing;
        }

        let is_shortcut = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("lnk"))
            .unwrap_or(false);

        let mut display_name = None;
        let mut launch_args = None;
        let mut working_dir = None;
        let resolved_path = if is_shortcut {
            let source_name = path.file_stem().map(|s| s.to_string_lossy().to_string());
            match crate::system::resolve_shortcut(&path) {
                Some(shortcut) if shortcut.target_path.exists() => {
                    display_name = source_name;
                    launch_args = shortcut.arguments;
                    working_dir = shortcut.working_dir;
                    shortcut.target_path
                }
                _ => return AddPinResult::ShortcutUnresolved,
            }
        } else {
            path
        };

        if !is_supported_app_path(&resolved_path) {
            return AddPinResult::Unsupported;
        }

        let key = normalize_launch_key(
            &resolved_path,
            launch_args.as_deref(),
            working_dir.as_deref(),
        );
        let exists = self.pinned_apps.iter().any(|app| {
            normalize_launch_key(
                &app.path,
                app.launch_args.as_deref(),
                app.working_dir.as_deref(),
            ) == key
        });
        if exists {
            return AddPinResult::Duplicate;
        }

        self.pinned_apps.push(PinnedApp::new(
            resolved_path,
            display_name,
            launch_args,
            working_dir,
        ));
        AddPinResult::Added
    }

    fn set_two_column_mode(&mut self, enabled: bool) {
        if self.config.two_column_mode == enabled {
            return;
        }

        self.dragging_app = None;
        self.drag_target = None;
        self.press_candidate = None;
        self.drop_anim = None;
        self.grid_drag_target = None;

        if enabled {
            let (left, right) = resolve_two_column_indices(
                &self.pinned_apps,
                self.config.two_column_layout.as_ref(),
            );
            reorder_pinned_apps_by_columns(&mut self.pinned_apps, &left, &right);
            self.config.two_column_mode = true;
            self.config.two_column_layout =
                Some(two_column_layout_from_split(&self.pinned_apps, left.len()));
        } else {
            let (left, right) = resolve_two_column_indices(
                &self.pinned_apps,
                self.config.two_column_layout.as_ref(),
            );
            reorder_pinned_apps_by_columns(&mut self.pinned_apps, &left, &right);
            self.config.two_column_layout =
                Some(two_column_layout_from_split(&self.pinned_apps, left.len()));
            self.config.two_column_mode = false;
        }

        self.sync_config_pins();
    }

    fn sync_two_column_layout_from_current(&mut self) {
        if !self.config.two_column_mode {
            return;
        }

        let (left, right) =
            resolve_two_column_indices(&self.pinned_apps, self.config.two_column_layout.as_ref());
        reorder_pinned_apps_by_columns(&mut self.pinned_apps, &left, &right);
        self.config.two_column_layout =
            Some(two_column_layout_from_split(&self.pinned_apps, left.len()));
    }

    fn handle_fade_out(&mut self, ctx: &egui::Context) -> bool {
        if let Some(start) = self.fade_out_start {
            let elapsed = start.elapsed();
            let duration = Duration::from_millis(150);
            if elapsed >= duration {
                self.fade_out_start = None;
                if self.hide_after_fade {
                    self.is_visible = false;
                    self.hide_after_fade = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    return true;
                }
                return true;
            }
            ctx.request_repaint();
        }
        false
    }

    fn draw_main_panel(&mut self, ctx: &egui::Context) -> Option<usize> {
        let theme = LauncherTheme::default();
        let panel_rounding = rounding(self.config.shape);
        let panel_frame = egui::Frame::none()
            .fill(egui::Color32::TRANSPARENT)
            .stroke(egui::Stroke::NONE);
        let is_dragging_file = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if is_dragging_file {
            ctx.request_repaint();
        }

        egui::CentralPanel::default()
            .frame(panel_frame)
            .show(ctx, |ui| {
                let response =
                    ui.allocate_response(ui.available_size(), egui::Sense::click_and_drag());

                let window_rect = ctx
                    .input(|i| i.viewport().outer_rect)
                    .unwrap_or(egui::Rect::ZERO);

                ui.painter().rect_filled(
                    response.rect.expand(10.0),
                    panel_rounding + 10.0,
                    theme.panel_shadow,
                );
                ui.painter()
                    .rect_filled(response.rect, panel_rounding, theme.panel_bg_bottom);
                paint_glow_blob(
                    ui.painter(),
                    egui::pos2(response.rect.right() - 28.0, response.rect.top() + 12.0),
                    62.0,
                    egui::Color32::from_rgba_premultiplied(75, 197, 165, 6),
                );
                paint_glow_blob(
                    ui.painter(),
                    egui::pos2(response.rect.left() + 50.0, response.rect.top() + 40.0),
                    44.0,
                    egui::Color32::from_rgba_premultiplied(120, 175, 240, 5),
                );
                ui.painter().rect_stroke(
                    response.rect,
                    panel_rounding,
                    egui::Stroke::new(1.0, theme.panel_border),
                );

                let header_rect = egui::Rect::from_min_size(
                    response.rect.min,
                    egui::vec2(response.rect.width(), HEADER_HEIGHT),
                );
                ui.painter()
                    .rect_filled(header_rect, panel_rounding, theme.header_bg_bottom);

                self.draw_header(ui, header_rect, &theme);
                let handle_resp = ui.allocate_rect(header_rect, egui::Sense::click_and_drag());
                let panel_size = response.rect.size();
                self.ensure_window_visible(ctx, window_rect, panel_size);
                self.handle_window_drag(ctx, ui, &handle_resp, window_rect, panel_size);
                self.draw_resize_handles(ui, ctx, response.rect, window_rect, panel_size);
                self.update_resize_drag(ctx, window_rect, panel_size);

                if response.double_clicked() {
                    if let Some(path) = &self.config.quick_launch_app {
                        if crate::system::shell_open(path) {
                            self.flash_start_time = Some(Instant::now());
                        }
                    }
                }

                response.context_menu(|ui| self.draw_context_menu(ui));

                let mut remove_idx = None;
                let content_h = (response.rect.height() - HEADER_HEIGHT).max(0.0);
                let visible_h = (self.panel_frac * content_h).clamp(0.0, content_h);
                let content_rect = egui::Rect::from_min_max(
                    egui::pos2(response.rect.min.x, response.rect.min.y + HEADER_HEIGHT),
                    egui::pos2(
                        response.rect.max.x,
                        response.rect.min.y + HEADER_HEIGHT + visible_h,
                    ),
                );

                if visible_h > 0.0 {
                    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(content_rect), |ui| {
                        remove_idx =
                            self.draw_pinned_list(ui, ctx, content_rect, &theme, is_dragging_file);
                    });
                }

                self.draw_flash_overlay(ui);
                self.draw_warning_overlay(ui, &theme);
                self.draw_fade_in_overlay(ui, panel_rounding);

                remove_idx
            })
            .inner
    }

    fn draw_header(&self, ui: &egui::Ui, header_rect: egui::Rect, theme: &LauncherTheme) {
        ui.painter().text(
            egui::pos2(header_rect.min.x + 12.0, header_rect.center().y),
            egui::Align2::LEFT_CENTER,
            APP_DISPLAY_NAME,
            egui::FontId::proportional(15.0),
            theme.title_color,
        );
    }

    fn ensure_window_visible(
        &mut self,
        ctx: &egui::Context,
        window_rect: egui::Rect,
        panel_size: egui::Vec2,
    ) {
        if self.is_dragging_window || self.resize_drag.is_some() {
            return;
        }

        let Some(monitor_size) = ctx.input(|i| i.viewport().monitor_size) else {
            return;
        };
        let window_size = sanitize_window_size(panel_size);
        let clamped = clamp_window_origin(window_rect.min, window_size, monitor_size);

        if (clamped.x - window_rect.min.x).abs() > 0.5
            || (clamped.y - window_rect.min.y).abs() > 0.5
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(clamped));
            self.save_window_geometry(clamped, window_size);
        }
    }

    fn handle_window_drag(
        &mut self,
        ctx: &egui::Context,
        ui: &egui::Ui,
        handle_resp: &egui::Response,
        window_rect: egui::Rect,
        panel_size: egui::Vec2,
    ) {
        if handle_resp.drag_started_by(egui::PointerButton::Primary) {
            self.is_dragging_window = true;
            self.drag_start_window_pos = Some(window_rect.min);
            if let Some(hover_pos) = ctx.input(|i| i.pointer.hover_pos()) {
                self.drag_start_global_mouse = Some(window_rect.min + hover_pos.to_vec2());
            }
        }

        if !self.is_dragging_window {
            return;
        }

        if ctx.input(|i| i.pointer.button_released(egui::PointerButton::Primary)) {
            self.is_dragging_window = false;

            let snap_threshold = 48.0;
            let mut new_pos = window_rect.min;
            let window_size = sanitize_window_size(panel_size);

            if let Some(monitor_size) = ctx.input(|i| i.viewport().monitor_size) {
                if new_pos.x.abs() < snap_threshold {
                    new_pos.x = 0.0;
                } else if (new_pos.x + window_size.x - monitor_size.x).abs() < snap_threshold {
                    new_pos.x = monitor_size.x - window_size.x;
                }

                if new_pos.y.abs() < snap_threshold {
                    new_pos.y = 0.0;
                } else if (new_pos.y + window_size.y - monitor_size.y).abs() < snap_threshold {
                    new_pos.y = monitor_size.y - window_size.y;
                }

                new_pos = clamp_window_origin(new_pos, window_size, monitor_size);
            }

            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(new_pos));
            self.save_window_geometry(new_pos, window_size);

            self.drag_start_window_pos = None;
            self.drag_start_global_mouse = None;
            return;
        }

        if let (Some(start_win_pos), Some(start_global_mouse)) =
            (self.drag_start_window_pos, self.drag_start_global_mouse)
        {
            if let Some(hover_pos) = ctx.input(|i| i.pointer.hover_pos()) {
                let current_global_mouse = window_rect.min + hover_pos.to_vec2();
                let delta = current_global_mouse - start_global_mouse;
                let mut new_origin = start_win_pos + delta;

                if let Some(monitor_size) = ctx.input(|i| i.viewport().monitor_size) {
                    let snap_threshold = 48.0;
                    let window_size = sanitize_window_size(panel_size);
                    new_origin = clamp_window_origin(new_origin, window_size, monitor_size);
                    let snap_color = egui::Color32::from_rgba_premultiplied(75, 197, 165, 160);
                    let stroke = egui::Stroke::new(2.0, snap_color);

                    if new_origin.x.abs() < snap_threshold {
                        ui.painter()
                            .vline(0.0, egui::Rangef::new(0.0, window_size.y), stroke);
                    }
                    if (new_origin.x + window_size.x - monitor_size.x).abs() < snap_threshold {
                        ui.painter().vline(
                            window_size.x - 2.0,
                            egui::Rangef::new(0.0, window_size.y),
                            stroke,
                        );
                    }
                    if new_origin.y.abs() < snap_threshold {
                        ui.painter()
                            .hline(egui::Rangef::new(0.0, window_size.x), 0.0, stroke);
                    }
                    if (new_origin.y + window_size.y - monitor_size.y).abs() < snap_threshold {
                        ui.painter().hline(
                            egui::Rangef::new(0.0, window_size.x),
                            window_size.y - 2.0,
                            stroke,
                        );
                    }
                }

                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(new_origin));
            }
        }
    }

    fn draw_resize_handles(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        panel_rect: egui::Rect,
        window_rect: egui::Rect,
        panel_size: egui::Vec2,
    ) {
        let left = egui::Rect::from_min_max(
            panel_rect.min,
            egui::pos2(panel_rect.min.x + RESIZE_EDGE_THICKNESS, panel_rect.max.y),
        );
        let right = egui::Rect::from_min_max(
            egui::pos2(panel_rect.max.x - RESIZE_EDGE_THICKNESS, panel_rect.min.y),
            panel_rect.max,
        );
        let bottom = egui::Rect::from_min_max(
            egui::pos2(panel_rect.min.x, panel_rect.max.y - RESIZE_EDGE_THICKNESS),
            panel_rect.max,
        );

        let bottom_left = egui::Rect::from_min_max(
            egui::pos2(panel_rect.min.x, panel_rect.max.y - RESIZE_CORNER_SIZE),
            egui::pos2(panel_rect.min.x + RESIZE_CORNER_SIZE, panel_rect.max.y),
        );
        let bottom_right = egui::Rect::from_min_max(
            panel_rect.max - egui::vec2(RESIZE_CORNER_SIZE, RESIZE_CORNER_SIZE),
            panel_rect.max,
        );

        self.interact_resize_zone(
            ui,
            ctx,
            ResizeEdge::BottomLeft,
            bottom_left,
            window_rect,
            panel_size,
        );
        self.interact_resize_zone(
            ui,
            ctx,
            ResizeEdge::BottomRight,
            bottom_right,
            window_rect,
            panel_size,
        );
        self.interact_resize_zone(ui, ctx, ResizeEdge::Left, left, window_rect, panel_size);
        self.interact_resize_zone(ui, ctx, ResizeEdge::Right, right, window_rect, panel_size);
        self.interact_resize_zone(ui, ctx, ResizeEdge::Bottom, bottom, window_rect, panel_size);
    }

    fn interact_resize_zone(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        edge: ResizeEdge,
        zone: egui::Rect,
        window_rect: egui::Rect,
        panel_size: egui::Vec2,
    ) {
        let id = ui.make_persistent_id(("resize_zone", resize_edge_key(edge)));
        let response = ui.interact(zone, id, egui::Sense::click_and_drag());

        if response.hovered() || response.dragged() {
            ui.output_mut(|o| {
                o.cursor_icon = resize_edge_cursor(edge);
            });
        }

        if response.drag_started_by(egui::PointerButton::Primary) {
            self.is_dragging_window = false;
            self.drag_start_window_pos = None;
            self.drag_start_global_mouse = None;

            if let Some(hover_pos) = ctx.input(|i| i.pointer.hover_pos()) {
                self.resize_drag = Some(ResizeDragState {
                    edge,
                    start_window_pos: window_rect.min,
                    start_window_size: sanitize_window_size(panel_size),
                    start_global_mouse: window_rect.min + hover_pos.to_vec2(),
                });
            }
        }
    }

    fn update_resize_drag(
        &mut self,
        ctx: &egui::Context,
        window_rect: egui::Rect,
        panel_size: egui::Vec2,
    ) {
        let Some(state) = self.resize_drag else {
            return;
        };

        if ctx.input(|i| i.pointer.button_released(egui::PointerButton::Primary)) {
            self.resize_drag = None;
            let saved_pos = ctx
                .input(|i| i.viewport().outer_rect)
                .map(|r| r.min)
                .unwrap_or(window_rect.min);
            let saved_size = ctx
                .input(|i| i.viewport().inner_rect)
                .map(|r| r.size())
                .unwrap_or_else(|| sanitize_window_size(panel_size));
            let saved_pos = if let Some(monitor_size) = ctx.input(|i| i.viewport().monitor_size) {
                clamp_window_origin(saved_pos, sanitize_window_size(saved_size), monitor_size)
            } else {
                saved_pos
            };
            self.save_window_geometry(saved_pos, saved_size);
            return;
        }

        let Some(hover_pos) = ctx.input(|i| i.pointer.hover_pos()) else {
            return;
        };
        let current_origin = ctx
            .input(|i| i.viewport().outer_rect)
            .map(|r| r.min)
            .unwrap_or(window_rect.min);
        let current_global_mouse = current_origin + hover_pos.to_vec2();
        let delta = current_global_mouse - state.start_global_mouse;
        let (new_pos, new_size) = self.apply_resize_delta(ctx, state, delta);

        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(new_pos));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(new_size));
        ctx.request_repaint();
    }

    fn apply_resize_delta(
        &self,
        ctx: &egui::Context,
        state: ResizeDragState,
        delta: egui::Vec2,
    ) -> (egui::Pos2, egui::Vec2) {
        let max_size = ctx
            .input(|i| i.viewport().monitor_size)
            .map(|size| {
                egui::vec2(
                    (size.x - 8.0).max(MIN_WINDOW_WIDTH),
                    (size.y - 8.0).max(MIN_WINDOW_HEIGHT),
                )
            })
            .unwrap_or(egui::vec2(4096.0, 4096.0));

        let clamp_width = |w: f32| w.clamp(MIN_WINDOW_WIDTH, max_size.x);
        let clamp_height = |h: f32| h.clamp(MIN_WINDOW_HEIGHT, max_size.y);

        let mut pos = state.start_window_pos;
        let mut size = state.start_window_size;

        match state.edge {
            ResizeEdge::Left => {
                let new_width = clamp_width(state.start_window_size.x - delta.x);
                pos.x = state.start_window_pos.x + (state.start_window_size.x - new_width);
                size.x = new_width;
            }
            ResizeEdge::Right => {
                size.x = clamp_width(state.start_window_size.x + delta.x);
            }
            ResizeEdge::Bottom => {
                size.y = clamp_height(state.start_window_size.y + delta.y);
            }
            ResizeEdge::BottomLeft => {
                let new_width = clamp_width(state.start_window_size.x - delta.x);
                pos.x = state.start_window_pos.x + (state.start_window_size.x - new_width);
                size.x = new_width;
                size.y = clamp_height(state.start_window_size.y + delta.y);
            }
            ResizeEdge::BottomRight => {
                size.x = clamp_width(state.start_window_size.x + delta.x);
                size.y = clamp_height(state.start_window_size.y + delta.y);
            }
        }

        let size = sanitize_window_size(size);
        if let Some(monitor_size) = ctx.input(|i| i.viewport().monitor_size) {
            pos = clamp_window_origin(pos, size, monitor_size);
        }

        (pos, size)
    }
    fn draw_context_menu(&mut self, ui: &mut egui::Ui) {
        style_compact_menu(ui);
        if ui
            .checkbox(&mut self.auto_start_enabled, "Auto-start")
            .clicked()
        {
            if let Err(err) = set_auto_start(self.auto_start_enabled) {
                eprintln!("Failed to set auto-start: {}", err);
                self.auto_start_enabled = !self.auto_start_enabled;
                self.show_warning("Auto-start failed");
            }
        }

        let mut two_column_mode = self.config.two_column_mode;
        if ui
            .checkbox(&mut two_column_mode, "Two-column mode")
            .changed()
        {
            self.set_two_column_mode(two_column_mode);
        }

        ui.separator();
        if ui.button("Quit").clicked() {
            info!("Exiting via context menu...");
            std::process::exit(0);
        }
    }

    fn draw_pinned_list(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        content_rect: egui::Rect,
        theme: &LauncherTheme,
        is_dragging_file: bool,
    ) -> Option<usize> {
        ui.add_space(CONTENT_PADDING);
        let list_width = (content_rect.width() - CONTENT_PADDING * 2.0).max(160.0);

        if self.config.two_column_mode {
            return self.draw_pinned_grid(
                ui,
                ctx,
                content_rect,
                theme,
                is_dragging_file,
                list_width,
            );
        }
        self.grid_drag_target = None;

        if self.pinned_apps.is_empty() {
            let empty_rect = egui::Rect::from_min_max(
                egui::pos2(
                    content_rect.min.x + CONTENT_PADDING,
                    content_rect.min.y + CONTENT_PADDING,
                ),
                egui::pos2(
                    content_rect.max.x - CONTENT_PADDING,
                    content_rect.max.y - CONTENT_PADDING,
                ),
            );
            ui.painter()
                .rect_stroke(empty_rect, 12.0, egui::Stroke::new(1.0, theme.drop_hint));
            ui.painter().text(
                empty_rect.center_top() + egui::vec2(0.0, 42.0),
                egui::Align2::CENTER_CENTER,
                "Drop app here",
                egui::FontId::proportional(16.0),
                theme.title_color,
            );
            if is_dragging_file {
                ui.painter().rect_filled(
                    empty_rect,
                    12.0,
                    egui::Color32::from_rgba_premultiplied(75, 197, 165, 26),
                );
            }
            return None;
        }

        let drag_i = if self.drop_anim.is_some() {
            None
        } else {
            self.dragging_app
        };
        let placeholder_slot = self
            .drop_anim
            .as_ref()
            .map(|anim| anim.insert_at)
            .or(self.drag_target);
        let pointer_pos = ctx.input(|i| i.pointer.hover_pos());
        let mut rects_for_target: Vec<egui::Rect> = Vec::new();
        let mut remove_idx = None;

        egui::ScrollArea::vertical()
            .max_height(content_rect.height() - CONTENT_PADDING * 2.0)
            .show(ui, |ui| {
                let mut slot_index = 0usize;

                for idx in 0..self.pinned_apps.len() {
                    if drag_i == Some(idx) {
                        continue;
                    }

                    if placeholder_slot == Some(slot_index)
                        && (self.dragging_app.is_some() || self.drop_anim.is_some())
                    {
                        let (r, _) = ui.allocate_exact_size(
                            egui::vec2(list_width, ROW_HEIGHT),
                            egui::Sense::hover(),
                        );
                        ui.painter()
                            .rect_stroke(r, 8.0, egui::Stroke::new(1.0, theme.drop_hint));
                        ui.add_space(5.0);
                    }

                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(list_width, ROW_HEIGHT),
                        egui::Sense::click_and_drag(),
                    );
                    rects_for_target.push(rect);

                    if resp.is_pointer_button_down_on()
                        && self.drop_anim.is_none()
                        && self.dragging_app.is_none()
                        && self.press_candidate.is_none()
                    {
                        if let Some(p) = ctx.input(|i| i.pointer.hover_pos()) {
                            self.press_candidate = Some((idx, Instant::now(), p));
                        }
                    }

                    let is_selected = self.selected_app == Some(idx);
                    let fill = if is_selected {
                        theme.row_selected
                    } else if resp.hovered() {
                        theme.row_hover
                    } else {
                        theme.row_bg
                    };
                    ui.painter().rect_filled(rect, 8.0, fill);
                    if is_selected || resp.hovered() {
                        ui.painter().rect_stroke(
                            rect,
                            8.0,
                            egui::Stroke::new(1.0, theme.row_border),
                        );
                    }

                    let icon_rect = egui::Rect::from_center_size(
                        egui::pos2(rect.min.x + 14.0 + ICON_SIDE * 0.5, rect.center().y),
                        egui::vec2(ICON_SIDE, ICON_SIDE),
                    );

                    if self.pinned_apps[idx].texture.is_none()
                        && !self.pinned_apps[idx].icon_requested
                    {
                        self.pinned_apps[idx].icon_requested = true;
                        let _ = self.icon_req_tx.send(IconRequest {
                            path: self.pinned_apps[idx].path.clone(),
                            name_hint: Some(self.pinned_apps[idx].name.clone()),
                            size: self.config.icon_size,
                        });
                    }

                    if let Some(tex) = &self.pinned_apps[idx].texture {
                        ui.painter().image(
                            tex.id(),
                            icon_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    } else {
                        ui.painter()
                            .rect_filled(icon_rect, 5.0, theme.icon_placeholder);
                    }

                    let text_pos = egui::pos2(icon_rect.max.x + 9.0, rect.center().y);
                    ui.painter().text(
                        text_pos,
                        egui::Align2::LEFT_CENTER,
                        &self.pinned_apps[idx].name,
                        egui::FontId::proportional(14.0),
                        theme.title_color,
                    );

                    let resp = resp.on_hover_text(self.pinned_apps[idx].path.to_string_lossy());
                    if self.dragging_app.is_none() {
                        if resp.double_clicked() {
                            let app = &self.pinned_apps[idx];
                            let _ = crate::system::shell_open_with(
                                &app.path,
                                app.launch_args.as_deref(),
                                app.working_dir.as_deref(),
                            );
                        } else if resp.clicked() {
                            self.selected_app = Some(idx);
                        }
                    }

                    resp.context_menu(|ui| {
                        if ui.button("Remove").clicked() {
                            remove_idx = Some(idx);
                            ui.close_menu();
                        }
                    });

                    ui.add_space(5.0);
                    slot_index += 1;
                }

                if placeholder_slot == Some(slot_index)
                    && (self.dragging_app.is_some() || self.drop_anim.is_some())
                {
                    let (r, _) = ui.allocate_exact_size(
                        egui::vec2(list_width, ROW_HEIGHT),
                        egui::Sense::hover(),
                    );
                    ui.painter()
                        .rect_stroke(r, 12.0, egui::Stroke::new(1.0, theme.drop_hint));
                }
            });

        if drag_i.is_some() && pointer_pos.is_some() {
            let py = pointer_pos.unwrap().y;
            let mut target = rects_for_target.len();
            for (pos, rect) in rects_for_target.iter().enumerate() {
                if py < rect.center().y {
                    target = pos;
                    break;
                }
            }
            if self.drag_target != Some(target) {
                self.drag_target = Some(target);
                ctx.request_repaint();
            }
        }

        if let Some((idx, start, start_pos)) = self.press_candidate {
            // Keep repainting while pressing so long-press timing is reliable even when pointer is still.
            ctx.request_repaint_after(Duration::from_millis(16));
            let down = ctx.input(|i| i.pointer.primary_down());
            let cur = ctx.input(|i| i.pointer.hover_pos());
            if !down {
                self.press_candidate = None;
            } else if let Some(p) = cur {
                if p.distance(start_pos) > REORDER_MOVE_TOLERANCE {
                    self.press_candidate = None;
                } else if start.elapsed() >= Duration::from_millis(REORDER_HOLD_MS) {
                    self.dragging_app = Some(idx);
                    self.drag_target = Some(idx.min(self.pinned_apps.len()));
                    self.press_candidate = None;
                    ctx.request_repaint();
                }
            }
        }

        if self.drop_anim.is_none()
            && self.dragging_app.is_some()
            && ctx.input(|i| i.pointer.primary_released())
        {
            if let (Some(from), Some(slot)) = (self.dragging_app.take(), self.drag_target.take()) {
                if from < self.pinned_apps.len() {
                    let start_y = ctx
                        .input(|i| i.pointer.hover_pos())
                        .map(|p| p.y - ROW_HEIGHT * 0.5)
                        .unwrap_or(content_rect.min.y + CONTENT_PADDING);
                    let end_y = if slot < rects_for_target.len() {
                        rects_for_target[slot].min.y
                    } else {
                        rects_for_target
                            .last()
                            .map(|r| r.max.y + 8.0)
                            .unwrap_or(content_rect.min.y + CONTENT_PADDING)
                    };
                    let item = self.pinned_apps.remove(from);
                    let insert_at = slot.min(self.pinned_apps.len());
                    self.drop_anim = Some(DropAnim {
                        item,
                        insert_at,
                        start: Instant::now(),
                        start_y,
                        end_y,
                    });
                    ctx.request_repaint();
                }
            }
        }

        self.draw_drag_row_overlay(ctx, content_rect, list_width, theme);
        remove_idx
    }

    fn draw_pinned_grid(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        content_rect: egui::Rect,
        theme: &LauncherTheme,
        is_dragging_file: bool,
        list_width: f32,
    ) -> Option<usize> {
        if self.pinned_apps.is_empty() {
            let empty_rect = egui::Rect::from_min_max(
                egui::pos2(
                    content_rect.min.x + CONTENT_PADDING,
                    content_rect.min.y + CONTENT_PADDING,
                ),
                egui::pos2(
                    content_rect.max.x - CONTENT_PADDING,
                    content_rect.max.y - CONTENT_PADDING,
                ),
            );
            ui.painter()
                .rect_stroke(empty_rect, 12.0, egui::Stroke::new(1.0, theme.drop_hint));
            ui.painter().text(
                empty_rect.center_top() + egui::vec2(0.0, 42.0),
                egui::Align2::CENTER_CENTER,
                "Drop app here",
                egui::FontId::proportional(16.0),
                theme.title_color,
            );
            if is_dragging_file {
                ui.painter().rect_filled(
                    empty_rect,
                    12.0,
                    egui::Color32::from_rgba_premultiplied(75, 197, 165, 26),
                );
            }
            return None;
        }

        let col_gap = 8.0;
        let row_gap = 6.0;
        let cell_width = ((list_width - col_gap).max(220.0)) * 0.5;
        let column_left_x = content_rect.min.x + CONTENT_PADDING;
        let column_right_x = column_left_x + cell_width + col_gap;

        let (left_indices, right_indices) =
            resolve_two_column_indices(&self.pinned_apps, self.config.two_column_layout.as_ref());

        let dragging_idx = self
            .dragging_app
            .filter(|idx| *idx < self.pinned_apps.len());
        let mut left_draw = left_indices.clone();
        let mut right_draw = right_indices.clone();
        if let Some(drag_idx) = dragging_idx {
            if let Some(pos) = left_draw.iter().position(|&idx| idx == drag_idx) {
                left_draw.remove(pos);
            } else if let Some(pos) = right_draw.iter().position(|&idx| idx == drag_idx) {
                right_draw.remove(pos);
            }
        }

        let mut remove_idx = None;
        let mut left_rects: Vec<egui::Rect> = Vec::new();
        let mut right_rects: Vec<egui::Rect> = Vec::new();

        egui::ScrollArea::vertical()
            .max_height(content_rect.height() - CONTENT_PADDING * 2.0)
            .show(ui, |ui| {
                let row_count = left_draw.len().max(right_draw.len());
                for row in 0..row_count {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = col_gap;
                        for col in 0..2 {
                            let app_idx = if col == 0 {
                                left_draw.get(row).copied()
                            } else {
                                right_draw.get(row).copied()
                            };

                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(cell_width, ROW_HEIGHT),
                                egui::Sense::click_and_drag(),
                            );

                            let Some(idx) = app_idx else {
                                continue;
                            };

                            if col == 0 {
                                left_rects.push(rect);
                            } else {
                                right_rects.push(rect);
                            }

                            if resp.is_pointer_button_down_on()
                                && self.drop_anim.is_none()
                                && self.dragging_app.is_none()
                                && self.press_candidate.is_none()
                            {
                                if let Some(p) = ctx.input(|i| i.pointer.hover_pos()) {
                                    self.press_candidate = Some((idx, Instant::now(), p));
                                }
                            }

                            let is_selected = self.selected_app == Some(idx);
                            let fill = if is_selected {
                                theme.row_selected
                            } else if resp.hovered() {
                                theme.row_hover
                            } else {
                                theme.row_bg
                            };
                            ui.painter().rect_filled(rect, 8.0, fill);
                            if is_selected || resp.hovered() {
                                ui.painter().rect_stroke(
                                    rect,
                                    8.0,
                                    egui::Stroke::new(1.0, theme.row_border),
                                );
                            }

                            let icon_rect = egui::Rect::from_center_size(
                                egui::pos2(rect.min.x + 10.0 + ICON_SIDE * 0.5, rect.center().y),
                                egui::vec2(ICON_SIDE, ICON_SIDE),
                            );

                            if self.pinned_apps[idx].texture.is_none()
                                && !self.pinned_apps[idx].icon_requested
                            {
                                self.pinned_apps[idx].icon_requested = true;
                                let _ = self.icon_req_tx.send(IconRequest {
                                    path: self.pinned_apps[idx].path.clone(),
                                    name_hint: Some(self.pinned_apps[idx].name.clone()),
                                    size: self.config.icon_size,
                                });
                            }

                            if let Some(tex) = &self.pinned_apps[idx].texture {
                                ui.painter().image(
                                    tex.id(),
                                    icon_rect,
                                    egui::Rect::from_min_max(
                                        egui::pos2(0.0, 0.0),
                                        egui::pos2(1.0, 1.0),
                                    ),
                                    egui::Color32::WHITE,
                                );
                            } else {
                                ui.painter()
                                    .rect_filled(icon_rect, 5.0, theme.icon_placeholder);
                            }

                            let text_rect = egui::Rect::from_min_max(
                                egui::pos2(icon_rect.max.x + 8.0, rect.min.y + 2.0),
                                egui::pos2(rect.max.x - 8.0, rect.max.y - 2.0),
                            );
                            let text_painter = ui.painter().with_clip_rect(text_rect);
                            text_painter.text(
                                egui::pos2(text_rect.min.x, rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                &self.pinned_apps[idx].name,
                                egui::FontId::proportional(14.0),
                                theme.title_color,
                            );

                            let resp =
                                resp.on_hover_text(self.pinned_apps[idx].path.to_string_lossy());
                            if self.dragging_app.is_none() {
                                if resp.double_clicked() {
                                    let app = &self.pinned_apps[idx];
                                    let _ = crate::system::shell_open_with(
                                        &app.path,
                                        app.launch_args.as_deref(),
                                        app.working_dir.as_deref(),
                                    );
                                } else if resp.clicked() {
                                    self.selected_app = Some(idx);
                                }
                            }

                            resp.context_menu(|ui| {
                                if ui.button("Remove").clicked() {
                                    remove_idx = Some(idx);
                                    ui.close_menu();
                                }
                            });
                        }
                    });

                    if row + 1 < row_count {
                        ui.add_space(row_gap);
                    }
                }
            });

        if let Some((idx, start, start_pos)) = self.press_candidate {
            ctx.request_repaint_after(Duration::from_millis(16));
            let down = ctx.input(|i| i.pointer.primary_down());
            let cur = ctx.input(|i| i.pointer.hover_pos());
            if !down {
                self.press_candidate = None;
            } else if let Some(p) = cur {
                if p.distance(start_pos) > REORDER_MOVE_TOLERANCE {
                    self.press_candidate = None;
                } else if start.elapsed() >= Duration::from_millis(REORDER_HOLD_MS) {
                    self.dragging_app = Some(idx);
                    self.drag_target = None;
                    self.grid_drag_target = find_column_slot(idx, &left_indices, &right_indices);
                    self.press_candidate = None;
                    ctx.request_repaint();
                }
            }
        }

        if let (Some(_drag_idx), Some(pointer_pos)) =
            (dragging_idx, ctx.input(|i| i.pointer.hover_pos()))
        {
            let target_col = if pointer_pos.x < column_right_x { 0 } else { 1 };
            let target_rects = if target_col == 0 {
                &left_rects
            } else {
                &right_rects
            };
            let max_slot = if target_col == 0 {
                left_draw.len()
            } else {
                right_draw.len()
            };
            let target_slot = slot_from_pointer(pointer_pos.y, target_rects).min(max_slot);
            let target = Some((target_col, target_slot));
            if self.grid_drag_target != target {
                self.grid_drag_target = target;
                ctx.request_repaint();
            }
        }

        if self.dragging_app.is_some() && ctx.input(|i| i.pointer.primary_released()) {
            if let Some(from_idx) = self.dragging_app.take() {
                if let Some((from_col, from_slot)) =
                    find_column_slot(from_idx, &left_indices, &right_indices)
                {
                    let mut left_new = left_indices.clone();
                    let mut right_new = right_indices.clone();

                    if from_col == 0 {
                        left_new.remove(from_slot);
                    } else {
                        right_new.remove(from_slot);
                    }

                    let (target_col, target_slot) = self
                        .grid_drag_target
                        .take()
                        .unwrap_or((from_col, from_slot));
                    let insert_vec = if target_col == 0 {
                        &mut left_new
                    } else {
                        &mut right_new
                    };
                    let insert_slot = target_slot.min(insert_vec.len());
                    insert_vec.insert(insert_slot, from_idx);

                    if left_new != left_indices || right_new != right_indices {
                        reorder_pinned_apps_by_columns(
                            &mut self.pinned_apps,
                            &left_new,
                            &right_new,
                        );
                        self.config.two_column_layout = Some(two_column_layout_from_split(
                            &self.pinned_apps,
                            left_new.len(),
                        ));
                        self.sync_config_pins();

                        let selected_idx = if target_col == 0 {
                            insert_slot
                        } else {
                            left_new.len() + insert_slot
                        };
                        self.selected_app =
                            Some(selected_idx.min(self.pinned_apps.len().saturating_sub(1)));
                    }
                }
            }

            self.grid_drag_target = None;
            self.drag_target = None;
            ctx.request_repaint();
        }

        if let Some((target_col, target_slot)) = self.grid_drag_target {
            if dragging_idx.is_some() {
                let target_rects = if target_col == 0 {
                    &left_rects
                } else {
                    &right_rects
                };
                let x = if target_col == 0 {
                    column_left_x
                } else {
                    column_right_x
                };
                let y = if target_slot < target_rects.len() {
                    target_rects[target_slot].min.y
                } else {
                    target_rects
                        .last()
                        .map(|r| r.max.y + row_gap)
                        .unwrap_or(content_rect.min.y + CONTENT_PADDING)
                };

                let placeholder =
                    egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(cell_width, ROW_HEIGHT));
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::new("grid_drop_placeholder"),
                ));
                painter.rect_stroke(placeholder, 8.0, egui::Stroke::new(1.0, theme.drop_hint));
            }
        }

        if let (Some(drag_idx), Some(pointer_pos)) =
            (dragging_idx, ctx.input(|i| i.pointer.hover_pos()))
        {
            let ghost_rect = egui::Rect::from_min_size(
                egui::pos2(
                    pointer_pos.x - cell_width * 0.5,
                    pointer_pos.y - ROW_HEIGHT * 0.5,
                ),
                egui::vec2(cell_width, ROW_HEIGHT),
            );
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("grid_drag_preview"),
            ));
            painter.rect_filled(
                ghost_rect.expand(DROP_SHADOW),
                12.0 + DROP_SHADOW,
                egui::Color32::from_rgba_premultiplied(0, 0, 0, 32),
            );
            painter.rect_filled(ghost_rect, 8.0, theme.row_selected);
            painter.rect_stroke(ghost_rect, 8.0, egui::Stroke::new(1.0, theme.drop_hint));

            let icon_rect = egui::Rect::from_center_size(
                egui::pos2(
                    ghost_rect.min.x + 10.0 + ICON_SIDE * 0.5,
                    ghost_rect.center().y,
                ),
                egui::vec2(ICON_SIDE, ICON_SIDE),
            );
            if let Some(tex) = &self.pinned_apps[drag_idx].texture {
                painter.image(
                    tex.id(),
                    icon_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            } else {
                painter.rect_filled(icon_rect, 5.0, theme.icon_placeholder);
            }
            painter.text(
                egui::pos2(icon_rect.max.x + 8.0, ghost_rect.center().y),
                egui::Align2::LEFT_CENTER,
                &self.pinned_apps[drag_idx].name,
                egui::FontId::proportional(14.0),
                theme.title_color,
            );
            ctx.request_repaint();
        }

        remove_idx
    }

    fn draw_drag_row_overlay(
        &mut self,
        ctx: &egui::Context,
        content_rect: egui::Rect,
        list_width: f32,
        theme: &LauncherTheme,
    ) {
        if let Some(anim) = &self.drop_anim {
            let elapsed = anim.start.elapsed();
            let duration = Duration::from_millis(200);
            let t = (elapsed.as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0);
            let eased = ease_out_elastic(t);
            let list_left = content_rect.min.x + CONTENT_PADDING;
            let list_right = list_left + list_width;
            let y = anim.start_y + (anim.end_y - anim.start_y) * eased;
            let r = egui::Rect::from_min_max(
                egui::pos2(list_left, y),
                egui::pos2(list_right, y + ROW_HEIGHT),
            );
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("drag_row"),
            ));
            painter.rect_filled(
                r.expand(DROP_SHADOW),
                12.0 + DROP_SHADOW,
                egui::Color32::from_rgba_premultiplied(0, 0, 0, 32),
            );
            painter.rect_filled(r, 8.0, theme.row_selected);
            painter.rect_stroke(r, 8.0, egui::Stroke::new(1.0, theme.drop_hint));

            let icon_rect = egui::Rect::from_center_size(
                egui::pos2(r.min.x + 14.0 + ICON_SIDE * 0.5, r.center().y),
                egui::vec2(ICON_SIDE, ICON_SIDE),
            );
            if let Some(tex) = &anim.item.texture {
                painter.image(
                    tex.id(),
                    icon_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            } else {
                painter.rect_filled(icon_rect, 5.0, theme.icon_placeholder);
            }
            painter.text(
                egui::pos2(icon_rect.max.x + 9.0, r.center().y),
                egui::Align2::LEFT_CENTER,
                &anim.item.name,
                egui::FontId::proportional(14.0),
                theme.title_color,
            );
            ctx.request_repaint();
        } else if let (Some(from), Some(pos)) =
            (self.dragging_app, ctx.input(|i| i.pointer.hover_pos()))
        {
            let list_left = content_rect.min.x + CONTENT_PADDING;
            let list_right = list_left + list_width;
            let y = pos.y - ROW_HEIGHT * 0.5;
            let r = egui::Rect::from_min_max(
                egui::pos2(list_left, y),
                egui::pos2(list_right, y + ROW_HEIGHT),
            );
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("drag_row"),
            ));
            painter.rect_filled(
                r.expand(DROP_SHADOW),
                12.0 + DROP_SHADOW,
                egui::Color32::from_rgba_premultiplied(0, 0, 0, 32),
            );
            painter.rect_filled(r, 8.0, theme.row_selected);
            painter.rect_stroke(r, 8.0, egui::Stroke::new(1.0, theme.drop_hint));

            let icon_rect = egui::Rect::from_center_size(
                egui::pos2(r.min.x + 14.0 + ICON_SIDE * 0.5, r.center().y),
                egui::vec2(ICON_SIDE, ICON_SIDE),
            );
            if let Some(tex) = &self.pinned_apps[from].texture {
                painter.image(
                    tex.id(),
                    icon_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            } else {
                painter.rect_filled(icon_rect, 5.0, theme.icon_placeholder);
            }
            painter.text(
                egui::pos2(icon_rect.max.x + 9.0, r.center().y),
                egui::Align2::LEFT_CENTER,
                &self.pinned_apps[from].name,
                egui::FontId::proportional(14.0),
                theme.title_color,
            );
            ctx.request_repaint();
        }
    }

    fn draw_flash_overlay(&mut self, ui: &egui::Ui) {
        if let Some(start_time) = self.flash_start_time {
            let elapsed = start_time.elapsed();
            let duration = Duration::from_millis(160);
            if elapsed < duration {
                let progress = elapsed.as_secs_f32() / duration.as_secs_f32();
                let alpha = (1.0 - progress) * 0.35;
                let color = egui::Color32::from_white_alpha((alpha * 255.0) as u8);
                ui.painter().rect_filled(ui.clip_rect(), 0.0, color);
                ui.ctx().request_repaint();
            } else {
                self.flash_start_time = None;
            }
        }
    }

    fn draw_warning_overlay(&mut self, ui: &egui::Ui, theme: &LauncherTheme) {
        if let Some((msg, start_time)) = &self.warning_message {
            let elapsed = start_time.elapsed();
            if elapsed < Duration::from_secs(2) {
                let painter = ui.ctx().layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::new("warning"),
                ));
                let rect = ui.clip_rect();

                let galley = painter.layout(
                    msg.clone(),
                    egui::FontId::proportional(15.0),
                    theme.toast_text,
                    f32::INFINITY,
                );

                let text_rect = galley.rect;
                let centered_rect = text_rect.translate(rect.center() - text_rect.center());
                painter.rect_filled(centered_rect.expand(10.0), 10.0, theme.toast_bg);
                painter.rect_stroke(
                    centered_rect.expand(10.0),
                    10.0,
                    egui::Stroke::new(1.0, theme.row_border),
                );
                painter.galley(centered_rect.min, galley, theme.toast_text);
                ui.ctx().request_repaint();
            } else {
                self.warning_message = None;
            }
        }
    }

    fn draw_fade_in_overlay(&mut self, ui: &egui::Ui, panel_rounding: f32) {
        if let Some(start) = self.fade_in_start {
            let elapsed = start.elapsed();
            let duration = Duration::from_millis(160);
            if elapsed < duration {
                let t = elapsed.as_secs_f32() / duration.as_secs_f32();
                let alpha = ((1.0 - t) * 80.0) as u8;
                ui.painter().rect_stroke(
                    ui.clip_rect().shrink(6.0),
                    panel_rounding,
                    egui::Stroke::new(
                        1.5,
                        egui::Color32::from_rgba_premultiplied(190, 220, 255, alpha),
                    ),
                );
                ui.ctx().request_repaint();
            } else {
                self.fade_in_start = None;
            }
        }
    }
}

fn clamp_window_origin(pos: egui::Pos2, size: egui::Vec2, monitor_size: egui::Vec2) -> egui::Pos2 {
    let min_x = MIN_VISIBLE_WIDTH - size.x;
    let max_x = (monitor_size.x - MIN_VISIBLE_WIDTH).max(min_x);
    let min_y = 0.0;
    let max_y = (monitor_size.y - HEADER_HEIGHT).max(min_y);

    egui::pos2(pos.x.clamp(min_x, max_x), pos.y.clamp(min_y, max_y))
}
fn resize_edge_cursor(edge: ResizeEdge) -> egui::CursorIcon {
    match edge {
        ResizeEdge::Left | ResizeEdge::Right => egui::CursorIcon::ResizeHorizontal,
        ResizeEdge::Bottom => egui::CursorIcon::ResizeVertical,
        ResizeEdge::BottomLeft => egui::CursorIcon::ResizeNeSw,
        ResizeEdge::BottomRight => egui::CursorIcon::ResizeNwSe,
    }
}

fn resize_edge_key(edge: ResizeEdge) -> &'static str {
    match edge {
        ResizeEdge::Left => "left",
        ResizeEdge::Right => "right",
        ResizeEdge::Bottom => "bottom",
        ResizeEdge::BottomLeft => "bottom_left",
        ResizeEdge::BottomRight => "bottom_right",
    }
}

fn style_compact_menu(ui: &mut egui::Ui) {
    let visuals = ui.visuals_mut();
    visuals.window_fill = egui::Color32::from_rgba_premultiplied(246, 248, 252, 252);
    visuals.panel_fill = egui::Color32::from_rgba_premultiplied(246, 248, 252, 252);
    visuals.extreme_bg_color = egui::Color32::from_rgba_premultiplied(238, 243, 250, 255);
    visuals.widgets.noninteractive.bg_fill = egui::Color32::TRANSPARENT;
    visuals.widgets.noninteractive.fg_stroke.color = egui::Color32::from_rgb(10, 16, 24);
    visuals.widgets.inactive.bg_fill = egui::Color32::TRANSPARENT;
    visuals.widgets.inactive.fg_stroke.color = egui::Color32::from_rgb(10, 16, 24);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgba_premultiplied(205, 225, 242, 230);
    visuals.widgets.hovered.fg_stroke.color = egui::Color32::from_rgb(5, 10, 18);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgba_premultiplied(188, 214, 235, 245);
    visuals.widgets.active.fg_stroke.color = egui::Color32::from_rgb(5, 10, 18);
    visuals.window_stroke.color = egui::Color32::from_rgba_premultiplied(126, 146, 171, 210);
    visuals.popup_shadow.color = egui::Color32::from_rgba_premultiplied(0, 0, 0, 42);

    let style = ui.style_mut();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 7.0);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AddPinResult {
    Added,
    Duplicate,
    Unsupported,
    ShortcutUnresolved,
    Missing,
    LimitReached,
}

fn is_supported_app_path(path: &Path) -> bool {
    if path.is_dir() {
        return true;
    }
    path.extension()
        .and_then(|s| s.to_str())
        .map(|ext| matches!(ext.to_ascii_lowercase().as_str(), "exe" | "lnk"))
        .unwrap_or(false)
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

fn resolve_two_column_indices(
    apps: &[PinnedApp],
    layout: Option<&TwoColumnLayout>,
) -> (Vec<usize>, Vec<usize>) {
    if apps.is_empty() {
        return (Vec::new(), Vec::new());
    }

    let keys: Vec<String> = apps
        .iter()
        .map(|app| {
            normalize_launch_key(
                &app.path,
                app.launch_args.as_deref(),
                app.working_dir.as_deref(),
            )
        })
        .collect();

    let mut used = vec![false; apps.len()];
    let mut left = Vec::with_capacity(apps.len());
    let mut right = Vec::with_capacity(apps.len());

    if let Some(layout) = layout {
        for entry in &layout.left {
            if let Some(idx) = find_unused_index_by_key(&keys, entry.key().as_str(), &used) {
                used[idx] = true;
                left.push(idx);
            }
        }
        for entry in &layout.right {
            if let Some(idx) = find_unused_index_by_key(&keys, entry.key().as_str(), &used) {
                used[idx] = true;
                right.push(idx);
            }
        }
    }

    if left.is_empty() && right.is_empty() {
        for idx in 0..apps.len() {
            if idx % 2 == 0 {
                left.push(idx);
            } else {
                right.push(idx);
            }
        }
        return (left, right);
    }

    for idx in 0..apps.len() {
        if !used[idx] {
            left.push(idx);
        }
    }

    (left, right)
}

fn find_unused_index_by_key(keys: &[String], target: &str, used: &[bool]) -> Option<usize> {
    keys.iter()
        .enumerate()
        .find(|(idx, key)| !used[*idx] && key.as_str() == target)
        .map(|(idx, _)| idx)
}

fn find_column_slot(index: usize, left: &[usize], right: &[usize]) -> Option<(usize, usize)> {
    if let Some(pos) = left.iter().position(|&idx| idx == index) {
        return Some((0, pos));
    }
    right
        .iter()
        .position(|&idx| idx == index)
        .map(|pos| (1, pos))
}

fn slot_from_pointer(pointer_y: f32, rects: &[egui::Rect]) -> usize {
    for (slot, rect) in rects.iter().enumerate() {
        if pointer_y < rect.center().y {
            return slot;
        }
    }
    rects.len()
}

fn reorder_pinned_apps_by_columns(apps: &mut Vec<PinnedApp>, left: &[usize], right: &[usize]) {
    let total = apps.len();
    if total == 0 {
        return;
    }

    let mut order = Vec::with_capacity(total);
    order.extend(left.iter().copied());
    order.extend(right.iter().copied());

    if order.len() != total {
        return;
    }

    let mut seen = vec![false; total];
    for &idx in &order {
        if idx >= total || seen[idx] {
            return;
        }
        seen[idx] = true;
    }

    let mut source: Vec<Option<PinnedApp>> = std::mem::take(apps).into_iter().map(Some).collect();
    let mut reordered = Vec::with_capacity(total);
    for idx in order {
        if let Some(item) = source[idx].take() {
            reordered.push(item);
        }
    }

    if reordered.len() == total {
        *apps = reordered;
    }
}

fn two_column_layout_from_split(apps: &[PinnedApp], left_len: usize) -> TwoColumnLayout {
    let split = left_len.min(apps.len());
    let left = apps
        .iter()
        .take(split)
        .map(two_column_entry_from_app)
        .collect();
    let right = apps
        .iter()
        .skip(split)
        .map(two_column_entry_from_app)
        .collect();
    TwoColumnLayout { left, right }
}

fn two_column_entry_from_app(app: &PinnedApp) -> TwoColumnEntry {
    TwoColumnEntry::from_launch(
        app.path.clone(),
        app.launch_args.clone(),
        app.working_dir.clone(),
    )
}

fn paint_glow_blob(painter: &egui::Painter, center: egui::Pos2, radius: f32, color: egui::Color32) {
    painter.circle_filled(center, radius, color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_app(name: &str) -> PinnedApp {
        PinnedApp::new(
            PathBuf::from(format!(r"C:\\Apps\\{name}.exe")),
            Some(name.to_string()),
            None,
            None,
        )
    }

    fn make_entry(name: &str) -> TwoColumnEntry {
        TwoColumnEntry::from_launch(PathBuf::from(format!(r"C:\\Apps\\{name}.exe")), None, None)
    }

    fn names(apps: &[PinnedApp]) -> Vec<String> {
        apps.iter().map(|app| app.name.clone()).collect()
    }

    #[test]
    fn two_column_layout_restores_saved_right_column() {
        let mut apps = vec![make_app("A"), make_app("B"), make_app("C"), make_app("D")];
        let saved_layout = TwoColumnLayout {
            left: vec![make_entry("A"), make_entry("C")],
            right: vec![make_entry("B"), make_entry("D")],
        };

        let (left, right) = resolve_two_column_indices(&apps, Some(&saved_layout));
        assert_eq!(left, vec![0, 2]);
        assert_eq!(right, vec![1, 3]);

        reorder_pinned_apps_by_columns(&mut apps, &left, &right);
        assert_eq!(names(&apps), vec!["A", "C", "B", "D"]);

        let (left_again, right_again) = resolve_two_column_indices(&apps, Some(&saved_layout));
        assert_eq!(left_again, vec![0, 1]);
        assert_eq!(right_again, vec![2, 3]);
    }

    #[test]
    fn unseen_apps_are_appended_to_left_column() {
        let apps = vec![
            make_app("A"),
            make_app("C"),
            make_app("B"),
            make_app("D"),
            make_app("E"),
        ];
        let saved_layout = TwoColumnLayout {
            left: vec![make_entry("A"), make_entry("C")],
            right: vec![make_entry("B"), make_entry("D")],
        };

        let (left, right) = resolve_two_column_indices(&apps, Some(&saved_layout));
        assert_eq!(left, vec![0, 1, 4]);
        assert_eq!(right, vec![2, 3]);
    }
}
