use crate::branding::{APP_AUTOSTART_VALUE, LEGACY_AUTOSTART_VALUE};
use std::path::{Path, PathBuf};
use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED, STGM_READ,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_SZ,
};
use windows::Win32::UI::Shell::{
    IShellLinkW, ShellExecuteW, ShellLink, SLGP_RAWPATH, SLR_ANY_MATCH, SLR_NO_UI,
};

use std::os::windows::ffi::OsStrExt;
use windows::Win32::UI::WindowsAndMessaging::SHOW_WINDOW_CMD;

#[derive(Debug, Clone)]
pub struct ShortcutResolution {
    pub target_path: PathBuf,
    pub arguments: Option<String>,
    pub working_dir: Option<PathBuf>,
}

fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn reg_has_value(hkey: HKEY, value_name: &str) -> bool {
    let name_wide = to_wide(value_name);
    RegQueryValueExW(hkey, PCWSTR(name_wide.as_ptr()), None, None, None, None)
        .ok()
        .is_ok()
}

unsafe fn reg_delete_value(hkey: HKEY, value_name: &str) {
    let name_wide = to_wide(value_name);
    let _ = RegDeleteValueW(hkey, PCWSTR(name_wide.as_ptr()));
}

pub fn get_auto_start_status() -> bool {
    unsafe {
        let run_key = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
        let mut hkey = HKEY::default();
        let run_key_wide = to_wide(run_key);

        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(run_key_wide.as_ptr()),
            0,
            KEY_READ,
            &mut hkey,
        )
        .ok()
        .is_ok()
        {
            let result = reg_has_value(hkey, APP_AUTOSTART_VALUE)
                || reg_has_value(hkey, LEGACY_AUTOSTART_VALUE);
            let _ = RegCloseKey(hkey);
            return result;
        }
    }
    false
}

pub fn set_auto_start(enabled: bool) -> windows::core::Result<()> {
    unsafe {
        let run_key = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
        let mut hkey = HKEY::default();
        let run_key_wide = to_wide(run_key);

        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(run_key_wide.as_ptr()),
            0,
            KEY_SET_VALUE,
            &mut hkey,
        )
        .ok()?;
        let app_name_wide = to_wide(APP_AUTOSTART_VALUE);

        if enabled {
            let exe_path = std::env::current_exe().unwrap_or_default();
            let exe_path_str = exe_path.to_string_lossy();
            let path_val = format!("\"{}\"", exe_path_str);
            let path_wide = to_wide(&path_val);

            let bytes: &[u8] =
                std::slice::from_raw_parts(path_wide.as_ptr() as *const u8, path_wide.len() * 2);

            RegSetValueExW(hkey, PCWSTR(app_name_wide.as_ptr()), 0, REG_SZ, Some(bytes)).ok()?;
            reg_delete_value(hkey, LEGACY_AUTOSTART_VALUE);
        } else {
            reg_delete_value(hkey, APP_AUTOSTART_VALUE);
            reg_delete_value(hkey, LEGACY_AUTOSTART_VALUE);
        }

        RegCloseKey(hkey).ok()?;
    }
    Ok(())
}

pub fn shell_open(path: &Path) -> bool {
    shell_open_with(path, None, None)
}

pub fn shell_open_with(path: &Path, args: Option<&str>, working_dir: Option<&Path>) -> bool {
    unsafe {
        let operation: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
        let path_wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let args_wide = args.map(|value| {
            value
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect::<Vec<u16>>()
        });
        let cwd_wide = working_dir.map(|value| {
            value
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<u16>>()
        });
        let result = ShellExecuteW(
            HWND(std::ptr::null_mut()),
            PCWSTR(operation.as_ptr()),
            PCWSTR(path_wide.as_ptr()),
            args_wide
                .as_ref()
                .map(|w| PCWSTR(w.as_ptr()))
                .unwrap_or(PCWSTR(std::ptr::null())),
            cwd_wide
                .as_ref()
                .map(|w| PCWSTR(w.as_ptr()))
                .unwrap_or(PCWSTR(std::ptr::null())),
            SHOW_WINDOW_CMD(1),
        );
        let code = result.0 as isize;
        code > 32
    }
}

