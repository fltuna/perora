fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("perora.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("the icon resource");
    }
    println!("cargo:rerun-if-changed=perora.rc");
    println!("cargo:rerun-if-changed=../src-tauri/icons/icon.ico");
}
