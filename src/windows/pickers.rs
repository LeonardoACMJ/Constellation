//! System dialogs live only in the configuration process.
use super::wide;
use std::{
    mem::{size_of, zeroed},
    path::{Path, PathBuf},
    ptr::null,
};
use windows_sys::Win32::{
    Foundation::HWND,
    System::Com::CoTaskMemFree,
    UI::{
        Controls::Dialogs::*,
        Shell::*,
        WindowsAndMessaging::{IDYES, MB_ICONQUESTION, MB_YESNO, MessageBoxW},
    },
};

pub enum Pick {
    Target,
    Runner,
    Import,
    Export,
    Folder,
    Icon,
    Software,
}
pub unsafe fn choose(hwnd: HWND, root: &Path, pick: Pick) -> Result<Option<PathBuf>, String> {
    unsafe {
        if matches!(pick, Pick::Folder) {
            let mut display = [0u16; 260];
            let title = wide("Choose a filesystem folder");
            let info = BROWSEINFOW {
                hwndOwner: hwnd,
                pszDisplayName: display.as_mut_ptr(),
                lpszTitle: title.as_ptr(),
                ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE | BIF_EDITBOX,
                ..zeroed()
            };
            let id = SHBrowseForFolderW(&info);
            if id.is_null() {
                return Ok(None);
            }
            let mut path = vec![0u16; 32768];
            let ok = SHGetPathFromIDListEx(id, path.as_mut_ptr(), path.len() as u32, 0);
            CoTaskMemFree(id.cast());
            if ok == 0 {
                return Err("Choose an ordinary filesystem folder".into());
            }
            let len = path.iter().position(|v| *v == 0).unwrap_or(path.len());
            return Ok(Some(PathBuf::from(String::from_utf16_lossy(&path[..len]))));
        }
        let mut file = vec![0u16; 32768];
        if matches!(pick, Pick::Export) {
            let default = wide(root.join("constellation-export.constellation"));
            file[..default.len()].copy_from_slice(&default);
        }
        let filters = wide(if matches!(pick, Pick::Icon) {
            "Icon images (PNG, JPEG, BMP, ICO)\0*.png;*.jpg;*.jpeg;*.bmp;*.ico\0All files\0*.*\0"
        } else if matches!(pick, Pick::Software) {
            "Software icons (EXE, COM, LNK)\0*.exe;*.com;*.lnk\0All files\0*.*\0"
        } else if matches!(pick, Pick::Import | Pick::Export) {
            "Portable bundle (*.constellation)\0*.constellation\0Configuration JSON (*.json)\0*.json\0All files\0*.*\0"
        } else {
            "Applications, shortcuts and scripts\0*.exe;*.com;*.lnk;*.url;*.ps1;*.py;*.cmd;*.bat\0All files\0*.*\0"
        });
        let initial = wide(root);
        let extension = wide("constellation");
        let mut info: OPENFILENAMEW = zeroed();
        info.lStructSize = size_of::<OPENFILENAMEW>() as u32;
        info.hwndOwner = hwnd;
        info.lpstrFilter = filters.as_ptr();
        info.nFilterIndex = 1;
        info.lpstrFile = file.as_mut_ptr();
        info.nMaxFile = file.len() as u32;
        info.lpstrInitialDir = initial.as_ptr();
        info.lpstrDefExt = if matches!(pick, Pick::Export) {
            extension.as_ptr()
        } else {
            null()
        };
        info.Flags = OFN_EXPLORER
            | OFN_NOCHANGEDIR
            | OFN_PATHMUSTEXIST
            | if matches!(pick, Pick::Export) {
                OFN_OVERWRITEPROMPT
            } else {
                OFN_FILEMUSTEXIST
            };
        let ok = if matches!(pick, Pick::Export) {
            GetSaveFileNameW(&mut info)
        } else {
            GetOpenFileNameW(&mut info)
        };
        if ok == 0 {
            let code = CommDlgExtendedError();
            return if code == 0 {
                Ok(None)
            } else {
                Err(format!("File picker failed (Windows code {code})"))
            };
        }
        let len = file.iter().position(|v| *v == 0).unwrap_or(file.len());
        let mut path = PathBuf::from(String::from_utf16_lossy(&file[..len]));
        if matches!(pick, Pick::Export) && info.nFilterIndex == 2 {
            let selected = path.clone();
            path.set_extension("json");
            // Changing the extension changes the destination after the native
            // dialog's overwrite prompt. Confirm that final file as well.
            if selected != path
                && path.try_exists().map_err(|e| e.to_string())?
                && MessageBoxW(
                    hwnd,
                    wide(format!("{} already exists. Replace it?", path.display())).as_ptr(),
                    wide("Export configuration").as_ptr(),
                    MB_YESNO | MB_ICONQUESTION,
                ) != IDYES
            {
                return Ok(None);
            }
        }
        Ok(Some(path))
    }
}
