use std::path::PathBuf;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    // `OUT_DIR` is `target/<profile>/build/<pkg>-<hash>/out`; the profile directory is three
    // levels up. The application looks for the runtime in `target/<profile>/D3D12/`, so this
    // crate reads it from the same place instead of inventing a second convention.
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let profile = out
        .ancestors()
        .nth(3)
        .expect("a profile directory above OUT_DIR")
        .to_path_buf();
    let runtime = profile.join("D3D12").join("D3D12Core.dll");
    println!("cargo:rerun-if-changed={}", runtime.display());
    if !runtime.is_file() {
        // No app-local runtime: leave the in-box one alone, exactly as a build without
        // the opt-in would.
        return;
    }

    // The loader resolves `D3D12SDKPath` next to the running module, and test binaries run
    // from `deps`, so mirror the runtime there before the tests need it.
    let deps = profile.join("deps").join("D3D12");
    let _ = std::fs::create_dir_all(&deps);
    let _ = std::fs::copy(&runtime, deps.join("D3D12Core.dll"));

    // Export the two symbols the Agility SDK loader looks for in the host module, so
    // d3d12.dll loads that runtime instead of the in-box copy. Without them Windows 10
    // reports shader model 6.5 and every SM 6.6 test is skipped.
    println!("cargo:rustc-link-arg=/EXPORT:D3D12SDKVersion,DATA");
    println!("cargo:rustc-link-arg=/EXPORT:D3D12SDKPath,DATA");
}
