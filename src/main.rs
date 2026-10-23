#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    #[cfg(windows)]
    if let Err(e) = constellation::windows::run() {
        if std::env::args().any(|a| a == "--check-config") {
            eprintln!("{e}");
        } else {
            constellation::windows::error(&e);
        }
        std::process::exit(1)
    }
    #[cfg(not(windows))]
    {
        eprintln!(
            "The portable core builds on this platform; a Linux desktop adapter is not implemented yet."
        );
        std::process::exit(1);
    }
}
