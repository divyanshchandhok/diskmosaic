//! Operating system integration: open, reveal, terminal, delete, clipboard.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn open_path(p: &Path) -> io::Result<()> {
    open::that_detached(p)
}

/// Show the item in the platform file manager (selecting it when possible).
pub fn reveal(p: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        Command::new("explorer.exe").arg(format!("/select,{}", p.display())).spawn()?;
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg("-R").arg(p).spawn()?;
        return Ok(());
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // Try the freedesktop FileManager1 interface first (selects the item).
        let uri = file_uri(p);
        let ok = Command::new("dbus-send")
            .args([
                "--session",
                "--dest=org.freedesktop.FileManager1",
                "--type=method_call",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
                &format!("array:string:{uri}"),
                "string:",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            return Ok(());
        }
        let dir = if p.is_dir() { p.to_path_buf() } else { p.parent().map(Path::to_path_buf).unwrap_or_else(|| p.to_path_buf()) };
        open::that_detached(dir)
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn file_uri(p: &Path) -> String {
    let s = p.to_string_lossy();
    let mut out = String::from("file://");
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Open a terminal / command prompt in the folder.
pub fn terminal_here(dir: &Path) -> io::Result<()> {
    let dir = if dir.is_dir() { dir.to_path_buf() } else { dir.parent().map(Path::to_path_buf).unwrap_or_default() };
    #[cfg(windows)]
    {
        Command::new("cmd.exe").args(["/c", "start", "cmd.exe", "/K", "cd", "/d"]).arg(&dir).spawn()?;
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").args(["-a", "Terminal"]).arg(&dir).spawn()?;
        return Ok(());
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut candidates: Vec<String> = Vec::new();
        if let Ok(t) = std::env::var("TERMINAL") {
            candidates.push(t);
        }
        for t in [
            "x-terminal-emulator", "xdg-terminal-exec", "gnome-terminal", "konsole", "kgx", "ptyxis", "alacritty",
            "kitty", "foot", "wezterm", "ghostty", "xfce4-terminal", "tilix", "terminator", "lxterminal", "xterm",
        ] {
            candidates.push(t.to_string());
        }
        for t in candidates {
            let mut cmd = Command::new(&t);
            cmd.current_dir(&dir);
            match t.as_str() {
                "gnome-terminal" | "ptyxis" | "kgx" => {
                    cmd.arg(format!("--working-directory={}", dir.display()));
                }
                "konsole" => {
                    cmd.arg("--workdir").arg(&dir);
                }
                "alacritty" | "wezterm" => {
                    if t == "wezterm" {
                        cmd.args(["start", "--cwd"]).arg(&dir);
                    } else {
                        cmd.arg("--working-directory").arg(&dir);
                    }
                }
                "kitty" => {
                    cmd.arg("--directory").arg(&dir);
                }
                "foot" => {
                    cmd.arg(format!("--working-directory={}", dir.display()));
                }
                "xfce4-terminal" | "tilix" | "terminator" | "lxterminal" => {
                    cmd.arg(format!("--working-directory={}", dir.display()));
                }
                _ => {}
            }
            if cmd.spawn().is_ok() {
                return Ok(());
            }
        }
        Err(io::Error::new(io::ErrorKind::NotFound, "No terminal emulator found"))
    }
}

pub fn move_to_trash(paths: &[PathBuf]) -> Result<(), String> {
    trash::delete_all(paths).map_err(|e| e.to_string())
}

pub fn delete_permanently(p: &Path) -> io::Result<()> {
    let m = std::fs::symlink_metadata(p)?;
    if m.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) }
}

pub fn rename(p: &Path, new_name: &str) -> io::Result<PathBuf> {
    let np = p.with_file_name(new_name);
    if np.exists() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, "A file with that name already exists"));
    }
    std::fs::rename(p, &np)?;
    Ok(np)
}

