//! 输入分发：按扩展名决定用哪条路径取图标。

pub mod clipboard;
pub mod ico;
pub mod lnk;
pub mod raster;
pub mod resource;
pub mod svg;

use std::sync::Arc;

use crate::model::{extension_of, IconEntry};

/// 把一张位图包装成列表条目
///
/// 列表里直接展示原图（由 `image_preview` 按帧缩放），不再另存缩略图，
/// 省掉每个图标一次 PNG 编码。
pub fn entry_from_image(image: image::RgbaImage, bit_label: String) -> IconEntry {
    let (width, height) = image.dimensions();
    IconEntry {
        width,
        height,
        bit_label,
        rgba: Arc::new(image),
    }
}

/// 读取任意受支持的输入，返回图标条目列表。
///
/// 图像与 SVG 得到单条，EXE/DLL/LNK 得到多条（已按质量排序）。
pub fn load_icons(path: &str, quality_filter: bool) -> Result<Vec<IconEntry>, String> {
    let ext = extension_of(path).ok_or_else(|| "无法识别文件类型".to_string())?;
    let file = std::path::Path::new(path);
    if !file.exists() {
        return Err(format!("文件不存在: {path}"));
    }

    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif" => {
            let decoded = raster::load_raster(file)?;
            Ok(vec![entry_from_image(decoded.image, decoded.bit_label)])
        }
        "svg" => {
            let image = svg::render_file(file)?;
            Ok(vec![entry_from_image(image, "32位RGBA".to_string())])
        }
        "lnk" => {
            let target = lnk::resolve_target(file)?;
            load_from_binary(&target, quality_filter)
        }
        "exe" | "dll" => load_from_binary(path, quality_filter),
        other => Err(format!("不支持的文件类型: .{other}")),
    }
}

fn load_from_binary(path: &str, quality_filter: bool) -> Result<Vec<IconEntry>, String> {
    let extracted = resource::extract_icons(std::path::Path::new(path), quality_filter)?;
    if extracted.is_empty() {
        return Err("文件中没有找到图标资源".to_string());
    }
    Ok(extracted
        .into_iter()
        .map(|icon| entry_from_image(icon.image, icon.bit_label))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_png_as_single_entry() {
        let path = std::env::temp_dir().join("icon_tool_load.png");
        let img = image::RgbaImage::from_pixel(64, 64, image::Rgba([1, 2, 3, 255]));
        img.save(&path).unwrap();
        let icons = load_icons(path.to_str().unwrap(), true).expect("load png");
        assert_eq!(icons.len(), 1);
        assert_eq!((icons[0].width, icons[0].height), (64, 64));
        assert_eq!(icons[0].bit_label, "32位RGBA");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_unknown_extension() {
        let path = std::env::temp_dir().join("icon_tool_unknown.txt");
        std::fs::write(&path, b"not an icon").unwrap();
        let err = load_icons(path.to_str().unwrap(), true).unwrap_err();
        assert!(err.contains("不支持"), "实际错误: {err}");
        let _ = std::fs::remove_file(&path);
    }
}
