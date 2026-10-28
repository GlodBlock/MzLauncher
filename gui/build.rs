fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/icon.rc");

    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let _ = embed_resource::compile("assets/icon.rc", embed_resource::NONE);
    }
}
