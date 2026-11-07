use std::path::PathBuf;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile(resources(), embed_resource::NONE)
            .manifest_optional()
            .expect("the icon resource");
    }
    println!("cargo:rerun-if-changed=perora.rc");
    println!("cargo:rerun-if-changed=../src-tauri/icons/icon.ico");
}

/// perora.rc (the icon), and after it what Windows says of the program: its name and its version
/// (Task Manager names it by them, and the list of programs that start with Windows). Written out
/// here, as the version comes from the manifest.
fn resources() -> PathBuf {
    let here = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let icon = std::fs::read_to_string(here.join("perora.rc"))
        .expect("perora.rc")
        .replace("../src-tauri/", &here.join("../src-tauri/").display().to_string().replace('\\', "/"));
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    // (Numbers only: a version such as 0.2.0-rc.1 is 0.2.0 to Windows.)
    let numbers: Vec<u16> = version
        .split(['.', '-', '+'])
        .take(3)
        .map(|part| part.parse().unwrap_or(0))
        .collect();
    let numeric = format!("{},{},{},0", numbers[0], numbers[1], numbers[2]);
    let text = format!(
        r#"{icon}

1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "FileDescription", "Perora"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "perora"
      VALUE "OriginalFilename", "perora.exe"
      VALUE "ProductName", "Perora"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    );
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("perora.rc");
    std::fs::write(&out, text).expect("the resources written out");
    out
}
