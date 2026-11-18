//! 把 `logo.ico` 编进 PE 资源段，让 exe 在资源管理器/任务栏里有自己的图标。
//!
//! Rust 默认不给可执行文件加图标资源，`icon_path` 只影响运行时的窗口图标，
//! 所以编译出来的 exe 在资源管理器里是空白图标。这里补上资源段。
//!
//! 不用 `winresource`：它靠 `reg.exe` 去注册表里翻 Windows SDK 路径，在受限
//! 环境（沙箱、精简镜像）会直接失败。改为依次尝试系统上现成的资源编译器：
//!
//! 1. `rc.exe`（Windows SDK / Visual Studio）
//! 2. `llvm-rc.exe`（LLVM）
//! 3. `windres`（MSYS2 / mingw）
//!
//! 三者都找不到时只打印告警，不影响构建。

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=logo.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let logo = Path::new(env!("CARGO_MANIFEST_DIR")).join("logo.ico");
    if !logo.is_file() {
        println!("cargo:warning=找不到 logo.ico，跳过 exe 图标嵌入");
        return;
    }

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    // rc 脚本里的路径统一用正斜杠：MS rc、llvm-rc、windres 都认，省掉转义问题
    let logo_rc = logo.display().to_string().replace('\\', "/");
    let rc_path = out_dir.join("app-icon.rc");
    if let Err(err) = std::fs::write(&rc_path, format!("1 ICON \"{logo_rc}\"\n")) {
        println!("cargo:warning=写入图标资源脚本失败: {err}");
        return;
    }
    let res_path = out_dir.join("app-icon.res");

    let Some((compiler, kind)) = find_compiler() else {
        println!(
            "cargo:warning=未找到 rc.exe / llvm-rc / windres，exe 将没有图标资源（功能不受影响）"
        );
        return;
    };

    let status = match kind {
        Kind::Rc => Command::new(&compiler)
            .arg(format!("/fo{}", res_path.display()))
            .arg(&rc_path)
            .output(),
        // windres 是 MSYS 程序，反斜杠会被它当成转义符吃掉，路径必须用正斜杠
        Kind::Windres => Command::new(&compiler)
            .arg("-i")
            .arg(slashes(&rc_path))
            .arg("-o")
            .arg(slashes(&res_path))
            // 默认产物是 COFF 对象，MSVC 链接器要的是 .res
            .arg("-O")
            .arg("res")
            .output(),
    };

    match status {
        Ok(output) if output.status.success() && res_path.is_file() => {
            println!("cargo:rustc-link-arg-bins={}", res_path.display());
        }
        Ok(output) => println!(
            "cargo:warning=图标资源编译失败（{} 退出码 {:?}）：{}",
            compiler.display(),
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        ),
        Err(err) => println!(
            "cargo:warning=无法执行 {}: {err}，exe 将没有图标",
            compiler.display()
        ),
    }
}

/// 转成正斜杠路径（MSYS 工具只认这个形式）
fn slashes(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

#[derive(Clone, Copy)]
enum Kind {
    /// rc.exe 与 llvm-rc 参数一致
    Rc,
    Windres,
}

fn find_compiler() -> Option<(PathBuf, Kind)> {
    if let Some(path) = in_path("rc.exe") {
        return Some((path, Kind::Rc));
    }
    if let Some(path) = in_path("llvm-rc.exe") {
        return Some((path, Kind::Rc));
    }
    let llvm = Path::new(r"C:\Program Files\LLVM\bin\llvm-rc.exe");
    if llvm.is_file() {
        return Some((llvm.to_path_buf(), Kind::Rc));
    }
    if let Some(path) = in_path("windres.exe").or_else(|| in_path("windres")) {
        return Some((path, Kind::Windres));
    }
    find_rc_in_visual_studio().map(|path| (path, Kind::Rc))
}

fn in_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// 在 Visual Studio 安装目录里翻 `VC/Tools/MSVC/<ver>/bin/Hostx64/x64/rc.exe`。
///
/// 只用于 `rc.exe` 不在 PATH 的机器（比如直接从 IDE 里构建），所以这里宁可
/// 写得笨一点，也不要引入额外依赖。
fn find_rc_in_visual_studio() -> Option<PathBuf> {
    let roots = [
        r"C:\Program Files\Microsoft Visual Studio",
        r"C:\Program Files (x86)\Microsoft Visual Studio",
    ];
    for root in roots {
        for edition in subdirs(Path::new(root)) {
            for version in subdirs(&edition) {
                let msvc = version.join("VC").join("Tools").join("MSVC");
                for toolset in subdirs(&msvc) {
                    for host in ["Hostx64", "Hostx86"] {
                        let candidate = toolset
                            .join("bin")
                            .join(host)
                            .join(if host == "Hostx64" { "x64" } else { "x86" })
                            .join("rc.exe");
                        if candidate.is_file() {
                            return Some(candidate);
                        }
                    }
                }
            }
        }
    }
    None
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect()
}
