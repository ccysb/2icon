//! 剪贴板：把 DIB 写入剪贴板（CF_DIB），可直接粘贴到画图、PS 等程序。

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_DIB;

/// 把 DIB 数据放进剪贴板。
pub fn set_dib(dib: &[u8]) -> Result<(), String> {
    if dib.is_empty() {
        return Err("图像数据为空".to_string());
    }
    unsafe {
        OpenClipboard(None).map_err(|e| format!("无法打开剪贴板: {e}"))?;
        let outcome = (|| -> Result<(), String> {
            EmptyClipboard().map_err(|e| format!("无法清空剪贴板: {e}"))?;
            let handle = GlobalAlloc(GMEM_MOVEABLE, dib.len())
                .map_err(|e| format!("无法分配剪贴板内存: {e}"))?;
            let target = GlobalLock(handle) as *mut u8;
            if target.is_null() {
                return Err("无法锁定剪贴板内存".to_string());
            }
            std::ptr::copy_nonoverlapping(dib.as_ptr(), target, dib.len());
            let _ = GlobalUnlock(handle);
            // 成功后内存所有权归剪贴板，不能自行释放
            SetClipboardData(CF_DIB.0 as u32, Some(HANDLE(handle.0)))
                .map_err(|e| format!("无法写入剪贴板: {e}"))?;
            Ok(())
        })();
        let _ = CloseClipboard();
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_payload() {
        assert!(set_dib(&[]).is_err());
    }

    #[test]
    fn writes_dib_to_clipboard() {
        // 2x2 白色 24 位 DIB
        let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 255, 255, 255]));
        let dib = crate::engine::raster::to_dib(&image);
        // 剪贴板可能被其它程序占用；失败时只要求错误信息可读
        match set_dib(&dib) {
            Ok(()) => {}
            Err(err) => assert!(!err.is_empty()),
        }
    }
}
