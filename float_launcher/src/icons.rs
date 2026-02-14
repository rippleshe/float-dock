use eframe::egui;
use std::collections::HashSet;
use std::io::{Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use tray_icon::Icon;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
};
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, ICONINFO};

pub fn extract_icon_from_exe(path: &Path) -> Option<egui::ColorImage> {
    unsafe {
        let mut sh_file_info = SHFILEINFOW::default();
        let path_wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let result = SHGetFileInfoW(
            windows::core::PCWSTR(path_wide.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut sh_file_info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );

        if result == 0 {
            return None;
        }

        let hicon = sh_file_info.hIcon;
        if hicon.is_invalid() {
            return None;
        }

        let mut icon_info = ICONINFO::default();
        if GetIconInfo(hicon, &mut icon_info).is_err() {
            let _ = DestroyIcon(hicon);
            return None;
        }

        let mut bitmap: BITMAP = std::mem::zeroed();
        if GetObjectW(
            HGDIOBJ(icon_info.hbmColor.0),
            std::mem::size_of::<BITMAP>() as i32,
            Some(&mut bitmap as *mut _ as *mut _),
        ) == 0
        {
            let _ = DeleteObject(icon_info.hbmColor);
            let _ = DeleteObject(icon_info.hbmMask);
            let _ = DestroyIcon(hicon);
            return None;
        }

        let width = bitmap.bmWidth as usize;
        let height = bitmap.bmHeight as usize;

        let hdc = CreateCompatibleDC(None);
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut pixels: Vec<u8> = vec![0; width * height * 4];

        let result = GetDIBits(
            hdc,
            icon_info.hbmColor,
            0,
            height as u32,
            Some(pixels.as_mut_ptr() as *mut _),
            &mut bmi,
            DIB_RGB_COLORS,
        );

        let _ = DeleteDC(hdc);
        let _ = DeleteObject(icon_info.hbmColor);
        let _ = DeleteObject(icon_info.hbmMask);
        let _ = DestroyIcon(hicon);

        if result == 0 {
            return None;
        }

        for chunk in pixels.chunks_exact_mut(4) {
            let b = chunk[0];
            let g = chunk[1];
            let r = chunk[2];
            let a = chunk[3];
            chunk[0] = r;
            chunk[1] = g;
            chunk[2] = b;
            chunk[3] = a;
        }

        Some(egui::ColorImage::from_rgba_unmultiplied(
            [width, height],
            &pixels,
        ))
    }
}

fn find_brand_icon_file() -> Option<PathBuf> {
    let names = ["app.ico", "favicon.ico"];
    for dir in icon_override_dirs() {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn color_image_to_tray_icon(image: &egui::ColorImage) -> Option<Icon> {
    let width = image.size[0] as u32;
    let height = image.size[1] as u32;
    Icon::from_rgba(image.as_raw().to_vec(), width, height).ok()
}

pub fn load_tray_icon_for_app(side: usize) -> Option<Icon> {
    let side = side.clamp(16, 256);

    if let Ok(exe) = std::env::current_exe() {
        if let Some(img) = extract_icon_from_exe(&exe) {
            let sized = resize_to_square(&img, side);
            if let Some(icon) = color_image_to_tray_icon(&sized) {
                return Some(icon);
            }
        }
    }

    let brand_path = find_brand_icon_file()?;
    let img = extract_icon_from_exe(&brand_path)?;
    let sized = resize_to_square(&img, side);
    color_image_to_tray_icon(&sized)
}

fn stable_hash64(input: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in input {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn icon_cache_path_at(config_dir: &Path, source_path: &Path) -> std::path::PathBuf {
    let icons_dir = config_dir.join("icons");
    let key = stable_hash64(source_path.to_string_lossy().as_bytes());
    icons_dir.join(format!("{:016x}.rgba", key))
}

pub fn load_cached_icon(source_path: &Path) -> Option<egui::ColorImage> {
    let config_dir = crate::config::AppConfig::config_dir()?;
    load_cached_icon_at(&config_dir, source_path)
}

fn load_cached_icon_at(config_dir: &Path, source_path: &Path) -> Option<egui::ColorImage> {
    let cache_path = icon_cache_path_at(config_dir, source_path);
    let mut file = std::fs::File::open(cache_path).ok()?;

    let mut header = [0u8; 16];
    file.read_exact(&mut header).ok()?;
    if &header[0..4] != b"FLI2" {
        return None;
    }
    let width = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
    let height = u32::from_le_bytes([header[8], header[9], header[10], header[11]]) as usize;
    let len = u32::from_le_bytes([header[12], header[13], header[14], header[15]]) as usize;
    if len != width.saturating_mul(height).saturating_mul(4) {
        return None;
    }

    let mut pixels = vec![0u8; len];
    file.read_exact(&mut pixels).ok()?;
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [width, height],
        &pixels,
    ))
}

pub fn save_cached_icon(source_path: &Path, image: &egui::ColorImage) {
    let Some(config_dir) = crate::config::AppConfig::config_dir() else {
        return;
    };
    save_cached_icon_at(&config_dir, source_path, image);
}

fn save_cached_icon_at(config_dir: &Path, source_path: &Path, image: &egui::ColorImage) {
    let cache_path = icon_cache_path_at(config_dir, source_path);
    if let Some(parent) = cache_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let width = image.size[0] as u32;
    let height = image.size[1] as u32;
    let rgba = image.as_raw();
    let len = rgba.len() as u32;

    let mut file = match std::fs::File::create(cache_path) {
        Ok(f) => f,
        Err(_) => return,
    };

    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(b"FLI2");
    out[4..8].copy_from_slice(&width.to_le_bytes());
    out[8..12].copy_from_slice(&height.to_le_bytes());
    out[12..16].copy_from_slice(&len.to_le_bytes());
    let _ = file.write_all(&out);
    let _ = file.write_all(rgba);
}

fn icon_override_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut seen = HashSet::new();

    let mut push_dir = |dir: PathBuf| {
        if dir.is_dir() {
            let key = dir.to_string_lossy().to_ascii_lowercase();
            if seen.insert(key) {
                dirs.push(dir);
            }
        }
    };

    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            push_dir(exe_dir.join("ico"));
            if let Some(parent) = exe_dir.parent() {
                push_dir(parent.join("float_launcher").join("ico"));
                push_dir(parent.join("float_dock").join("ico"));
            }
        }
    }

    push_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ico"));
    dirs
}

