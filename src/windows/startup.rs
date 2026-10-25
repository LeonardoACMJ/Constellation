//! Opt-in "start with Windows", as a shortcut in the user's own Startup folder.
//!
//! A file the user can see, move and delete suits a portable app better than a
//! registry value they cannot; it also appears in Task Manager's Startup tab,
//! so the choice stays theirs to undo without this editor.
//!
//! windows-sys ships the CLSID but no COM interfaces, and the `windows` crate
//! would be a large dependency for one shortcut, so the two vtables are declared
//! here. Their slot order is the contract: a wrong index calls the wrong method.
//! Every slot is listed, used or not, so the order can be read against
//! `shobjidl_core.h` and `objidl.h` rather than counted.

use super::wide;
use std::path::{Path, PathBuf};
use windows_sys::Win32::{
    Foundation::S_OK,
    System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoTaskMemFree, CoUninitialize,
    },
    UI::Shell::{FOLDERID_Startup, SHGetKnownFolderPath, ShellLink},
};
use windows_sys::core::{GUID, HRESULT, PCWSTR};

const NAME: &str = "Constellation.lnk";
const IID_SHELL_LINK_W: GUID = GUID::from_u128(0x000214f9_0000_0000_c000_000000000046);
const IID_PERSIST_FILE: GUID = GUID::from_u128(0x0000010b_0000_0000_c000_000000000046);

type Unused = usize;

#[repr(C)]
struct ShellLinkVtbl {
    query_interface: unsafe extern "system" fn(*mut Link, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: Unused,
    release: unsafe extern "system" fn(*mut Link) -> u32,
    get_path: Unused,
    get_id_list: Unused,
    set_id_list: Unused,
    get_description: Unused,
    set_description: unsafe extern "system" fn(*mut Link, PCWSTR) -> HRESULT,
    get_working_directory: Unused,
    set_working_directory: unsafe extern "system" fn(*mut Link, PCWSTR) -> HRESULT,
    get_arguments: Unused,
    set_arguments: Unused,
    get_hotkey: Unused,
    set_hotkey: Unused,
    get_show_cmd: Unused,
    set_show_cmd: Unused,
    get_icon_location: Unused,
    set_icon_location: Unused,
    set_relative_path: Unused,
    resolve: Unused,
    // Last slot of IShellLinkW, not the first: order is easy to get wrong here.
    set_path: unsafe extern "system" fn(*mut Link, PCWSTR) -> HRESULT,
}
#[repr(C)]
struct Link {
    vtbl: *const ShellLinkVtbl,
}

#[repr(C)]
struct PersistFileVtbl {
    query_interface: Unused,
    add_ref: Unused,
    release: unsafe extern "system" fn(*mut PersistFile) -> u32,
    get_class_id: Unused,
    is_dirty: Unused,
    load: Unused,
    save: unsafe extern "system" fn(*mut PersistFile, PCWSTR, i32) -> HRESULT,
    save_completed: Unused,
    get_cur_file: Unused,
}
#[repr(C)]
struct PersistFile {
    vtbl: *const PersistFileVtbl,
}

use std::ffi::c_void;

/// Where the shortcut lives, or None when Windows will not name the folder.
pub fn shortcut() -> Option<PathBuf> {
    unsafe {
        let mut raw = std::ptr::null_mut();
        if SHGetKnownFolderPath(&FOLDERID_Startup, 0, std::ptr::null_mut(), &mut raw) != S_OK {
            return None;
        }
        let mut len = 0;
        while *raw.add(len) != 0 {
            len += 1;
        }
        let folder = String::from_utf16_lossy(std::slice::from_raw_parts(raw, len));
        CoTaskMemFree(raw.cast());
        (!folder.is_empty()).then(|| PathBuf::from(folder).join(NAME))
    }
}

pub fn enabled() -> bool {
    shortcut().is_some_and(|p| p.is_file())
}

/// Removing is a file deletion; creating needs COM.
pub fn set(on: bool) -> Result<(), String> {
    let path = shortcut().ok_or("Windows did not provide a Startup folder")?;
    if !on {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(format!("Cannot remove the Startup shortcut: {e}"))
            }
            _ => Ok(()),
        };
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let folder = exe.parent().ok_or("The executable has no folder")?;
    unsafe { write(&path, &exe, folder) }
}

unsafe fn write(link_path: &Path, exe: &Path, folder: &Path) -> Result<(), String> {
    unsafe {
        // The editor is a separate process that uses COM nowhere else, so it
        // initialises and tears down around this one call.
        let started = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
        let mut link: *mut Link = std::ptr::null_mut();
        let created = CoCreateInstance(
            &ShellLink,
            std::ptr::null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_SHELL_LINK_W,
            (&mut link as *mut *mut Link).cast(),
        );
        let result = (|| {
            if created != S_OK || link.is_null() {
                return Err("Cannot create a shortcut object".to_string());
            }
            let v = &*(*link).vtbl;
            if (v.set_path)(link, wide(&*exe.to_string_lossy()).as_ptr()) != S_OK {
                return Err("Cannot point the shortcut at this executable".into());
            }
            // A relative target resolves from the working directory, so the
            // shortcut must start the app in its own folder.
            (v.set_working_directory)(link, wide(&*folder.to_string_lossy()).as_ptr());
            (v.set_description)(link, wide("Constellation radial launcher").as_ptr());
            let mut file: *mut PersistFile = std::ptr::null_mut();
            if (v.query_interface)(
                link,
                &IID_PERSIST_FILE,
                (&mut file as *mut *mut PersistFile).cast(),
            ) != S_OK
                || file.is_null()
            {
                return Err("Cannot save the shortcut".into());
            }
            let fv = &*(*file).vtbl;
            let saved = (fv.save)(file, wide(&*link_path.to_string_lossy()).as_ptr(), 1);
            (fv.release)(file);
            if saved != S_OK {
                return Err(format!("Cannot write {}", link_path.display()));
            }
            Ok(())
        })();
        if !link.is_null() {
            ((*(*link).vtbl).release)(link);
        }
        if started == S_OK {
            CoUninitialize();
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Exercises the real vtables against a throwaway path. A wrong slot index
    /// would call another method here, in a test, rather than inside the
    /// resident app — and the user's own Startup folder is never touched.
    #[test]
    fn the_shell_writes_a_real_shortcut_through_these_vtables() {
        let dir =
            std::env::temp_dir().join(format!("constellation-startup-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let link = dir.join(NAME);
        let exe = std::env::current_exe().unwrap();
        unsafe { write(&link, &exe, exe.parent().unwrap()) }.unwrap();
        let bytes = std::fs::read(&link).expect("the shortcut exists");
        // Every .lnk opens with a 76-byte header whose first field is its size.
        assert_eq!(bytes[0..4], [0x4c, 0, 0, 0], "not a shell link header");
        assert!(bytes.len() > 100, "a real .lnk is not a stub");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_startup_path_is_named_after_this_app() {
        if let Some(p) = shortcut() {
            assert_eq!(p.file_name().unwrap(), NAME);
            assert!(p.parent().is_some_and(|d| d.is_dir()), "{p:?}");
        }
    }
}
