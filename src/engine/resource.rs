//! 从 EXE/DLL 的 PE 资源中提取图标（RT_GROUP_ICON + RT_ICON）。
//!
//! 逻辑与旧 Python 版一一对应，包含三个容易被忽略的细节：
//! 1. 图标组目录项里的宽高为 1 字节，`0` 代表 256；
//! 2. 质量分 = 宽×高 + 位深×10，按尺寸去重时保留分数最高的一帧；
//! 3. 组图标很少时，单独 RT_ICON 资源才是真正的图标来源，要兜底解析。

use std::path::Path;

use image::RgbaImage;
use windows::core::{BOOL, PCWSTR};
use windows::Win32::Foundation::{FreeLibrary, HGLOBAL, HMODULE};
use windows::Win32::System::LibraryLoader::{
    EnumResourceNamesW, FindResourceW, LoadLibraryExW, LoadResource, LockResource, SizeofResource,
    LOAD_LIBRARY_AS_DATAFILE,
};

const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;

/// 质量过滤开关打开时最多展示的图标数
const LIMIT_FILTERED: usize = 12;
/// 关闭质量过滤时最多展示的图标数
const LIMIT_ALL: usize = 20;

/// 提取结果
pub struct ExtractedIcon {
    /// 解码后的像素
    pub image: RgbaImage,
    /// 位深描述
    pub bit_label: String,
    /// 质量分
    pub score: i64,
    /// 宽
    pub width: u32,
    /// 高
    pub height: u32,
}

/// 资源名：整数 ID 或字符串名
#[derive(Clone)]
struct ResName {
    int_id: Option<u16>,
    text: String,
}

/// 图标组目录项
#[derive(Clone)]
struct GroupEntry {
    width: u32,
    height: u32,
    bit_count: u16,
    icon_id: u16,
    score: i64,
}

/// RAII：确保模块句柄被释放
struct Module(HMODULE);

impl Drop for Module {
    fn drop(&mut self) {
        unsafe {
            let _ = FreeLibrary(self.0);
        }
    }
}

fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// 把整数资源 ID 转成 `MAKEINTRESOURCE` 形式的指针
fn int_resource(id: u16) -> PCWSTR {
    PCWSTR(id as usize as *const u16)
}

unsafe extern "system" fn enum_name_proc(
    _module: HMODULE,
    _kind: PCWSTR,
    name: PCWSTR,
    lparam: isize,
) -> BOOL {
    let out = unsafe { &mut *(lparam as *mut Vec<ResName>) };
    let raw = name.0 as usize;
    if raw & 0xFFFF_0000 == 0 {
        // 高 16 位为 0：这是整数资源 ID（IS_INTRESOURCE 语义）
        out.push(ResName {
            int_id: Some(raw as u16),
            text: raw.to_string(),
        });
    } else {
        let mut len = 0usize;
        unsafe {
            while *name.0.add(len) != 0 {
                len += 1;
                if len > 512 {
                    break;
                }
            }
            let slice = std::slice::from_raw_parts(name.0, len);
            out.push(ResName {
                int_id: None,
                text: String::from_utf16_lossy(slice),
            });
        }
    }
    BOOL(1)
}

unsafe fn enum_names(module: HMODULE, type_id: u16) -> Vec<ResName> {
    let mut names: Vec<ResName> = Vec::new();
    let ptr = &mut names as *mut Vec<ResName> as isize;
    let ok = unsafe {
        EnumResourceNamesW(
            Some(module),
            int_resource(type_id),
            Some(enum_name_proc),
            ptr,
        )
    };
    if ok.as_bool() {
        names
    } else {
        Vec::new()
    }
}

unsafe fn load_blob(module: HMODULE, type_id: u16, name: &ResName) -> Option<Vec<u8>> {
    let wide_name: Option<Vec<u16>> = if name.int_id.is_none() {
        Some(name.text.encode_utf16().chain(std::iter::once(0)).collect())
    } else {
        None
    };
    let name_ptr = match (&wide_name, name.int_id) {
        (Some(buf), _) => PCWSTR(buf.as_ptr()),
        (None, Some(id)) => int_resource(id),
        (None, None) => return None,
    };
    let resource = unsafe { FindResourceW(Some(module), name_ptr, int_resource(type_id)) };
    if resource.is_invalid() {
        return None;
    }
    let handle: HGLOBAL = unsafe { LoadResource(Some(module), resource) }.ok()?;
    let size = unsafe { SizeofResource(Some(module), resource) } as usize;
    if size == 0 {
        return None;
    }
    let data = unsafe { LockResource(handle) } as *const u8;
    if data.is_null() {
        return None;
    }
    Some(unsafe { std::slice::from_raw_parts(data, size) }.to_vec())
}

