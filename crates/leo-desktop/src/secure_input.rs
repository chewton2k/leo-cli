#[cfg(target_os = "macos")]
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn IsSecureEventInputEnabled() -> u8;
}
#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceSecondsSinceLastEventType(state: i32, kind: u32) -> f64;
}
pub fn user_idle() -> std::time::Duration {
    #[cfg(target_os = "macos")]
    {
        std::time::Duration::try_from_secs_f64(unsafe {
            CGEventSourceSecondsSinceLastEventType(1, u32::MAX)
        })
        .unwrap_or_default()
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::time::Duration::MAX
    }
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
