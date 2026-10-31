#[cfg(all(target_os = "macos", not(test)))]
pub struct Awake(
    objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2::runtime::NSObjectProtocol>>,
);

#[cfg(any(not(target_os = "macos"), test))]
pub struct Awake;

impl Awake {
    pub fn recording() -> Self {
        #[cfg(all(target_os = "macos", not(test)))]
        {
            Self(
                objc2_foundation::NSProcessInfo::processInfo().beginActivityWithOptions_reason(
                    objc2_foundation::NSActivityOptions::UserInitiated
                        | objc2_foundation::NSActivityOptions::IdleSystemSleepDisabled,
                    &objc2_foundation::NSString::from_str("Leo is recording audio"),
                ),
            )
        }
        #[cfg(any(not(target_os = "macos"), test))]
        {
            Self
        }
    }
}

#[cfg(all(target_os = "macos", not(test)))]
impl Drop for Awake {
    fn drop(&mut self) {
        unsafe { objc2_foundation::NSProcessInfo::processInfo().endActivity(&self.0) };
    }
}