pub fn resolve_shortcut(path: &Path) -> Option<ShortcutResolution> {
    let is_lnk = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("lnk"))
        .unwrap_or(false);
    if !is_lnk {
        return None;
    }

    unsafe {
        let com_initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();

        let result = (|| {
            let shell_link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
            let persist_file: IPersistFile = shell_link.cast().ok()?;

            let shortcut_wide: Vec<u16> = path
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            persist_file
                .Load(PCWSTR(shortcut_wide.as_ptr()), STGM_READ)
                .ok()?;

            let _ = shell_link.Resolve(
                HWND(std::ptr::null_mut()),
                (SLR_NO_UI | SLR_ANY_MATCH).0 as u32,
            );

            let mut target_buf = vec![0u16; 4096];
            let mut find_data = WIN32_FIND_DATAW::default();
            let _ = shell_link.GetPath(&mut target_buf, &mut find_data, SLGP_RAWPATH.0 as u32);
            let mut target = utf16z_to_string(&target_buf);
            if target.trim().is_empty() {
                shell_link
                    .GetPath(&mut target_buf, &mut find_data, 0)
                    .ok()?;
                target = utf16z_to_string(&target_buf);
            }
            if target.trim().is_empty() {
                return None;
            }

            let mut args_buf = vec![0u16; 2048];
            let _ = shell_link.GetArguments(&mut args_buf);
            let arguments = normalize_opt_text(utf16z_to_string(&args_buf));

            let mut wd_buf = vec![0u16; 2048];
            let _ = shell_link.GetWorkingDirectory(&mut wd_buf);
            let working_dir = normalize_opt_text(utf16z_to_string(&wd_buf)).map(PathBuf::from);

            Some(ShortcutResolution {
                target_path: PathBuf::from(target.trim()),
                arguments,
                working_dir,
            })
        })();

        if com_initialized {
            CoUninitialize();
        }
        result
    }
}

pub fn resolve_shortcut_target(path: &Path) -> Option<PathBuf> {
    resolve_shortcut(path).map(|v| v.target_path)
}

fn utf16z_to_string(wide: &[u16]) -> String {
    let end = wide.iter().position(|c| *c == 0).unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..end])
}

fn normalize_opt_text(text: String) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
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
    fn resolve_shortcut_reads_target_args_and_workdir() {
        let uniq = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time error")
            .as_nanos();
        let base = std::env::temp_dir().join(format!("float_launcher_shortcut_test_{uniq}"));
        std::fs::create_dir_all(&base).expect("create temp dir");
        let target = base.join("dummy.exe");
        std::fs::write(&target, b"MZ").expect("write dummy exe");
        let shortcut = base.join("dummy.lnk");

        let script = format!(
            "$w=New-Object -ComObject WScript.Shell; \
             $s=$w.CreateShortcut('{shortcut}'); \
             $s.TargetPath='{target}'; \
             $s.Arguments='--from-shortcut'; \
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

        let resolved = resolve_shortcut(&shortcut).expect("shortcut should resolve");
        assert_eq!(norm(&resolved.target_path), norm(&target));
        assert_eq!(resolved.arguments.as_deref(), Some("--from-shortcut"));
        assert_eq!(
            resolved.working_dir.as_ref().map(|p| norm(p)),
            Some(norm(&base))
        );

        let _ = std::fs::remove_file(&shortcut);
        let _ = std::fs::remove_file(&target);
        let _ = std::fs::remove_dir_all(&base);
    }
}

#[allow(dead_code)]
pub fn apply_acrylic(hwnd: HWND) {
    println!("Applying acrylic effect to window HWND: {:?}", hwnd);
}
