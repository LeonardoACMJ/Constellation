use super::wide;
use crate::{config::Target, storage};
use std::{
    path::Path,
    process::{Command, Stdio},
};
use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};

pub fn launch(root: &Path, target: &Target) -> Result<(), String> {
    launch_using(root, target, &mut shell)
}
fn launch_using(
    root: &Path,
    target: &Target,
    open: &mut impl FnMut(&str, &Path) -> Result<(), String>,
) -> Result<(), String> {
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
                return spawn_console(
                    root,
                    runner,
                    &[],
                    Some(&format!("/d /s /c \"{command}\"")),
                    cwd.as_deref(),
                );
            }
            if runner_name.starts_with("powershell") || runner_name.starts_with("pwsh") {
                argv.extend(["-NoProfile".into(), "-File".into()]);
            }
            argv.push(path.to_string_lossy().into_owned());
            argv.extend_from_slice(args);
            spawn_console(root, runner, &argv, None, cwd.as_deref())
        }
        Target::Shortcut { path } | Target::Folder { path } => {
            let resolved = storage::resolve(root, path);
            let exists = if matches!(target, Target::Folder { .. }) {
                resolved.is_dir()
            } else {
                resolved.is_file()
            };
            if !exists {
                return Err(format!(
                    "Target does not exist or has the wrong type: {}",
                    resolved.display()
                ));
            }
            open(&resolved.to_string_lossy(), root)
        }
        Target::Url { url } => open(url, root),
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
// Script runners need their own real console input/output. Inheriting the GUI
// resident's missing handles (or NUL stdin) breaks set /p, Read-Host and timeout.
// Do not attach a console to the resident or change its process-wide handles.
fn spawn_console(
    root: &Path,
    runner: &str,
    args: &[String],
    raw_tail: Option<&str>,
    cwd: Option<&str>,
) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{
            CREATE_NEW_CONSOLE, CreateProcessW, PROCESS_INFORMATION, STARTUPINFOW,
        },
    };
    let executable = if Path::new(runner).components().count() > 1 || runner.contains(['\\', '/']) {
        storage::resolve(root, runner)
    } else {
        runner.into()
    };
    let executable = executable
        .to_str()
        .ok_or("Script runner is not valid Unicode")?;
    if executable.contains(['\"', '\0']) {
        return Err("Invalid script runner path".into());
    }
    // argv[0] is quoted as a file name; subsequent arguments follow CRT escaping.
    let mut line = format!("\"{executable}\"");
    for arg in args {
        line.push(' ');
        line.push_str(&quote_argument(arg));
    }
    if let Some(raw) = raw_tail {
        line.push(' ');
        line.push_str(raw);
    }
    let directory = cwd
        .map(|v| storage::resolve(root, v))
        .unwrap_or_else(|| root.into());
    let directory: Vec<u16> = directory.as_os_str().encode_wide().chain([0]).collect();
    if line.contains('\0') || directory[..directory.len() - 1].contains(&0) {
        return Err("Launch arguments contain a NUL character".into());
    }
    let mut line = wide(&line);
    if line.len() > 32767 {
        return Err("Script command line is too long".into());
    }
    unsafe {
        let mut startup: STARTUPINFOW = std::mem::zeroed();
        startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        let mut process: PROCESS_INFORMATION = std::mem::zeroed();
        // No STARTF_USESTDHANDLES: Windows supplies the new console's handles.
        // No handle inheritance: never pass the tray app's handles to scripts.
        if CreateProcessW(
            std::ptr::null(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_NEW_CONSOLE,
            std::ptr::null(),
            directory.as_ptr(),
            &startup,
            &mut process,
        ) == 0
        {
            return Err(format!(
                "Cannot run script: {}",
                std::io::Error::last_os_error()
            ));
        }
        CloseHandle(process.hThread);
        CloseHandle(process.hProcess);
    }
    Ok(())
}

