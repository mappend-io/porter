fn build_js(name: &str) {
    use std::process::Command;

    println!("cargo:rerun-if-changed={name}/src");
    println!("cargo:rerun-if-changed={name}/index.html");
    println!("cargo:rerun-if-changed={name}/vite.config.js");
    println!("cargo:rerun-if-changed={name}/package.json");

    let install_status = Command::new("bun")
        .args(["install"])
        .args(["--frozen-lockfile"])
        .current_dir(name)
        .status()
        .expect("failed to spawn 'bun install', is bun installed?");

    assert!(install_status.success(), "{name} install failed");

    let build_status = Command::new("bun")
        .args(["run", "build"])
        .current_dir(name)
        .status()
        .expect("failed to spawn 'bun run build', is bun installed?");

    assert!(build_status.success(), "{name} build failed");
}

fn main() {
    #[cfg(feature = "embedded-viewer")]
    {
        build_js("viewer");
        build_js("terrarium");
    }
}
