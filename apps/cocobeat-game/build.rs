fn main() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=../../assets/brand/icons/cocobeat.ico");
    #[cfg(windows)]
    embed_resource::compile("app.rc", embed_resource::NONE)
        .manifest_required()
        .expect("Cannot embed Windows application icon");
}
