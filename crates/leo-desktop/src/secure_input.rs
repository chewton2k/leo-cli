#[cfg(target_os = "macos")]
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn IsSecureEventInputEnabled() -> u8;
}
pub fn enabled() -> bool {
    #[cfg(target_os = "macos")]
    {
        unsafe { IsSecureEventInputEnabled() != 0 }
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}