/// 解析 GRPICONDIR：6 字节头 + 每个 14 字节目录项
fn parse_group(blob: &[u8]) -> Vec<GroupEntry> {
    if blob.len() < 6 {
        return Vec::new();
    }
    let count = u16::from_le_bytes([blob[4], blob[5]]) as usize;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let start = 6 + i * 14;
        if start + 14 > blob.len() {
            break;
        }
        let e = &blob[start..start + 14];
        let width = if e[0] == 0 { 256 } else { e[0] as u32 };
        let height = if e[1] == 0 { 256 } else { e[1] as u32 };
        let bit_count = u16::from_le_bytes([e[6], e[7]]);
        let icon_id = u16::from_le_bytes([e[12], e[13]]);
        out.push(GroupEntry {
            width,
            height,
            bit_count,
            icon_id,
            score: (width * height) as i64 + (bit_count as i64) * 10,
        });
    }
    out
}

fn bit_label_from_bpp(bit_count: u16) -> String {
    match bit_count {
        32 => "32位RGBA".to_string(),
        24 => "24位RGB".to_string(),
        8 => "8位索引".to_string(),
        4 => "4位索引".to_string(),
        1 => "1位黑白".to_string(),
        other => format!("{other}位"),
    }
}

/// 把单个 RT_ICON 数据解码为 RGBA。
///
/// 资源里可能是 PNG（Vista+ 的大图标），也可能是裸 DIB；裸 DIB 需要包一层
/// 单条目 ICO 头才能交给解码器。
fn decode_icon_blob(data: &[u8], width: u32, height: u32, bit_count: u16) -> Option<RgbaImage> {
    if data.len() < 16 {
        return None;
    }
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return crate::engine::raster::decode_bytes(data).ok();
    }
    let w_byte = if width == 0 || width >= 256 {
        0
    } else {
        width as u8
    };
    let h_byte = if height == 0 || height >= 256 {
        0
    } else {
        height as u8
    };
    let mut ico = Vec::with_capacity(22 + data.len());
    ico.extend_from_slice(&0u16.to_le_bytes());
    ico.extend_from_slice(&1u16.to_le_bytes());
    ico.extend_from_slice(&1u16.to_le_bytes());
    ico.push(w_byte);
    ico.push(h_byte);
    ico.push(0);
    ico.push(0);
    ico.extend_from_slice(&1u16.to_le_bytes());
    ico.extend_from_slice(&bit_count.max(1).to_le_bytes());
    ico.extend_from_slice(&(data.len() as u32).to_le_bytes());
    ico.extend_from_slice(&22u32.to_le_bytes());
    ico.extend_from_slice(data);
    crate::engine::raster::decode_bytes(&ico).ok()
}

