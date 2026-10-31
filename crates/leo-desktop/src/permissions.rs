#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
}
pub fn accessibility() -> bool {
    #[cfg(target_os = "macos")]
    {
        unsafe { AXIsProcessTrusted() }
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}
pub fn elevated_target() -> bool {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::{
            Foundation::CloseHandle,
            Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
            System::Threading::{OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION},
            UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
        };
        let mut pid = 0;
        GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid));
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return true;
        };
        let mut token = Default::default();
        let mut elevated = TOKEN_ELEVATION::default();
        let mut size = 0;
        let result = OpenProcessToken(process, TOKEN_QUERY, &mut token).and_then(|_| {
            GetTokenInformation(
                token,
                TokenElevation,
                Some((&mut elevated as *mut TOKEN_ELEVATION).cast()),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut size,
            )
        });
        let _ = CloseHandle(token);
        let _ = CloseHandle(process);
        result.is_err() || elevated.TokenIsElevated != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn focused_point() -> Option<(f64, f64)> {
    #[cfg(target_os = "macos")]
    unsafe {
        use std::ffi::c_void;
        #[repr(C)]
        struct Point {
            x: f64,
            y: f64,
        }
        #[link(name = "ApplicationServices", kind = "framework")]
        unsafe extern "C" {
            fn AXUIElementCreateSystemWide() -> *const c_void;
            fn AXUIElementCopyAttributeValue(
                element: *const c_void,
                attribute: *const c_void,
                value: *mut *const c_void,
            ) -> i32;
            fn AXValueGetValue(value: *const c_void, kind: i32, out: *mut c_void) -> bool;
        }
        #[link(name = "CoreFoundation", kind = "framework")]
        unsafe extern "C" {
            fn CFRelease(value: *const c_void);
        }
        struct Owned(*const c_void);
        impl Drop for Owned {
            fn drop(&mut self) {
                if !self.0.is_null() {
                    unsafe { CFRelease(self.0) };
                }
            }
        }
        fn attribute(element: *const c_void, name: &str) -> Option<Owned> {
            let name = objc2_foundation::NSString::from_str(name);
            let mut value = std::ptr::null();
            if unsafe {
                AXUIElementCopyAttributeValue(
                    element,
                    (&*name as *const objc2_foundation::NSString).cast(),
                    &mut value,
                )
            } != 0
                || value.is_null()
            {
                return None;
            }
            Some(Owned(value))
        }
        if !accessibility() {
            return None;
        }
        let system = Owned(AXUIElementCreateSystemWide());
        let app = attribute(system.0, "AXFocusedApplication")?;
        let window = attribute(app.0, "AXFocusedWindow")?;
        let position = attribute(window.0, "AXPosition")?;
        let size = attribute(window.0, "AXSize")?;
        let mut point = Point { x: 0.0, y: 0.0 };
        let mut extent = Point { x: 0.0, y: 0.0 };
        if !AXValueGetValue(position.0, 1, (&mut point as *mut Point).cast())
            || !AXValueGetValue(size.0, 2, (&mut extent as *mut Point).cast())
        {
            return None;
        }
        Some((point.x + extent.x / 2.0, point.y + extent.y / 2.0))
    }
    #[cfg(windows)]
    unsafe {
        use windows::Win32::{
            Foundation::RECT,
            UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect},
        };
        let mut rect = RECT::default();
        GetWindowRect(GetForegroundWindow(), &mut rect).ok()?;
        Some((
            (rect.left as f64 + rect.right as f64) / 2.0,
            (rect.top as f64 + rect.bottom as f64) / 2.0,
        ))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        None
    }
}

pub fn microphone() -> &'static str {
    if leo_services::session::mic::disabled() {
        return "unavailable";
    }
    #[cfg(target_os = "macos")]
    unsafe {
        use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
        let Some(media) = AVMediaTypeAudio else {
            return "unknown";
        };
        let status = AVCaptureDevice::authorizationStatusForMediaType(media);
        if status == AVAuthorizationStatus::Authorized {
            "allowed"
        } else if status == AVAuthorizationStatus::NotDetermined {
            "not_requested"
        } else {
            "denied"
        }
    }
    #[cfg(windows)]
    unsafe {
        use windows::{
            Devices::Enumeration::{DeviceAccessInformation, DeviceAccessStatus, DeviceClass},
            Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED},
        };
        let initialized = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
        let status = DeviceAccessInformation::CreateFromDeviceClass(DeviceClass::AudioCapture)
            .and_then(|info| info.CurrentStatus());
        if initialized {
            CoUninitialize();
        }
        match status {
            Ok(DeviceAccessStatus::Allowed) => "allowed",
            Ok(DeviceAccessStatus::DeniedByUser | DeviceAccessStatus::DeniedBySystem) => "denied",
            _ => "unknown",
        }
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        "unknown"
    }
}
pub fn request_microphone(app: &tauri::AppHandle) -> anyhow::Result<()> {
    anyhow::ensure!(
        !leo_services::session::mic::disabled(),
        "Microphone capture is disabled in this session."
    );
    #[cfg(target_os = "macos")]
    app.run_on_main_thread(|| unsafe {
        use objc2_av_foundation::{AVCaptureDevice, AVMediaTypeAudio};
        if let Some(media) = AVMediaTypeAudio {
            let callback = block2::RcBlock::new(|_: objc2::runtime::Bool| {});
            AVCaptureDevice::requestAccessForMediaType_completionHandler(media, &callback);
        }
    })?;
    #[cfg(windows)]
    {
        let _ = app;
        open_privacy("ms-settings:privacy-microphone")?;
    }
    Ok(())
}
pub fn request_accessibility(_app: &tauri::AppHandle) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    tauri::async_runtime::block_on(
        tauri_plugin_macos_permissions::request_accessibility_permission(),
    );
    Ok(())
}
#[cfg(windows)]
fn open_privacy(uri: &str) -> anyhow::Result<()> {
    use windows::{
        core::PCWSTR,
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    };
    let uri: Vec<u16> = uri.encode_utf16().chain(std::iter::once(0)).collect();
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR::null(),
            PCWSTR(uri.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    anyhow::ensure!(result.0 as usize>32,"Windows privacy settings could not be opened. Open Settings → Privacy & security → Microphone.");
    Ok(())
}

pub fn shortcut_clash(shortcut: &str) -> bool {
    let value = shortcut.to_ascii_lowercase();
    #[cfg(target_os = "macos")]
    {
        use std::ffi::OsStr;
        if !["option+space", "alt+space", "opt+space"].contains(&value.as_str()) {
            return false;
        }
        crate::helpers::capture(
            "pgrep",
            &[OsStr::new("-f"), OsStr::new("/Alfred[^/]*\\.app/")],
            std::time::Duration::from_secs(1),
        )
        .is_ok_and(|output| output.status.success())
    }
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::{
            Input::KeyboardAndMouse::GetKeyboardLayout,
            WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
        };
        if !["ctrl+space", "control+space"].contains(&value.as_str()) {
            return false;
        }
        let thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let language = (GetKeyboardLayout(thread).0 as usize) & 0x3ff;
        matches!(language, 4 | 17 | 18)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = value;
        false
    }
}
