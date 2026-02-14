use crate::branding::APP_DISPLAY_NAME;
use crate::events::{IconRequest, IconResult, UserEvent};
use crate::icons::{
    extract_icon_with_cache, generate_colored_icon, load_tray_icon_for_app, resize_to_square,
};
use crossbeam_channel::TryRecvError;
use eframe::egui;
use log::{error, info};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem},
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
    MOD_SHIFT, VIRTUAL_KEY, VK_CONTROL, VK_F10, VK_F11, VK_F9, VK_MENU, VK_OEM_4, VK_OEM_5,
    VK_OEM_6, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetMessageW, PeekMessageW, MSG, PM_NOREMOVE, WM_HOTKEY,
};

pub const HOTKEY_SHOW: &str = "Ctrl+Alt+Shift+[";
pub const HOTKEY_HIDE: &str = "Ctrl+Alt+Shift+]";
pub const HOTKEY_QUIT: &str = "Ctrl+Alt+Shift+\\";

const HOTKEY_SHOW_FALLBACK: &str = "Ctrl+Alt+Shift+F9";
const HOTKEY_HIDE_FALLBACK: &str = "Ctrl+Alt+Shift+F10";
const HOTKEY_QUIT_FALLBACK: &str = "Ctrl+Alt+Shift+F11";

const HOTKEY_ID_SHOW: i32 = 1001;
const HOTKEY_ID_HIDE: i32 = 1002;
const HOTKEY_ID_QUIT: i32 = 1003;
const HOTKEY_ID_SHOW_FALLBACK: i32 = 1101;
const HOTKEY_ID_HIDE_FALLBACK: i32 = 1102;
const HOTKEY_ID_QUIT_FALLBACK: i32 = 1103;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeAction {
    Show,
    Hide,
    Toggle,
    Quit,
}

#[derive(Clone, Copy)]
struct HotkeyBinding {
    id: i32,
    vk: u32,
    action: RuntimeAction,
    label: &'static str,
}

const HOTKEY_BINDINGS: [HotkeyBinding; 6] = [
    HotkeyBinding {
        id: HOTKEY_ID_SHOW,
        vk: VK_OEM_4.0 as u32,
        action: RuntimeAction::Show,
        label: HOTKEY_SHOW,
    },
    HotkeyBinding {
        id: HOTKEY_ID_HIDE,
        vk: VK_OEM_6.0 as u32,
        action: RuntimeAction::Hide,
        label: HOTKEY_HIDE,
    },
    HotkeyBinding {
        id: HOTKEY_ID_QUIT,
        vk: VK_OEM_5.0 as u32,
        action: RuntimeAction::Quit,
        label: HOTKEY_QUIT,
    },
    HotkeyBinding {
        id: HOTKEY_ID_SHOW_FALLBACK,
        vk: VK_F9.0 as u32,
        action: RuntimeAction::Show,
        label: HOTKEY_SHOW_FALLBACK,
    },
    HotkeyBinding {
        id: HOTKEY_ID_HIDE_FALLBACK,
        vk: VK_F10.0 as u32,
        action: RuntimeAction::Hide,
        label: HOTKEY_HIDE_FALLBACK,
    },
    HotkeyBinding {
        id: HOTKEY_ID_QUIT_FALLBACK,
        vk: VK_F11.0 as u32,
        action: RuntimeAction::Quit,
        label: HOTKEY_QUIT_FALLBACK,
    },
];

pub struct RuntimeHandles {
    pub tray_icon: TrayIcon,
    pub rx: Receiver<UserEvent>,
    pub icon_req_tx: Sender<IconRequest>,
    pub toggle_item: MenuItem,
    pub icon_awake: Icon,
    pub icon_sleep: Icon,
}

pub fn build_runtime(ctx: &egui::Context) -> RuntimeHandles {
    let (icon_req_tx, icon_req_rx) = mpsc::channel::<IconRequest>();
    let (ui_tx, ui_rx) = mpsc::channel::<UserEvent>();
    let (action_tx, action_rx) = mpsc::channel::<RuntimeAction>();

    spawn_icon_worker(icon_req_rx, ui_tx.clone(), ctx.clone());

    let base_icon =
        load_tray_icon_for_app(32).unwrap_or_else(|| generate_colored_icon([45, 190, 150, 255]));
    let icon_awake = base_icon.clone();
    let icon_sleep = base_icon;

    let tray_menu = Menu::new();
    let toggle_item = MenuItem::new("Hide", true, None);
    let quit_item = MenuItem::new("Quit", true, None);
    tray_menu
        .append_items(&[&toggle_item, &quit_item])
        .expect("failed to append tray menu");

    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip(APP_DISPLAY_NAME)
        .with_icon(icon_awake.clone())
        .build()
        .expect("failed to create tray icon");

    let toggle_id = toggle_item.id().clone();
    let quit_id = quit_item.id().clone();

    spawn_native_hotkey_worker(action_tx.clone());
    spawn_hotkey_polling_fallback(action_tx);
    spawn_runtime_event_loop(ui_tx, action_rx, ctx.clone(), toggle_id, quit_id);

    RuntimeHandles {
        tray_icon,
        rx: ui_rx,
        icon_req_tx,
        toggle_item,
        icon_awake,
        icon_sleep,
    }
}

fn spawn_icon_worker(
    icon_req_rx: Receiver<IconRequest>,
    tx: Sender<UserEvent>,
    ctx: egui::Context,
) {
    thread::spawn(move || {
        let com_initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() };
        while let Ok(req) = icon_req_rx.recv() {
            let side = req.size.clamp(16, 256) as usize;
            let image = extract_icon_with_cache(&req.path, req.name_hint.as_deref())
                .map(|img| resize_to_square(&img, side));
            let _ = tx.send(UserEvent::IconReady(IconResult {
                path: req.path,
                image,
            }));
            ctx.request_repaint();
        }
        if com_initialized {
            unsafe { CoUninitialize() };
        }
    });
}