/// Put files on the clipboard so they can be pasted in a file manager.
/// `cut` marks them to be moved instead of copied.
pub fn clipboard_files(paths: &[PathBuf], cut: bool) -> Result<(), String> {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use std::io::Write;
        use std::process::Stdio;
        let uris: Vec<String> = paths.iter().map(|p| file_uri(p)).collect();
        let gnome = format!("{}\n{}", if cut { "cut" } else { "copy" }, uris.join("\n"));
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
        let tries: Vec<(&str, Vec<&str>, String)> = if wayland {
            vec![
                ("wl-copy", vec!["--type", "x-special/gnome-copied-files"], gnome.clone()),
                ("wl-copy", vec!["--type", "text/uri-list"], uris.join("\r\n")),
            ]
        } else {
            vec![
                ("xclip", vec!["-selection", "clipboard", "-t", "x-special/gnome-copied-files"], gnome.clone()),
                ("xclip", vec!["-selection", "clipboard", "-t", "text/uri-list"], uris.join("\r\n")),
            ]
        };
        for (cmd, args, data) in tries {
            if let Ok(mut child) = Command::new(cmd).args(&args).stdin(Stdio::piped()).spawn() {
                if let Some(mut si) = child.stdin.take() {
                    let _ = si.write_all(data.as_bytes());
                }
                // wl-copy forks into the background; don't wait for it.
                return Ok(());
            }
        }
        Err("Install wl-clipboard (Wayland) or xclip (X11) to copy files to the clipboard".into())
    }
    #[cfg(target_os = "macos")]
    {
        let _ = cut;
        let list: Vec<String> = paths.iter().map(|p| format!("POSIX file \"{}\"", p.display().to_string().replace('"', "\\\""))).collect();
        let script = format!("set the clipboard to {{{}}}", list.join(", "));
        Command::new("osascript").args(["-e", &script]).status().map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(windows)]
    {
        let verb = if cut { "Cut" } else { "Copy" };
        let list: Vec<String> = paths.iter().map(|p| format!("'{}'", p.display().to_string().replace('\'', "''"))).collect();
        let ps = format!(
            "Add-Type -AssemblyName System.Windows.Forms; $c = New-Object System.Collections.Specialized.StringCollection; @({}) | % {{ [void]$c.Add($_) }}; $d = New-Object System.Windows.Forms.DataObject; $d.SetFileDropList($c); $ms = New-Object System.IO.MemoryStream(4); $ms.Write([BitConverter]::GetBytes([int]{}),0,4); $d.SetData('Preferred DropEffect', $ms); [System.Windows.Forms.Clipboard]::SetDataObject($d, $true)",
            list.join(","),
            if verb == "Cut" { 2 } else { 5 }
        );
        Command::new("powershell").args(["-NoProfile", "-STA", "-Command", &ps]).status().map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Show the platform "properties / get info" dialog when available.
pub fn properties(p: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "tell application \"Finder\" to open information window of (POSIX file \"{}\" as alias)",
            p.display().to_string().replace('"', "\\\"")
        );
        let _ = Command::new("osascript").args(["-e", &script, "-e", "tell application \"Finder\" to activate"]).spawn();
        return true;
    }
    #[cfg(windows)]
    {
        let ps = format!(
            "$s = New-Object -ComObject Shell.Application; $f = $s.Namespace('{}'); $i = $f.ParseName('{}'); $i.InvokeVerb('properties'); Start-Sleep 30",
            p.parent().map(|x| x.display().to_string()).unwrap_or_default().replace('\'', "''"),
            p.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default().replace('\'', "''")
        );
        let _ = Command::new("powershell").args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &ps]).spawn();
        return true;
    }
    #[allow(unreachable_code)]
    {
        let _ = p;
        false
    }
}

pub fn is_elevated() -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return false;
            }
            let mut el = TOKEN_ELEVATION { TokenIsElevated: 0 };
            let mut len = 0u32;
            let ok = GetTokenInformation(token, TokenElevation, &mut el as *mut _ as *mut _, std::mem::size_of::<TOKEN_ELEVATION>() as u32, &mut len);
            CloseHandle(token);
            ok != 0 && el.TokenIsElevated != 0
        }
    }
}

/// Relaunch this program with administrator / root rights.
pub fn restart_elevated(args: &[String]) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        let arglist: Vec<String> = args.iter().map(|a| format!("'{}'", a.replace('\'', "''"))).collect();
        let ps = if arglist.is_empty() {
            format!("Start-Process -FilePath '{}' -Verb RunAs", exe.display())
        } else {
            format!("Start-Process -FilePath '{}' -Verb RunAs -ArgumentList {}", exe.display(), arglist.join(","))
        };
        Command::new("powershell").args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &ps]).spawn().map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        let cmd = format!("{} {} > /dev/null 2>&1 &", shell_quote(&exe.to_string_lossy()), args.iter().map(|a| shell_quote(a)).collect::<Vec<_>>().join(" "));
        let script = format!("do shell script \"{}\" with administrator privileges", cmd.replace('\\', "\\\\").replace('"', "\\\""));
        let st = Command::new("osascript").args(["-e", &script]).status().map_err(|e| e.to_string())?;
        if !st.success() {
            return Err("Administrator access wasn't granted, or the password prompt was cancelled.".into());
        }
        return Ok(());
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut cmd = Command::new("pkexec");
        cmd.arg("env");
        for v in ["DISPLAY", "WAYLAND_DISPLAY", "XDG_RUNTIME_DIR", "XAUTHORITY", "XDG_SESSION_TYPE", "DBUS_SESSION_BUS_ADDRESS"] {
            if let Ok(val) = std::env::var(v) {
                cmd.arg(format!("{v}={val}"));
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            // Keep using the invoking user's settings.
            cmd.arg(format!("DISKMOSAIC_CONFIG_HOME={home}"));
        }
        cmd.arg(&exe).args(args);
        cmd.spawn().map_err(|e| format!("Couldn't start the elevated relaunch: {e}"))?;
        Ok(())
    }
}

#[cfg(target_os = "macos")]
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn new_instance() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = Command::new(exe).spawn();
    }
}

pub fn restart_self() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = Command::new(exe).args(std::env::args().skip(1)).spawn();
    }
    std::process::exit(0);
}
