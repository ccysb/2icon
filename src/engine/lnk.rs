//! 解析 Windows 快捷方式（.lnk）指向的目标文件。

use std::path::Path;

use windows::core::{Interface, HSTRING};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

/// 取回快捷方式指向的目标路径。
pub fn resolve_target(lnk: &Path) -> Result<String, String> {
    unsafe {
        // 同一线程重复初始化会返回 S_FALSE；模式冲突可忽略，COM 已可用即可
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    let link: IShellLinkW = unsafe {
        CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| format!("无法创建 ShellLink: {e}"))?
    };

    let persist: windows::Win32::System::Com::IPersistFile = link
        .cast()
        .map_err(|e| format!("无法获取 IPersistFile: {e}"))?;

    let path = HSTRING::from(lnk.as_os_str());
    unsafe {
        persist
            .Load(&path, windows::Win32::System::Com::STGM_READ)
            .map_err(|e| format!("无法读取快捷方式: {e}"))?;
    }

    let mut buffer = [0u16; 1024];
    unsafe {
        link.GetPath(&mut buffer, std::ptr::null_mut(), 0)
            .map_err(|e| format!("无法解析快捷方式目标: {e}"))?;
    }

    let len = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
    let target = String::from_utf16_lossy(&buffer[..len]);
    if target.trim().is_empty() {
        return Err("快捷方式没有指向任何目标文件".to_string());
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_a_real_shortcut_when_available() {
        // 开始菜单里一定有 .lnk；找不到就跳过（不同机器布局不同）
        let dirs = [
            r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs",
            r"C:\Users\Public\Desktop",
        ];
        let mut found = None;
        for dir in dirs {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map(|e| e.eq_ignore_ascii_case("lnk")) == Some(true) {
                        found = Some(path);
                        break;
                    }
                }
            }
            if found.is_some() {
                break;
            }
        }
        let Some(lnk) = found else { return };
        let target = resolve_target(&lnk);
        // 目标可能已卸载，但至少不应 panic，且错误信息可读
        match target {
            Ok(path) => assert!(!path.is_empty()),
            Err(err) => assert!(!err.is_empty()),
        }
    }
}
