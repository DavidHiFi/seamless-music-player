// The exe's own icon. The .rc references assets/smp.ico; until that file
// exists the build still needs to succeed, so a fallback .rc pointing at a
// generated placeholder is used instead (written by build.rs itself).
fn main() {
    let ico = std::path::Path::new("assets/smp.ico");
    let rc = if ico.exists() { "app.rc" } else { "app-fallback.rc" };
    // Keep the path stable: the UI helper owns assets/smp.ico, this crate
    // only reads it.
    println!("cargo:rerun-if-changed=assets/smp.ico");
    // The page is baked into the exe: any change under ui/ needs a rebuild.
    println!("cargo:rerun-if-changed=ui");
    println!("cargo:rerun-if-changed={rc}");
    if std::path::Path::new(rc).exists() {
        embed_resource::compile(rc, embed_resource::NONE);
    }
}
