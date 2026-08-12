//! Reserved unsafe edge for the future C ABI extracted from Nuvio's Windows
//! `player_bridge.cpp`. No native player library is linked or loaded in the POC.

#[repr(C)]
pub struct NuvioPlayerHandle {
    _private: [u8; 0],
}
