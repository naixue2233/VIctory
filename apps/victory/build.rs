fn main() {
    // Export the two symbols the Agility SDK loader looks for in the host module, so
    // d3d12.dll loads D3D12Core.dll from `.\D3D12\` instead of using the in-box runtime.
    // Without those exports a Windows 10 host cannot offer shader model 6.6.
    //
    // Done with /EXPORT rather than a .def file: the repository's asset policy only
    // allows an allowlisted set of file types.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-arg=/EXPORT:D3D12SDKVersion,DATA");
        println!("cargo:rustc-link-arg=/EXPORT:D3D12SDKPath,DATA");
    }
}
