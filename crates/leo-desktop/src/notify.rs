use std::time::{Duration, UNIX_EPOCH};

use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

const OFFER_FOR: Duration = Duration::from_secs(10 * 60);

pub fn listen(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.notification().on_action(move |performed| {
        let uid = performed
            .notification()
            .and_then(|shown| shown.extra().get("uid"))
            .and_then(|uid| uid.as_str());
        if let (Some(uid), "tap") = (uid, performed.action_id()) {
            crate::tray::page(&handle, "event", uid);
        }
    });
}

pub fn starting(app: &AppHandle, event: leo_web::app_settings::EventStarting) {
    let _ = app
        .notification()
        .builder()
        .title(format!("{} is starting", event.title))
        .body("Click to record the call. leo writes the notes when it ends.")
        .extra("uid", &event.uid)
        .show();
    let start = UNIX_EPOCH + Duration::from_secs(event.start.timestamp().max(0) as u64);
    crate::tray::offer(app, event.uid, event.title, start + OFFER_FOR);
}
