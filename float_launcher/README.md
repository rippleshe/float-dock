# Float Dock

## 简介
Windows 轻量级浮动应用启动器（Rust + egui）。

## 关键热键
- `Ctrl+Alt+Shift+[`: 唤起并聚焦
- `Ctrl+Alt+Shift+]`: 隐藏窗口（进程继续驻留）
- `Ctrl+Alt+Shift+\`: 终止程序

说明：热键由 Windows 原生 `RegisterHotKey` 驱动，并保留 `Ctrl+Alt+Shift+F9/F10/F11` 兼容兜底，隐藏状态下可唤起/终止。

## 功能
- 拖拽固定 `.exe` / `.lnk` / 文件夹（`.lnk` 会自动解析为目标路径，并保留启动参数/工作目录）
- 双击启动、右键移除、长按排序
- 图标提取与本地缓存（`.lnk` 自动解析目标程序图标）
- 支持自定义图标覆盖：在 `ico` 目录放置 `应用名.ico`，可覆盖对应条目图标（`favicon.ico` 可作兜底）
- `ico/app.ico` 用作应用品牌图标（构建后 `.exe` 文件图标 + 托盘图标统一）
- 极简右键菜单（Auto-start / Two-column mode / Quit）
- 双列模式支持长按跨列拖拽排序；切回单列按“第一列在前、第二列在后”合并；再次切回双列可恢复原第二列归属
- 托盘菜单 + 开机自启
- 支持窗口左右/底部边缘与底角拖拽缩放，并记忆上次位置与尺寸（下次启动自动恢复）
- 拖动窗口时自动限制顶部不移出屏幕，避免标题栏丢失后无法拖回
- 自动加载 Windows 字体回退（如微软雅黑），避免中文标题缺字

## 编译
```bash
cargo build --release
```

可执行文件路径：`target/release/float_dock.exe`





