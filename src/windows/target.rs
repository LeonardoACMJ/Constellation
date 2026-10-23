use super::wide;
use crate::{config::Target, storage};
use std::{
    path::Path,
    process::{Command, Stdio},
};
use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};

pub fn launch(root: &Path, target: &Target) -> Result<(), String> {
    match target {
        Target::Program { path, args, cwd } => spawn(root, path, args, cwd.as_deref()),
        Target::Script {
            path,
            runner,
            args,
            cwd,
        } => {
            let path = storage::resolve(root, path);
            if !path.is_file() {
                return Err(format!("Script does not exist: {}", path.display()));
            }
            let extension = path
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase();
            let runner = runner.as_deref().unwrap_or(match extension.as_str() {
                "ps1" => "powershell.exe",
                "py" => "python.exe",
                "bat" | "cmd" => "cmd.exe",
                _ => return Err("Set an explicit runner for this script type".into()),
            });
            let mut argv = vec![];
            let runner_name = Path::new(runner)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase();
            if runner_name == "cmd.exe" || runner_name == "cmd" {
                // cmd has a shell parser; reject command-expansion metacharacters rather
                // than accidentally treating a file name or argument as another command.
                let script = path.to_string_lossy();
                if [&*script]
                    .into_iter()
                    .chain(args.iter().map(String::as_str))
                    .any(|a| a.chars().any(|ch| "\"&|<>^%!\r\n".contains(ch)))
                {
                    return Err("Batch paths/arguments containing shell metacharacters are not supported; use a .lnk wrapper or an explicit non-shell runner".into());
                }
                let command = std::iter::once(script.as_ref())
                    .chain(args.iter().map(String::as_str))
                    .map(|v| format!("\"{v}\""))
                    .collect::<Vec<_>>()
                    .join(" ");
                let mut cmd = Command::new(runner);
                cmd.args(["/d", "/s", "/c"]);
                use std::os::windows::process::CommandExt;
                cmd.raw_arg(format!("\"{command}\""));
                cmd.current_dir(
                    cwd.as_deref()
                        .map(|v| storage::resolve(root, v))
                        .unwrap_or_else(|| root.to_path_buf()),
                );
                cmd.stdin(Stdio::null())
                    .spawn()
                    .map_err(|e| format!("Cannot run script: {e}"))?;
                return Ok(());
            }
            if runner_name.starts_with("powershell") || runner_name.starts_with("pwsh") {
                argv.extend(["-NoProfile".into(), "-File".into()]);
            }
            argv.push(path.to_string_lossy().into_owned());
            argv.extend_from_slice(args);
            spawn(root, runner, &argv, cwd.as_deref())
        }
        Target::Shortcut { path } | Target::Folder { path } => {
            shell(&storage::resolve(root, path).to_string_lossy(), root)
        }
        Target::Url { url } => shell(url, root),
    }
}
fn spawn(root: &Path, path: &str, args: &[String], cwd: Option<&str>) -> Result<(), String> {
    // A plain executable name is resolved through PATH; explicit relative paths use portable root.
    let command = if Path::new(path).components().count() > 1 || path.contains(['\\', '/']) {
        storage::resolve(root, path)
    } else {
        path.into()
    };
    let mut c = Command::new(&command);
    c.args(args)
        .current_dir(
            cwd.map(|v| storage::resolve(root, v))
                .unwrap_or_else(|| root.to_path_buf()),
        )
        .stdin(Stdio::null());
    c.spawn()
        .map_err(|e| format!("Could not launch {}: {e}", command.display()))?;
    Ok(())
}
fn shell(target: &str, root: &Path) -> Result<(), String> {
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            wide("open").as_ptr(),
            wide(target).as_ptr(),
            std::ptr::null(),
            wide(root).as_ptr(),
            SW_SHOWNORMAL,
        )
    };
    if result as isize <= 32 {
        Err(format!(
            "Could not open target (Windows code {})",
            result as isize
        ))
    } else {
        Ok(())
    }
}
