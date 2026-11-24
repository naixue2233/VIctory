//! Agility SDK opt-in.
//!
//! Windows 10's in-box D3D12 runtime tops out at shader model 6.5, and this crate emits
//! SM 6.6 (dynamic resources, `WaveSize`, compute derivatives). The Agility SDK loader in
//! `d3d12.dll` switches to a redistributable `D3D12Core.dll` when the host module exports
//! the two symbols below; `D3D12SDKPath` names the folder holding it, relative to the
//! executable, and `D3D12SDKVersion` must match the `D3D12_SDK_VERSION` in the SDK's
//! `d3d12.h` (620 for package 1.620.2).
//!
//! `build.rs` exports them with the linker's `/EXPORT:...,DATA` option. Once the exports are
//! present the loader refuses to create a device when the DLL is missing (`0x887e0003`), so
//! the DLL has to ship next to the executable under `D3D12\`.

#[allow(dead_code)]
#[unsafe(no_mangle)]
pub static D3D12SDKVersion: u32 = 620;

/// A `const char*`, not a char array: the loader dereferences this symbol.
#[allow(dead_code)]
#[unsafe(no_mangle)]
pub static D3D12SDKPath: &[u8; 9] = b".\\D3D12\\\0";
