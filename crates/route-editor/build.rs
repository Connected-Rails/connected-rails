//! Puts the window icon into the executable's own resource — what Explorer, the task
//! bar and Alt+Tab read. Gated on the host, like the build dependency it uses: the
//! resource compiler is the Windows SDK's, and the release builds the Windows
//! binaries on Windows.

fn main() {
    #[cfg(windows)]
    winresource::WindowsResource::new()
        .set_icon("../app-icon/icon.ico")
        .compile()
        .expect("embedding icon.ico");
}
