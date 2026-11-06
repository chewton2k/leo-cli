mod felix;
pub mod health;
mod housekeeping;
mod powers;
mod record;
mod settings;
mod speech;

pub use housekeeping::Housekeeping;
pub use powers::powers;
pub use settings::WebSettings;

pub fn stop_writing() {
    leo_services::ai::stop_writing();
}
