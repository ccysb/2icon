//! 应用数据模型：尺寸规则、输出格式、图标条目。
//!
//! 这些规则与界面无关，可单独测试；界面层只负责呈现与派发消息。

use std::sync::Arc;

use image::RgbaImage;

/// 预设尺寸按钮（与旧版界面保持一致）
pub const PRESET_SIZES: [u32; 8] = [16, 32, 48, 64, 96, 128, 256, 512];

/// ICO 容器单帧上限：目录项宽高字段只有 1 字节，0 代表 256
pub const ICO_SIZE_LIMIT: u32 = 256;

/// ICO 内恒定写入的标准尺寸阶梯（Windows 图标规范常用档位）
pub const ICO_STANDARD_SIZES: [u32; 8] = [256, 128, 96, 64, 48, 32, 24, 16];

/// 自定义尺寸允许范围
pub const SIZE_MIN: u32 = 16;
/// 自定义尺寸允许范围
pub const SIZE_MAX: u32 = 1024;

/// 256 档是否用 PNG 编码。
///
/// * `false` —— 全部 DIB 编码（整包约 400KB），任何解析器都能看到完整的 256；
///   `.NET System.Drawing.Icon`、rsrc 类工具、不少图标编辑器只认 DIB 条目。
/// * `true` —— 体积小（整包约 140KB），但只有支持 PNG 条目的解析器能识别 256。
///
/// 本工具的输出常被拿去喂别的 Windows 程序，故默认兼容优先。
pub const ICO_USE_PNG_FOR_256: bool = false;

/// 项目仓库地址（状态栏入口与关于对话框共用）
pub const GITHUB_REPO_URL: &str = "https://github.com/pcoof/2icon";

/// 网格缩略图边长（逻辑像素）
pub const THUMB_SIZE: u32 = 72;

/// 返回 ICO 内要写入的尺寸列表（从大到小）。
///
/// 阶梯恒以标准集合为基准，绝不因为“源图很小”或“当前输出尺寸很小”而裁掉
/// 大尺寸——否则导出的 ICO 会退化成只剩一个 16x16 帧的单帧文件。用户选中的
/// 尺寸若不是标准档（例如自定义 200），额外补入。
pub fn ico_size_ladder(preferred: u32) -> Vec<u32> {
    let mut sizes: Vec<u32> = ICO_STANDARD_SIZES.to_vec();
    let preferred = preferred.clamp(SIZE_MIN, ICO_SIZE_LIMIT);
    if !sizes.contains(&preferred) {
        sizes.push(preferred);
    }
    sizes.sort_unstable_by(|a, b| b.cmp(a));
    sizes.dedup();
    sizes
}

/// 输出格式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Windows 图标容器
    Ico,
    /// 无损位图
    Png,
    /// 有损位图
    Webp,
}

impl Format {
    /// 全部可选格式（界面按此顺序排列）
    pub const ALL: [Format; 3] = [Format::Ico, Format::Png, Format::Webp];

    /// 小写扩展名
    pub const fn extension(self) -> &'static str {
        match self {
            Format::Ico => "ico",
            Format::Png => "png",
            Format::Webp => "webp",
        }
    }

    /// 界面上显示的格式名
    pub const fn label(self) -> &'static str {
        match self {
            Format::Ico => "ICO",
            Format::Png => "PNG",
            Format::Webp => "WEBP",
        }
    }

    /// 该格式是否支持压缩质量（仅 WEBP）
    pub const fn uses_quality(self) -> bool {
        matches!(self, Format::Webp)
    }
}

/// 一个已提取出来的图标：原始像素 + 列表缩略图 + 位深描述。
#[derive(Clone, Debug)]
pub struct IconEntry {
    /// 原始宽度
    pub width: u32,
    /// 原始高度
    pub height: u32,
    /// 位深描述，例如 “32位RGBA”
    pub bit_label: String,
    /// 原始像素（保存、复制、缩放都用它）
    pub rgba: Arc<RgbaImage>,
}

impl IconEntry {
    /// 尺寸描述文本：“256x256”
    pub fn size_text(&self) -> String {
        format!("{}x{}", self.width, self.height)
    }
}

/// 生成默认输出路径：与输入同目录、同基名，扩展名跟随当前格式。
pub fn default_output_path(input: &str, format: Format) -> String {
    let path = std::path::Path::new(input);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    dir.join(format!("{stem}.{}", format.extension()))
        .to_string_lossy()
        .to_string()
}

/// 把输出路径的扩展名替换为当前格式。
pub fn retarget_extension(output: &str, format: Format) -> String {
    if output.trim().is_empty() {
        return output.to_string();
    }
    let path = std::path::Path::new(output);
    path.with_extension(format.extension())
        .to_string_lossy()
        .to_string()
}

/// 依据文件扩展名判断是否受支持的输入类型。
pub fn is_supported_input(path: &str) -> bool {
    matches!(
        extension_of(path).as_deref(),
        Some("png")
            | Some("jpg")
            | Some("jpeg")
            | Some("webp")
            | Some("svg")
            | Some("exe")
            | Some("dll")
            | Some("lnk")
    )
}

/// 取小写扩展名（不含点）
pub fn extension_of(path: &str) -> Option<String> {
    std::path::Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ladder_always_contains_standard_sizes() {
        assert_eq!(ico_size_ladder(16), vec![256, 128, 96, 64, 48, 32, 24, 16]);
        assert_eq!(ico_size_ladder(96), vec![256, 128, 96, 64, 48, 32, 24, 16]);
    }

    #[test]
    fn ladder_keeps_custom_size() {
        let ladder = ico_size_ladder(200);
        assert_eq!(ladder.first(), Some(&256));
        assert!(ladder.contains(&200));
    }

    #[test]
    fn ladder_clamps_above_limit() {
        let ladder = ico_size_ladder(1024);
        assert_eq!(ladder.first(), Some(&256));
        assert!(!ladder.contains(&1024));
    }

    #[test]
    fn output_path_follows_format() {
        let out = default_output_path(r"C:\icons\logo.png", Format::Ico);
        assert!(out.ends_with(r"logo.ico"), "{out}");
        assert_eq!(
            retarget_extension(&out, Format::Webp),
            r"C:\icons\logo.webp"
        );
    }

    #[test]
    fn supported_inputs() {
        assert!(is_supported_input("a.EXE"));
        assert!(is_supported_input("a.lnk"));
        assert!(!is_supported_input("a.txt"));
    }
}
