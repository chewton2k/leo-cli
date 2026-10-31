mod felix;
mod powers;
mod record;
mod settings;
mod storage;

pub use powers::powers;
pub use settings::WebSettings;
pub use storage::Housekeeping;

pub fn stop_writing() {
    leo_services::ai::stop_writing();
}
