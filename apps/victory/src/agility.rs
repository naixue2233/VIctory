//! Agility SDK opt-in for the front end.
//!
//! The system `d3d12.dll` switches to the redistributable `D3D12Core.dll` shipped under
//! `D3D12\` next to this executable when the host module exports the two symbols below.
//! That is what makes the SM 6.6 this project emits reachable on Windows 10, whose in-box
//! runtime stops at 6.5. `D3D12SDKVersion` must equal the `D3D12_SDK_VERSION` of the
//! shipped SDK (620 for package 1.620.2).
//!
//! `build.rs` exports them with the linker's `/EXPORT:...,DATA` option. Once the exports
//! are present the loader refuses to create a device when the DLL is missing
//! (`0x887e0003`), so the DLL has to ship with the executable.

#[allow(dead_code)]
#[unsafe(no_mangle)]
pub static D3D12SDKVersion: u32 = 610;

/// A `const char*`, not a char array: the loader dereferences this symbol.
#[allow(dead_code)]
#[unsafe(no_mangle)]
pub static D3D12SDKPath: &[u8; 9] = b".\\D3D12\\\0";
