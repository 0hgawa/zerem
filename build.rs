/// How much stack the Slint compiler gets.
///
/// Its passes recurse over the element tree, so the stack it needs grows with
/// how deeply the `.slint` nests — and the main thread of a build script gets
/// the default 8 MB on Linux but only 1 MB on Windows. Adding a wrapper around
/// the details panel was enough to overflow it, in release and not in debug,
/// which is a build that fails on one profile for a reason nothing in the
/// source hints at. A thread we own does not have that ceiling.
const COMPILER_STACK: usize = 32 * 1024 * 1024;

fn main() {
    std::thread::Builder::new()
        .stack_size(COMPILER_STACK)
        .spawn(|| slint_build::compile("ui/app.slint").expect("compile ui/app.slint"))
        .expect("spawn the Slint compiler")
        .join()
        .expect("the Slint compiler panicked");

    #[cfg(windows)]
    embed_windows_resources();
}

/// The executable icon and its version metadata.
///
/// `cfg(windows)` in a build script tests the HOST, which is also what gates the
/// `embed-resource` build dependency in Cargo.toml — the two agree on any native
/// build, and on a Linux host neither the crate nor this code exists.
#[cfg(windows)]
fn embed_windows_resources() {
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));

    // Written rather than committed: the same rasteriser the tray uses, so the
    // icon in Explorer and the icon in the tray cannot drift apart.
    let ico = out.join("zerem.ico");
    std::fs::write(&ico, zerem_core::icon::ico(&[16, 24, 32, 48, 64, 128, 256])).expect("write the app icon");

    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let part = |name: &str| -> u16 { std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(0) };
    let (major, minor, patch) =
        (part("CARGO_PKG_VERSION_MAJOR"), part("CARGO_PKG_VERSION_MINOR"), part("CARGO_PKG_VERSION_PATCH"));

    // Generated from the crate version, so the file's properties in Explorer
    // cannot disagree with Cargo.toml — one source of truth, no hand-edited .rc.
    let rc = format!(
        "1 ICON \"{icon}\"\n\
         1 VERSIONINFO\n\
         FILEVERSION {major},{minor},{patch},0\n\
         PRODUCTVERSION {major},{minor},{patch},0\n\
         FILEOS 0x40004L\n\
         FILETYPE 0x1L\n\
         BEGIN\n\
         \x20 BLOCK \"StringFileInfo\"\n\
         \x20 BEGIN\n\
         \x20   BLOCK \"040904b0\"\n\
         \x20   BEGIN\n\
         \x20     VALUE \"CompanyName\", \"Ohgawa\"\n\
         \x20     VALUE \"FileDescription\", \"Zerem - native BitTorrent client\"\n\
         \x20     VALUE \"FileVersion\", \"{version}\"\n\
         \x20     VALUE \"InternalName\", \"zerem\"\n\
         \x20     VALUE \"OriginalFilename\", \"zerem.exe\"\n\
         \x20     VALUE \"ProductName\", \"Zerem\"\n\
         \x20     VALUE \"ProductVersion\", \"{version}\"\n\
         \x20     VALUE \"LegalCopyright\", \"Copyright (C) 2026 Ohgawa\"\n\
         \x20   END\n\
         \x20 END\n\
         \x20 BLOCK \"VarFileInfo\"\n\
         \x20 BEGIN\n\
         \x20   VALUE \"Translation\", 0x409, 1200\n\
         \x20 END\n\
         END\n",
        // Doubled, because the resource compiler reads a backslash in a quoted
        // path as an escape of its own.
        icon = ico.display().to_string().replace('\\', "\\\\"),
    );

    let rc_path = out.join("zerem.rc");
    std::fs::write(&rc_path, rc).expect("write zerem.rc");
    embed_resource::compile(&rc_path, embed_resource::NONE)
        .manifest_optional()
        .expect("embed the icon and version info");
}