fn quote_argument(value: &str) -> String {
    let mut result = String::from("\"");
    let mut slashes = 0;
    for ch in value.chars() {
        if ch == '\\' {
            slashes += 1;
            continue;
        }
        result.extend(std::iter::repeat_n(
            '\\',
            if ch == '\"' { slashes * 2 + 1 } else { slashes },
        ));
        result.push(ch);
        slashes = 0;
    }
    result.extend(std::iter::repeat_n('\\', slashes * 2));
    result.push('\"');
    result
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicUsize, Ordering},
        time::{Duration, Instant},
    };
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    fn sandbox() -> std::path::PathBuf {
        let p = std::env::current_dir()
            .unwrap()
            .join("target/launch-checks")
            .join(format!(
                "{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&p).unwrap();
        p
    }
    fn wait(path: &Path) {
        let until = Instant::now() + Duration::from_secs(15);
        while !path.exists() {
            assert!(Instant::now() < until, "Missing {}", path.display());
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    #[test]
    fn console_argument_escaping_preserves_quotes_and_trailing_slashes() {
        assert_eq!(quote_argument(""), "\"\"");
        assert_eq!(quote_argument("two words"), "\"two words\"");
        assert_eq!(quote_argument("a\"b"), "\"a\\\"b\"");
        assert_eq!(quote_argument("C:\\two words\\"), "\"C:\\two words\\\\\"");
        assert_eq!(quote_argument("a\\\"b"), "\"a\\\\\\\"b\"");
    }

    // Remote Windows CI only: no real user script or power-management action.
    #[test]
    fn interactive_batch_has_real_console_input_and_output() {
        let root = sandbox().join("folder (with spaces)");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("seed.ps1"),
            r#"
$ErrorActionPreference='Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class InputProbe {
 [StructLayout(LayoutKind.Explicit, CharSet=CharSet.Unicode, Size=20)]
 public struct Record {
  [FieldOffset(0)] public short Type;
  [FieldOffset(4)] public int Down;
  [FieldOffset(8)] public short Repeat;
  [FieldOffset(14)] public char Character;
 }
 [DllImport("kernel32.dll")] static extern IntPtr GetStdHandle(int id);
 [DllImport("kernel32.dll")] static extern bool GetConsoleMode(IntPtr h, out uint mode);
 [DllImport("kernel32.dll", CharSet=CharSet.Unicode)]
 static extern bool WriteConsoleInputW(IntPtr h, Record[] records, uint count, out uint written);
 public static void Seed() {
  uint mode, written;
  foreach(int id in new int[]{-10,-11,-12})
   if(!GetConsoleMode(GetStdHandle(id),out mode)) throw new Exception("Invalid console handle");
  var records=new Record[3];
  var chars="17\r";
  for(int i=0;i<3;i++) records[i]=new Record{Type=1,Down=1,Repeat=1,Character=chars[i]};
  if(!WriteConsoleInputW(GetStdHandle(-10),records,3,out written)||written!=3)
   throw new Exception("Cannot seed isolated console");
 }
}
'@
[InputProbe]::Seed()
[IO.File]::WriteAllText((Join-Path (Get-Location) 'console.txt'), 'real console')
"#,
        )
        .unwrap();
        fs::write(root.join("Sleep fixture (inert).bat"),
            "@echo off\r\npowershell.exe -NoProfile -File seed.ps1\r\nif errorlevel 1 exit /b 1\r\nset /p minutes=Minutes (inert test): \r\n> answer.txt echo %minutes%\r\n").unwrap();
        launch(
            &root,
            &Target::Script {
                path: "Sleep fixture (inert).bat".into(),
                runner: None,
                args: vec![],
                cwd: None,
            },
        )
        .unwrap();
        wait(&root.join("answer.txt"));
        assert_eq!(
            fs::read_to_string(root.join("console.txt")).unwrap(),
            "real console"
        );
        assert_eq!(
            fs::read_to_string(root.join("answer.txt")).unwrap().trim(),
            "17"
        );
    }

    #[test]
    fn program_arguments_and_cwd_are_literal() {
        let root = sandbox();
        fs::create_dir(root.join("working folder")).unwrap();
        launch(&root,&Target::Program { path:"powershell.exe".into(), cwd:Some("working folder".into()),
            args:vec!["-NoProfile".into(),"-NonInteractive".into(),"-Command".into(),
                "[IO.File]::WriteAllText((Join-Path (Get-Location) 'program.txt'), 'spaces & literal')".into()] }).unwrap();
        wait(&root.join("working folder/program.txt"));
        assert_eq!(
            fs::read_to_string(root.join("working folder/program.txt")).unwrap(),
            "spaces & literal"
        );
    }
    #[test]
    fn batch_and_powershell_scripts_run_from_portable_root() {
        let root = sandbox();
        fs::write(
            root.join("mark.cmd"),
            "@echo off\r\n> batch.txt echo %~1\r\n",
        )
        .unwrap();
        launch(
            &root,
            &Target::Script {
                path: "mark.cmd".into(),
                runner: None,
                args: vec!["two words".into()],
                cwd: None,
            },
        )
        .unwrap();
        wait(&root.join("batch.txt"));
        assert_eq!(
            fs::read_to_string(root.join("batch.txt")).unwrap().trim(),
            "two words"
        );
        fs::write(root.join("mark.ps1"),"param([string]$Message)\n[IO.File]::WriteAllText((Join-Path (Get-Location) 'ps.txt'), $Message)").unwrap();
        launch(
            &root,
            &Target::Script {
                path: "mark.ps1".into(),
                runner: None,
                args: vec!["literal & value".into()],
                cwd: None,
            },
        )
        .unwrap();
        wait(&root.join("ps.txt"));
        assert_eq!(
            fs::read_to_string(root.join("ps.txt")).unwrap(),
            "literal & value"
        );
        assert!(
            launch(
                &root,
                &Target::Script {
                    path: "mark.cmd".into(),
                    runner: None,
                    args: vec!["unsafe&argument".into()],
                    cwd: None
                }
            )
            .is_err()
        );
    }
    #[test]
    fn shell_routes_urls_and_relative_folders_without_opening_handlers() {
        let root = sandbox();
        fs::create_dir(root.join("folder")).unwrap();
        let mut calls = vec![];
        let mut open = |target: &str, cwd: &Path| {
            calls.push((target.to_string(), cwd.to_path_buf()));
            Ok(())
        };
        launch_using(
            &root,
            &Target::Url {
                url: "https://example.invalid/path?q=hello%20world".into(),
            },
            &mut open,
        )
        .unwrap();
        launch_using(
            &root,
            &Target::Folder {
                path: "folder".into(),
            },
            &mut open,
        )
        .unwrap();
        assert_eq!(calls[0].0, "https://example.invalid/path?q=hello%20world");
        assert_eq!(std::path::PathBuf::from(&calls[1].0), root.join("folder"));
        assert!(calls.iter().all(|(_, cwd)| cwd == &root));
    }
    #[test]
    fn missing_targets_return_errors_before_shell_dispatch() {
        let root = sandbox();
        let mut calls = 0;
        for target in [
            Target::Folder {
                path: "missing".into(),
            },
            Target::Shortcut {
                path: "missing.lnk".into(),
            },
        ] {
            assert!(
                launch_using(&root, &target, &mut |_, _| {
                    calls += 1;
                    Ok(())
                })
                .is_err()
            );
        }
        assert_eq!(calls, 0);
        assert!(
            launch(
                &root,
                &Target::Program {
                    path: ".\\missing.exe".into(),
                    args: vec![],
                    cwd: None
                }
            )
            .is_err()
        );
        assert!(
            launch(
                &root,
                &Target::Script {
                    path: "missing.ps1".into(),
                    runner: None,
                    args: vec![],
                    cwd: None
                }
            )
            .is_err()
        );
    }
    #[test]
    fn windows_shortcut_dispatches_once_to_a_controlled_target() {
        let root = sandbox();
        let quote = |p: &Path| p.to_string_lossy().replace("'", "''");
        let script = format!(
            "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}'); $s.TargetPath=$env:ComSpec; $s.Arguments='/d /c echo shortcut>shortcut.txt'; $s.WorkingDirectory='{}'; $s.WindowStyle=7; $s.Save()",
            quote(&root.join("mark.lnk")),
            quote(&root)
        );
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        launch(
            &root,
            &Target::Shortcut {
                path: "mark.lnk".into(),
            },
        )
        .unwrap();
        wait(&root.join("shortcut.txt"));
        assert_eq!(
            fs::read_to_string(root.join("shortcut.txt"))
                .unwrap()
                .trim(),
            "shortcut"
        );
    }
}
