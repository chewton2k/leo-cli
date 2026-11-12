use enigo::{Enigo, Key, Keyboard, Settings};
use std::sync::Mutex;

const SHIFT: u64 = 0x0002_0000;
const CONTROL: u64 = 0x0004_0000;
const OPTION: u64 = 0x0008_0000;
const COMMAND: u64 = 0x0010_0000;
const FN: u64 = 0x0080_0000;

#[cfg(target_os = "macos")]
mod macos {
    use super::Key;
    use log::{debug, warn};
    use std::ffi::c_void;

    type TisInputSourceRef = *const c_void;
    type CfDataRef = *const c_void;
    type CfStringRef = *const c_void;

    const ANSI_V_KEYCODE: u16 = 9;
    const KEYCODE_COUNT: u16 = 128;
    const UC_KEY_ACTION_DISPLAY: u16 = 3;
    const UC_KEY_TRANSLATE_NO_DEAD_KEYS_MASK: u32 = 1;
    const COMMAND_MODIFIER_STATE: u32 = 1;

    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        fn TISCopyCurrentKeyboardLayoutInputSource() -> TisInputSourceRef;
        fn TISGetInputSourceProperty(
            input_source: TisInputSourceRef,
            property_key: CfStringRef,
        ) -> CfDataRef;
        static kTISPropertyUnicodeKeyLayoutData: CfStringRef;
        fn UCKeyTranslate(
            key_layout: *const u8,
            virtual_key_code: u16,
            key_action: u16,
            modifier_key_state: u32,
            keyboard_type: u32,
            key_translate_options: u32,
            dead_key_state: *mut u32,
            max_string_length: usize,
            actual_string_length: *mut usize,
            unicode_string: *mut u16,
        ) -> i32;
        fn LMGetKbdType() -> u8;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFDataGetBytePtr(data: CfDataRef) -> *const u8;
        fn CFRelease(value: *const c_void);
    }

    const HID_SYSTEM_STATE: i32 = 1;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventSourceFlagsState(state_id: i32) -> u64;
    }

    pub(super) fn held_modifiers() -> u64 {
        unsafe { CGEventSourceFlagsState(HID_SYSTEM_STATE) }
    }

    struct InputSource(TisInputSourceRef);

    impl Drop for InputSource {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CFRelease(self.0) };
            }
        }
    }

    fn find_keycode(mut matches: impl FnMut(u16) -> bool) -> Option<u16> {
        (0..KEYCODE_COUNT).find(|&keycode| matches(keycode))
    }

    fn resolve_command_v_keycode() -> Result<u16, String> {
        let source = InputSource(unsafe { TISCopyCurrentKeyboardLayoutInputSource() });
        if source.0.is_null() {
            return Err("macOS returned no current keyboard layout input source".into());
        }

        let layout_data =
            unsafe { TISGetInputSourceProperty(source.0, kTISPropertyUnicodeKeyLayoutData) };
        if layout_data.is_null() {
            return Err("current macOS keyboard layout has no Unicode layout data".into());
        }

        let layout = unsafe { CFDataGetBytePtr(layout_data) };
        if layout.is_null() {
            return Err("current macOS keyboard layout data is empty".into());
        }

        let keyboard_type = unsafe { LMGetKbdType() } as u32;
        let keycode = find_keycode(|keycode| {
            let mut dead_key_state = 0;
            let mut chars = [0_u16; 4];
            let mut length = 0_usize;

            let status = unsafe {
                UCKeyTranslate(
                    layout,
                    keycode,
                    UC_KEY_ACTION_DISPLAY,
                    COMMAND_MODIFIER_STATE,
                    keyboard_type,
                    UC_KEY_TRANSLATE_NO_DEAD_KEYS_MASK,
                    &mut dead_key_state,
                    chars.len(),
                    &mut length,
                    chars.as_mut_ptr(),
                )
            };

            status == 0 && length == 1 && chars[0] == u16::from(b'v')
        })
        .ok_or_else(|| "could not map Cmd+V in the current macOS keyboard layout".to_string())?;

        Ok(keycode)
    }

    pub(super) fn command_v_key() -> Key {
        match resolve_command_v_keycode() {
            Ok(keycode) => {
                debug!("Resolved Cmd+V for the active macOS layout to keycode {keycode}");
                Key::Other(u32::from(keycode))
            }
            Err(error) => {
                warn!(
                    "Could not resolve Cmd+V for the active macOS layout ({error}); using ANSI V keycode {ANSI_V_KEYCODE}"
                );
                Key::Other(u32::from(ANSI_V_KEYCODE))
            }
        }
    }
}

