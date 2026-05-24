//! Build script: on Windows, embed `assets/rsicon.ico` into every binary
//! produced by this crate (both `rustyserial.exe` and `rustyserial-gui.exe`)
//! so Explorer, the taskbar, and Alt+Tab all show the cereal-bowl icon.
//!
//! On other platforms this script is effectively a no-op.

fn main() {
    // Re-run only when the icon source changes; otherwise cargo would invoke
    // build.rs on every change to any file.
    println!("cargo:rerun-if-changed=assets/rsicon.ico");
    println!("cargo:rerun-if-changed=build.rs");

    #[cfg(target_os = "windows")]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/rsicon.ico");
        if let Err(err) = res.compile() {
            // Don't fail the whole build over icon embedding. Print a
            // warning the user will see if they `cargo build -v`.
            println!("cargo:warning=failed to embed Windows icon: {err}");
        }
    }
}
