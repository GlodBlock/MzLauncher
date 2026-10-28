fn main() {
    println!("cargo:rerun-if-changed=assets/logo-pixel.ico");
    println!("cargo:rerun-if-changed=assets/logo-pixel.rc");

    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let _ = embed_resource::compile("assets/logo-pixel.rc", embed_resource::NONE);
    }
}
