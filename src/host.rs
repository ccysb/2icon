//! 进程内 Win32 宿主增强。
//!
//! ZSUI 负责窗口创建、绘制与输入路由，但没有暴露窗口句柄，也没有文件拖放。
//! 这里在**本进程内**通过 `EnumWindows` 找到自己创建的顶层窗口，然后：
//!
//! * 去掉最小化/最大化按钮（对应旧版 Tk 的 `-toolwindow` 意图）；
//! * 打开 `WS_EX_ACCEPTFILES` 并子类化窗口过程，接收 `WM_DROPFILES`。
//!
//! 只拦截 `WM_DROPFILES`，其余消息一律转交原窗口过程，不影响框架绘制。

use std::sync::OnceLock;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{DragAcceptFiles, DragFinish, DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, EnumWindows, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsWindowVisible, SetWindowLongPtrW, SetWindowPos, GWL_STYLE,
    SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_MAXIMIZEBOX, WS_MINIMIZEBOX,
};

/// 一次拖放事件的处理者：拿到首个文件路径后写回应用并请求重建
type DropHandler = Box<dyn Fn(String) + Send + Sync>;

static DROP_HANDLER: OnceLock<DropHandler> = OnceLock::new();

/// 宿主增强的安装状态（只在 UI 线程写入）
static INSTALLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

const WM_DROPFILES: u32 = 0x0233;
const SUBCLASS_ID: usize = 0x32_49_43;

type SubclassProc =
    Option<unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM, usize, usize) -> LRESULT>;

#[link(name = "comctl32")]
unsafe extern "system" {
    fn SetWindowSubclass(
        hwnd: HWND,
        proc_: SubclassProc,
        id: usize,
        ref_data: usize,
    ) -> windows::core::BOOL;
    fn DefSubclassProc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
}

/// 注册拖放回调（在 UI 线程调用一次即可）
pub fn set_drop_handler(on_drop: impl Fn(String) + Send + Sync + 'static) {
    let _ = DROP_HANDLER.set(Box::new(on_drop));
}

/// 在**当前线程**安装宿主增强，返回是否已安装或此前已安装。
///
/// 必须在窗口所属线程（也就是 UI 线程）上调用：`SetWindowSubclass` 不接受
/// 跨线程子类化，跨线程调用会直接失败。视图函数由运行时在 UI 线程调用，
/// 因此在 `view` 里调用是安全的；未找到窗口时返回 `false`，下次重试。
pub fn try_install(window_title: &'static str) -> bool {
    if is_installed() {
        return true;
    }
    let Some(hwnd) = find_main_window(window_title) else {
        return false;
    };
    unsafe { apply_host_tweaks(hwnd) };
    INSTALLED.store(true, std::sync::atomic::Ordering::Release);
    true
}

/// 宿主增强是否已安装
pub fn is_installed() -> bool {
    INSTALLED.load(std::sync::atomic::Ordering::Acquire)
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
    let out = lparam.0 as *mut HWND;
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid != std::process::id() || !unsafe { IsWindowVisible(hwnd) }.as_bool() {
        return windows::core::BOOL(1);
    }
    // 只认有标题的可见顶层窗口，避免误抓工具提示之类的隐藏窗口
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    if len <= 0 {
        return windows::core::BOOL(1);
    }
    unsafe { *out = hwnd };
    windows::core::BOOL(0)
}

fn find_main_window(title: &str) -> Option<HWND> {
    let mut found = HWND(std::ptr::null_mut());
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut found as *mut HWND as isize));
    }
    if found.0.is_null() {
        return None;
    }
    // 记下实际标题，便于标题与预期不一致时排查（不影响后续增强）
    let mut buf = [0u16; 256];
    let len = unsafe { GetWindowTextW(found, &mut buf) } as usize;
    let text = String::from_utf16_lossy(&buf[..len.min(buf.len())]);
    if text != title {
        eprintln!("提示: 主窗口标题为 “{text}”，预期 “{title}”");
    }
    Some(found)
}

unsafe fn apply_host_tweaks(hwnd: HWND) {
    eprintln!("宿主增强: 找到窗口句柄 {hwnd:?}");
    // 1) 去掉最小化/最大化按钮
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) };
    let mask = (WS_MINIMIZEBOX.0 as isize) | (WS_MAXIMIZEBOX.0 as isize);
    unsafe {
        SetWindowLongPtrW(hwnd, GWL_STYLE, style & !mask);
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
        );
    }

    // 2) 接受文件拖放
    unsafe { DragAcceptFiles(hwnd, true) };

    // 3) 子类化以接收 WM_DROPFILES
    let style_after = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) };
    eprintln!("宿主增强: 样式 0x{style:08x} -> 0x{style_after:08x}");
    unsafe {
        let ok = SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, 0);
        eprintln!(
            "宿主增强: 子类化 {}",
            if ok.as_bool() { "成功" } else { "失败" }
        );
    }
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _ref_data: usize,
) -> LRESULT {
    if msg == WM_DROPFILES {
        let drop = HDROP(wparam.0 as *mut core::ffi::c_void);
        let count = unsafe { DragQueryFileW(drop, 0xFFFF_FFFF, None) };
        if count > 0 {
            let mut buf = [0u16; 1024];
            let len = unsafe { DragQueryFileW(drop, 0, Some(&mut buf)) } as usize;
            if len > 0 {
                let path = String::from_utf16_lossy(&buf[..len.min(buf.len())]);
                if let Some(handler) = DROP_HANDLER.get() {
                    handler(path);
                }
            }
        }
        unsafe { DragFinish(drop) };
        return LRESULT(0);
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// 用系统浏览器打开链接。
pub fn open_url(url: &str) {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let verb = HSTRING::from("open");
    let target = HSTRING::from(url);
    unsafe {
        ShellExecuteW(None, &verb, &target, None, None, SW_SHOWNORMAL);
    }
}

/// 未使用但保留：避免个别消息需要手动兜底时改动面扩大
#[allow(dead_code)]
unsafe fn default_window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}
