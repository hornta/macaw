//! Embeds the icon, version information and application manifest into macaw.exe, so Windows
//! shows a proper name, publisher details and icon (in Explorer, Task Manager and security
//! prompts). The version comes from Cargo.toml.

use std::path::{Path, PathBuf};

fn main() {
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let icon = dir.join("../../assets/macaw.ico");
    let manifest = dir.join("macaw.manifest");
    let version = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    let number = |var: &str| std::env::var(var).ok().and_then(|v| v.parse::<u16>().ok()).unwrap_or(0);
    let (major, minor, patch) = (
        number("CARGO_PKG_VERSION_MAJOR"),
        number("CARGO_PKG_VERSION_MINOR"),
        number("CARGO_PKG_VERSION_PATCH"),
    );

    // Numeric constants instead of <winver.h>, so no SDK include path is needed:
    // FILEOS 0x40004 = VOS_NT_WINDOWS32, FILETYPE 0x1 = VFT_APP, 24 = RT_MANIFEST.
    let rc = format!(
        r#"1 ICON "{icon}"
1 24 "{manifest}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Macaw"
      VALUE "FileDescription", "Macaw: Mac keyboard helper for Windows"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "macaw"
      VALUE "LegalCopyright", "Open source under the MIT License"
      VALUE "OriginalFilename", "macaw.exe"
      VALUE "ProductName", "Macaw"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = rc_path(&icon),
        manifest = rc_path(&manifest),
    );
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("macaw.rc");
    std::fs::write(&out, rc).expect("write macaw.rc");
    println!("cargo:rerun-if-changed=macaw.manifest");
    println!("cargo:rerun-if-changed=../../assets/macaw.ico");
    embed_resource::compile(&out, embed_resource::NONE)
        .manifest_required()
        .expect("embed resources");
}

fn rc_path(path: &Path) -> String {
    path.display().to_string().replace('\\', "\\\\")
}