pub struct EnigoState(pub Slot<Enigo>);
pub struct Slot<T>(Mutex<Option<T>>);
impl<T> Slot<T> {
    pub fn new(value: T) -> Self {
        Self(Mutex::new(Some(value)))
    }
    pub fn with<R>(&self, work: impl FnOnce(&mut T) -> R) -> Option<R> {
        let mut value = self.0.lock().ok()?.take()?;
        let result = work(&mut value);
        *self.0.lock().ok()? = Some(value);
        Some(result)
    }
}

impl EnigoState {
    pub fn new() -> Result<Self, String> {
        let enigo = Enigo::new(&Settings::default())
            .map_err(|e| format!("Failed to initialize Enigo: {}", e))?;
        Ok(Self(Slot::new(enigo)))
    }
}

#[cfg(target_os = "macos")]
use macos::held_modifiers;

#[cfg(target_os = "windows")]
fn held_modifiers() -> u64 {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    [
        (VK_SHIFT, SHIFT),
        (VK_CONTROL, CONTROL),
        (VK_MENU, OPTION),
        (VK_LWIN, COMMAND),
        (VK_RWIN, COMMAND),
    ]
    .into_iter()
    .filter(|(key, _)| unsafe { GetAsyncKeyState(i32::from(key.0)) } < 0)
    .fold(0, |flags, (_, flag)| flags | flag)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn held_modifiers() -> u64 {
    0
}

pub fn modifiers_up(flags: u64) -> bool {
    flags & (SHIFT | CONTROL | OPTION | COMMAND | FN) == 0
}

pub fn wait_for_modifiers_up() {
    let started = std::time::Instant::now();
    while !modifiers_up(held_modifiers()) && started.elapsed() < std::time::Duration::from_secs(1) {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

pub fn send_paste_ctrl_v(enigo: &mut Enigo, hold_ms: u64) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let (modifier_key, v_key_code) = (Key::Meta, macos::command_v_key());
    #[cfg(target_os = "windows")]
    let (modifier_key, v_key_code) = (Key::Control, Key::Other(0x56));
    #[cfg(target_os = "linux")]
    let (modifier_key, v_key_code) = (Key::Control, Key::Unicode('v'));

    enigo
        .key(modifier_key, enigo::Direction::Press)
        .map_err(|e| format!("Failed to press modifier key: {}", e))?;
    let click = enigo
        .key(v_key_code, enigo::Direction::Click)
        .map_err(|e| format!("Failed to click V key: {}", e));

    std::thread::sleep(std::time::Duration::from_millis(hold_ms));

    let release = enigo
        .key(modifier_key, enigo::Direction::Release)
        .map_err(|e| format!("Failed to release modifier key: {}", e));

    click.and(release)
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_held_modifier_keys_hold_back_the_paste() {
        assert!(super::modifiers_up(0));
        assert!(super::modifiers_up(0x0001_0000 | 0x0000_0100 | 0x2000_0000));
        for flag in [
            super::SHIFT,
            super::CONTROL,
            super::OPTION,
            super::COMMAND,
            super::FN,
        ] {
            assert!(!super::modifiers_up(flag | 0x0001_0000));
        }
    }

    #[test]
    fn keyboard_work_runs_without_holding_the_state_lock() {
        let slot = super::Slot::new(1);
        assert_eq!(
            slot.with(|value| {
                assert!(slot.0.try_lock().is_ok());
                *value += 1;
                *value
            }),
            Some(2)
        );
        assert_eq!(slot.with(|value| *value), Some(2));
    }
}
