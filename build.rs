use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-env-changed=RC");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    assert_eq!(
        env::var("CARGO_CFG_TARGET_ENV").as_deref(),
        Ok("msvc"),
        "Windows builds currently use MSVC resources"
    );
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("constellation.res");
    let mut candidates: Vec<PathBuf> = env::var_os("RC").map(PathBuf::from).into_iter().collect();
    if let Some(path) = env::var_os("PATH") {
        candidates.extend(env::split_paths(&path).map(|p| p.join("rc.exe")));
    }
    // CI can have the Windows SDK installed without its bin directory on PATH.
    let mut roots: Vec<PathBuf> = env::var_os("WindowsSdkDir")
        .map(PathBuf::from)
        .into_iter()
        .collect();
    if let Some(programs) = env::var_os("ProgramFiles(x86)") {
        roots.push(PathBuf::from(programs).join("Windows Kits/10"));
    }
    for root in roots {
        if let Ok(entries) = std::fs::read_dir(root.join("bin")) {
            let mut versions: Vec<_> = entries.flatten().map(|e| e.path()).collect();
            versions.sort();
            candidates.extend(versions.into_iter().rev().map(|p| p.join("x64/rc.exe")));
        }
    }
    let rc = candidates.into_iter().find(|p| p.is_file()).expect(
        "Activate E:/_DevTools/env.cmd or provide RC pointing to the existing Windows SDK rc.exe",
    );
    let status = Command::new(rc)
        .arg("/nologo")
        .arg("/fo")
        .arg(&out)
        .arg("assets/app.rc")
        .status()
        .expect("Run Windows SDK resource compiler");
    assert!(status.success(), "Icon resource compilation failed");
    println!("cargo:rustc-link-arg={}", out.display());
}