fn spawn_native_hotkey_worker(action_tx: Sender<RuntimeAction>) {
    thread::spawn(move || unsafe {
        let mut init_msg = MSG::default();
        let _ = PeekMessageW(&mut init_msg, None, 0, 0, PM_NOREMOVE);

        let mods = MOD_ALT | MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT;
        let mut registered_count = 0usize;
        for binding in HOTKEY_BINDINGS {
            if let Err(err) = RegisterHotKey(None, binding.id, mods, binding.vk) {
                error!(
                    "failed to register native hotkey {}: {}",
                    binding.label, err
                );
            } else {
                registered_count += 1;
                info!("registered native hotkey {}", binding.label);
            }
        }
        if registered_count == 0 {
            error!("no native hotkeys registered; fallback polling remains active");
        }

        let mut msg = MSG::default();
        loop {
            let status = GetMessageW(&mut msg, None, 0, 0).0;
            if status == -1 {
                error!("GetMessageW failed in hotkey worker");
                break;
            }
            if status == 0 {
                break;
            }
            if msg.message == WM_HOTKEY {
                let hotkey_id = msg.wParam.0 as i32;
                let action = HOTKEY_BINDINGS
                    .iter()
                    .find(|binding| binding.id == hotkey_id)
                    .map(|binding| binding.action);
                if let Some(action) = action {
                    let _ = action_tx.send(action);
                }
            }
        }

        for binding in HOTKEY_BINDINGS {
            let _ = UnregisterHotKey(None, binding.id);
        }
    });
}

fn spawn_hotkey_polling_fallback(action_tx: Sender<RuntimeAction>) {
    thread::spawn(move || unsafe {
        let mut prev_show = false;
        let mut prev_hide = false;
        let mut prev_quit = false;

        loop {
            let key_down = |vk: VIRTUAL_KEY| (GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000) != 0;
            let alt_down = key_down(VK_MENU);
            let ctrl_down = key_down(VK_CONTROL);
            let shift_down = key_down(VK_SHIFT);
            let chord_down = alt_down && ctrl_down && shift_down;

            let show_down = chord_down && (key_down(VK_OEM_4) || key_down(VK_F9));
            let hide_down = chord_down && (key_down(VK_OEM_6) || key_down(VK_F10));
            let quit_down = chord_down && (key_down(VK_OEM_5) || key_down(VK_F11));

            if show_down && !prev_show {
                let _ = action_tx.send(RuntimeAction::Show);
            }
            if hide_down && !prev_hide {
                let _ = action_tx.send(RuntimeAction::Hide);
            }
            if quit_down && !prev_quit {
                let _ = action_tx.send(RuntimeAction::Quit);
            }

            prev_show = show_down;
            prev_hide = hide_down;
            prev_quit = quit_down;

            thread::sleep(Duration::from_millis(20));
        }
    });
}

fn spawn_runtime_event_loop(
    ui_tx: Sender<UserEvent>,
    action_rx: Receiver<RuntimeAction>,
    ctx: egui::Context,
    toggle_menu_id: tray_icon::menu::MenuId,
    quit_menu_id: tray_icon::menu::MenuId,
) {
    thread::spawn(move || {
        let mut is_visible = true;
        loop {
            while let Ok(action) = action_rx.try_recv() {
                apply_runtime_action(action, &ui_tx, &ctx, &mut is_visible);
            }

            match MenuEvent::receiver().try_recv() {
                Ok(event) => {
                    if event.id == toggle_menu_id {
                        apply_runtime_action(RuntimeAction::Toggle, &ui_tx, &ctx, &mut is_visible);
                    } else if event.id == quit_menu_id {
                        apply_runtime_action(RuntimeAction::Quit, &ui_tx, &ctx, &mut is_visible);
                    }
                }
                Err(err) => {
                    if !matches!(err, TryRecvError::Empty) {
                        error!("menu receiver error: {}", err);
                    }
                }
            }

            match TrayIconEvent::receiver().try_recv() {
                Ok(event) => {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        apply_runtime_action(RuntimeAction::Toggle, &ui_tx, &ctx, &mut is_visible);
                    }
                }
                Err(err) => {
                    if !matches!(err, TryRecvError::Empty) {
                        error!("tray receiver error: {}", err);
                    }
                }
            }

            thread::sleep(Duration::from_millis(10));
        }
    });
}

fn apply_runtime_action(
    action: RuntimeAction,
    ui_tx: &Sender<UserEvent>,
    ctx: &egui::Context,
    is_visible: &mut bool,
) {
    match action {
        RuntimeAction::Show => {
            *is_visible = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            let _ = ui_tx.send(UserEvent::Show);
            ctx.request_repaint();
        }
        RuntimeAction::Hide => {
            if *is_visible {
                *is_visible = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                let _ = ui_tx.send(UserEvent::Hide);
                ctx.request_repaint();
            }
        }
        RuntimeAction::Toggle => {
            if *is_visible {
                apply_runtime_action(RuntimeAction::Hide, ui_tx, ctx, is_visible);
            } else {
                apply_runtime_action(RuntimeAction::Show, ui_tx, ctx, is_visible);
            }
        }
        RuntimeAction::Quit => {
            let _ = ui_tx.send(UserEvent::Quit);
            std::process::exit(0);
        }
    }
}
