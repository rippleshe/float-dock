# Float Dock 🚀

一个面向 Windows 的轻量级浮动启动栏（Rust + egui），主打低占用、快速唤起、拖拽即用。

## 📌 项目定位

- 🎯 **目标体验**：像 macOS Finder/Dock 一样快速启动常用应用，但更轻量。
- ⚡ **性能优先**：低内存、低 CPU、快速冷启动。
- 🪟 **Windows 友好**：托盘常驻、全局热键、开机自启、透明浮层风格。

## ✨ 核心功能

- 🧲 **拖拽固定**：支持拖入 `.exe` / `.lnk` / 文件夹。
- 🔗 **快捷方式解析**：拖入 `.lnk` 时自动解析目标路径（保留启动参数与工作目录）。
- 🖱️ **快速启动**：双击条目即可启动。
- 🧠 **图标缓存**：应用图标提取后缓存，减少重复开销。
- 🎨 **自定义图标覆盖**：支持 `ico` 目录自定义 `.ico`。
- 🧱 **窗口可缩放并记忆**：支持边缘/底角拉伸，重启后恢复上次位置与尺寸。
- 📚 **双列模式**：支持双列展示。
- 🔄 **双列排序记忆**：双列下可长按拖拽跨列排序；切单列后按“第一列+第二列”合并，再切回双列可恢复原第二列归属。
- 🧰 **极简右键菜单**：仅保留 `Auto-start`、`Two-column mode`、`Quit`。

## ⌨️ 全局快捷键

- `Ctrl + Alt + Shift + [`：显示并聚焦窗口
- `Ctrl + Alt + Shift + ]`：隐藏窗口（进程继续驻留）
- `Ctrl + Alt + Shift + \`：退出程序

兼容兜底：`Ctrl + Alt + Shift + F9/F10/F11`。

## 🧭 使用流程

1. 运行 `float_dock.exe`。
2. 把 `.exe` / `.lnk` / 文件夹拖进窗口完成固定。
3. 双击条目启动目标。
4. 右键条目可直接 `Remove`（立即删除，无确认弹窗）。
5. 右键面板可切换自启动、双列模式或退出。

## 🖼️ 图标说明

- 应用图标覆盖：
  把 `应用名.ico` 放到 `float_launcher/ico`（或与 exe 同级 `ico` 目录）。
- 兜底图标：
  可放置 `favicon.ico` 作为未命中时的默认图标。
- 品牌图标：
  `ico/app.ico` 用于生成的 exe 图标与托盘图标。

## 🔧 构建

```bash
cd float_launcher
cargo build --release
```

构建产物：`float_launcher/target/release/float_dock.exe`

## 📁 目录结构

```text
FloatDock/
├─ PRD/
├─ dist/
├─ float_launcher/
│  ├─ ico/
│  ├─ src/
│  └─ Cargo.toml
└─ README.md
```