/// 从 EXE/DLL 提取图标。
///
/// `quality_filter` 为真时只保留 24×24 以上且位深 ≥ 4 的图标（若全部被过滤则
/// 至少保留一个）；无论是否过滤，都按旧版的数量上限截断。
pub fn extract_icons(path: &Path, quality_filter: bool) -> Result<Vec<ExtractedIcon>, String> {
    let path_wide = wide(path);
    let module = unsafe {
        LoadLibraryExW(PCWSTR(path_wide.as_ptr()), None, LOAD_LIBRARY_AS_DATAFILE)
            .map_err(|e| format!("无法加载文件: {e}"))?
    };
    let module = Module(module);

    // 1) 收集图标组
    let mut candidates: Vec<GroupEntry> = Vec::new();
    let groups = unsafe { enum_names(module.0, RT_GROUP_ICON) };
    for group in &groups {
        let Some(blob) = (unsafe { load_blob(module.0, RT_GROUP_ICON, group) }) else {
            continue;
        };
        candidates.extend(parse_group(&blob));
    }

    // 2) 按尺寸去重，保留分数最高的一帧
    candidates.sort_by_key(|item| -item.score);
    let mut seen: Vec<(u32, u32)> = Vec::new();
    let mut unique: Vec<GroupEntry> = Vec::new();
    for entry in candidates {
        let key = (entry.width, entry.height);
        if !seen.contains(&key) {
            seen.push(key);
            unique.push(entry);
        }
    }
    unique.sort_by_key(|item| -item.score);

    // 3) 组图标太少时，用单独的 RT_ICON 资源兜底
    if unique.len() <= 3 {
        let individuals = unsafe { enum_names(module.0, RT_ICON) };
        for name in individuals.iter().take(15) {
            let Some(blob) = (unsafe { load_blob(module.0, RT_ICON, name) }) else {
                continue;
            };
            if blob.len() <= 40 {
                continue;
            }
            // PNG 直接读尺寸；DIB 从 BITMAPINFOHEADER 读 biWidth / biHeight
            let (w, h, bpp) = if blob.starts_with(b"\x89PNG\r\n\x1a\n") {
                match crate::engine::raster::decode_bytes(&blob) {
                    Ok(img) => (img.width(), img.height(), 32u16),
                    Err(_) => continue,
                }
            } else if blob.len() >= 16 {
                let bi_w = i32::from_le_bytes([blob[4], blob[5], blob[6], blob[7]]);
                let bi_h = i32::from_le_bytes([blob[8], blob[9], blob[10], blob[11]]);
                let bpp = u16::from_le_bytes([blob[14], blob[15]]);
                (bi_w.max(0) as u32, (bi_h / 2).max(0) as u32, bpp)
            } else {
                continue;
            };
            if w == 0 || h == 0 {
                continue;
            }
            // 单独资源的 ID 就是资源名；整数 ID 才能直接作为 RT_ICON 名加载
            if name.int_id.is_none() {
                continue;
            }
            unique.push(GroupEntry {
                width: w,
                height: h,
                bit_count: bpp,
                icon_id: name.int_id.unwrap_or(0),
                score: (w * h) as i64 + 320,
            });
        }
        unique.sort_by_key(|item| -item.score);
    }

    // 4) 逐个解码
    let mut decoded: Vec<ExtractedIcon> = Vec::new();
    for entry in &unique {
        let name = ResName {
            int_id: Some(entry.icon_id),
            text: entry.icon_id.to_string(),
        };
        let Some(blob) = (unsafe { load_blob(module.0, RT_ICON, &name) }) else {
            continue;
        };
        let Some(image) = decode_icon_blob(&blob, entry.width, entry.height, entry.bit_count)
        else {
            continue;
        };
        decoded.push(ExtractedIcon {
            width: image.width(),
            height: image.height(),
            bit_label: bit_label_from_bpp(entry.bit_count),
            score: entry.score,
            image,
        });
    }

    // 5) 质量过滤：至少 24×24 且位深 ≥ 4；若全被过滤掉则保留最高分的一个
    let selected: Vec<ExtractedIcon> = if quality_filter {
        let filtered: Vec<ExtractedIcon> = decoded
            .iter()
            .filter(|i| i.width >= 24 && i.height >= 24 && score_bpp(i) >= 4)
            .map(clone_icon)
            .collect();
        if filtered.is_empty() {
            decoded.first().map(clone_icon).into_iter().collect()
        } else {
            filtered
        }
    } else {
        decoded.iter().map(clone_icon).collect()
    };

    let limit = if quality_filter {
        LIMIT_FILTERED
    } else {
        LIMIT_ALL
    };
    Ok(selected.into_iter().take(limit).collect())
}

fn score_bpp(icon: &ExtractedIcon) -> u16 {
    // 位深描述里带有数字，直接解析前缀即可，避免额外字段
    icon.bit_label
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

fn clone_icon(icon: &ExtractedIcon) -> ExtractedIcon {
    ExtractedIcon {
        image: icon.image.clone(),
        bit_label: icon.bit_label.clone(),
        score: icon.score,
        width: icon.width,
        height: icon.height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_group_directory() {
        // 头：reserved=0 type=1 count=2
        let mut blob = vec![0, 0, 1, 0, 2, 0];
        // 项1：256×256（0 表示 256）、32 位、id=7
        blob.extend_from_slice(&[0, 0, 0, 0, 1, 0, 32, 0, 100, 0, 0, 0, 7, 0]);
        // 项2：48×48、8 位、id=8
        blob.extend_from_slice(&[48, 48, 0, 0, 1, 0, 8, 0, 50, 0, 0, 0, 8, 0]);
        let entries = parse_group(&blob);
        assert_eq!(entries.len(), 2);
        assert_eq!((entries[0].width, entries[0].height), (256, 256));
        assert_eq!(entries[0].bit_count, 32);
        assert_eq!(entries[0].score, 256 * 256 + 320);
        assert_eq!((entries[1].width, entries[1].height), (48, 48));
    }

    #[test]
    fn bit_labels_match_legacy_wording() {
        assert_eq!(bit_label_from_bpp(32), "32位RGBA");
        assert_eq!(bit_label_from_bpp(8), "8位索引");
        assert_eq!(bit_label_from_bpp(1), "1位黑白");
    }

    #[test]
    fn extracts_icons_from_system_shell() {
        // 用系统 DLL 做真实样本（一定有图标资源）
        let dll = Path::new(r"C:\Windows\System32\shell32.dll");
        if !dll.exists() {
            return;
        }
        let icons = extract_icons(dll, false).expect("提取 shell32 图标");
        assert!(!icons.is_empty(), "shell32 应该能提取到图标");
        assert!(icons.iter().all(|i| i.width > 0 && i.height > 0));
        // 未过滤时上限 20
        assert!(icons.len() <= LIMIT_ALL);
    }
}