fn normalize_icon_name_key(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lowered = trimmed.to_ascii_lowercase();
    let key = lowered
        .strip_suffix(".ico")
        .unwrap_or(lowered.as_str())
        .trim()
        .to_string();
    if key.is_empty() {
        None
    } else {
        Some(key)
    }
}

fn find_named_custom_icon(source_path: &Path, name_hint: Option<&str>) -> Option<PathBuf> {
    let mut keys: Vec<String> = Vec::new();
    if let Some(hint) = name_hint.and_then(normalize_icon_name_key) {
        keys.push(hint);
    }
    if let Some(stem) = source_path
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(normalize_icon_name_key)
    {
        if !keys.iter().any(|k| k == &stem) {
            keys.push(stem);
        }
    }
    if keys.is_empty() {
        return None;
    }

    for dir in icon_override_dirs() {
        for key in &keys {
            let direct = dir.join(format!("{key}.ico"));
            if direct.is_file() {
                return Some(direct);
            }
        }

        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                let is_ico = p
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case("ico"))
                    .unwrap_or(false);
                if !is_ico {
                    continue;
                }
                let Some(stem) = p
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(normalize_icon_name_key)
                else {
                    continue;
                };
                if keys.iter().any(|k| k == &stem) {
                    return Some(p);
                }
            }
        }
    }

    None
}

fn find_generic_custom_icon() -> Option<PathBuf> {
    let generic_names = ["default.ico", "app.ico", "favicon.ico"];
    for dir in icon_override_dirs() {
        for name in generic_names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn load_custom_icon_with_cache(icon_path: &Path) -> Option<egui::ColorImage> {
    if let Some(img) = load_cached_icon(icon_path) {
        return Some(img);
    }
    let img = extract_icon_from_exe(icon_path)?;
    save_cached_icon(icon_path, &img);
    Some(img)
}

pub fn extract_icon_with_cache(
    source_path: &Path,
    name_hint: Option<&str>,
) -> Option<egui::ColorImage> {
    if let Some(custom_icon) = find_named_custom_icon(source_path, name_hint) {
        if let Some(img) = load_custom_icon_with_cache(&custom_icon) {
            return Some(img);
        }
    }

    if let Some(img) = load_cached_icon(source_path) {
        return Some(img);
    }

    let icon_source = crate::system::resolve_shortcut_target(source_path)
        .filter(|p| p.exists())
        .unwrap_or_else(|| source_path.to_path_buf());
    if let Some(img) = extract_icon_from_exe(&icon_source) {
        save_cached_icon(source_path, &img);
        return Some(img);
    }

    if let Some(custom_fallback) = find_generic_custom_icon() {
        return load_custom_icon_with_cache(&custom_fallback);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_cache_roundtrip_50() {
        let base = std::env::temp_dir().join(format!(
            "float_launcher_icon_cache_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        for i in 0..50u32 {
            let source = std::path::PathBuf::from(format!(r"C:\fake\app_{i}.exe"));
            let side = 64usize;
            let pixels = vec![(i % 255) as u8; side * side * 4];
            let img = egui::ColorImage::from_rgba_unmultiplied([side, side], &pixels);
            save_cached_icon_at(&base, &source, &img);
            let loaded = load_cached_icon_at(&base, &source).expect("missing cached icon");
            assert_eq!(loaded.size, [side, side]);
            assert_eq!(loaded.as_raw().len(), side * side * 4);
        }
    }
}

pub fn resize_to_square(image: &egui::ColorImage, side: usize) -> egui::ColorImage {
    let src_w = image.size[0];
    let src_h = image.size[1];
    if src_w == side && src_h == side {
        return image.clone();
    }
    let src = image.as_raw();
    let mut out = vec![0u8; side * side * 4];
    for y in 0..side {
        let sy = y * src_h / side;
        for x in 0..side {
            let sx = x * src_w / side;
            let si = (sy * src_w + sx) * 4;
            let di = (y * side + x) * 4;
            out[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([side, side], &out)
}

pub fn generate_colored_icon(color: [u8; 4]) -> Icon {
    let width = 32;
    let height = 32;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..height {
        for _ in 0..width {
            rgba.extend_from_slice(&color);
        }
    }
    Icon::from_rgba(rgba, width, height).expect("Failed to create icon")
}
