# Float Dock

A lightweight, high-performance floating application launcher written in Rust using egui.

## Features

- **Ultra-lightweight**: Minimized resource usage.
- **Floating Window**: Always on top, transparent background.
- **Edge Resize + Memory**: Drag left/right/bottom edges (and bottom corners) to resize; last position and size are restored on next launch.
- **Screen-safe Dragging**: The top drag bar is clamped to stay visible so the window can always be moved back.
- **Drag & Drop**: Pin `.exe` / `.lnk` / folders by dropping onto the window.
- **Quick Access**: `Ctrl+Alt+Shift+[` show, `Ctrl+Alt+Shift+]` hide, `Ctrl+Alt+Shift+\` quit.
- **Minimal Context Menu**: `Auto-start`, `Two-column mode`, `Quit`.
- **Two-column Layout Memory**: In two-column mode, long-press drag can reorder across columns; switching to single-column flattens as left-then-right, switching back restores previous second-column membership.
- **Custom Icons**: Place `.ico` files in `float_launcher/ico` (or next to exe in `ico`) to override app icons.
- **Brand Icon**: `ico/app.ico` is used for both built `.exe` icon and tray icon.

## Build Instructions

To build the application for release:

```bash
cd float_launcher
cargo build --release
```

The executable will be located in `float_launcher/target/release/float_dock.exe`.

## How to Use

1.  **Launch**: Run the executable.
2.  **Toggle Visibility**: `Ctrl+Alt+Shift+[` show, `Ctrl+Alt+Shift+]` hide, `Ctrl+Alt+Shift+\` quit.
3.  **Pin Items**: Drag and drop `.exe` / `.lnk` / folders onto the launcher window (`.lnk` is auto-resolved to target path, preserving launch args/working dir).
4.  **Custom Icon Override**: Put `AppName.ico` in `float_launcher/ico` (or `ico` next to exe) to override that app's icon. `favicon.ico` can be used as fallback.
5.  **App/Tray Brand Icon**: Put `app.ico` in `float_launcher/ico` (or `ico` next to exe) to set both `.exe` file icon and tray icon.
6.  **Launch Apps**: Click on the pinned app icons.
7.  **Context Menu**: Right-click on the launcher background to:
    *   Enable/Disable **Auto-start**.
    *   Enable/Disable **Two-column mode** (supports cross-column long-press reorder with layout memory).
    *   Quit.
8.  **Remove Apps**: Right-click on a pinned app icon and select "Remove" (immediate delete, no confirmation dialog).




