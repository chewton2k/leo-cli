pub struct Speech;

impl leo_web::speech::SpeechApi for Speech {
    fn view(&self) -> anyhow::Result<serde_json::Value> {
        leo_services::speech::library::library()?.view()
    }
    fn change(&self, change: serde_json::Value) -> anyhow::Result<serde_json::Value> {
        leo_services::speech::library::library()?.change(&change)
    }
}
