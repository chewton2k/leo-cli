use super::*;

struct Backing {
    calls: std::sync::atomic::AtomicUsize,
    store: Arc<Mutex<Storage>>,
    outcome: u8,
}

impl SettingsApi for Backing {
    fn describe(&self, _: &std::path::Path) -> serde_json::Value {
        serde_json::json!({})
    }

    fn apply(&self, _: &serde_json::Value) -> anyhow::Result<String> {
        anyhow::bail!("Not used")
    }

    fn test(&self, _: &str) -> anyhow::Result<String> {
        anyhow::bail!("Not used")
    }

    fn backup(&self, _: &std::path::Path) -> anyhow::Result<Option<std::process::Command>> {
        assert!(self.store.try_lock().is_ok());
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        match self.outcome {
            0 => Ok(None),
            1 => {
                let mut command = std::process::Command::new("git");
                command.arg("--version").stdout(std::process::Stdio::null());
                Ok(Some(command))
            }
            _ => anyhow::bail!("Do not expose this private configuration detail"),
        }
    }
}

#[tokio::test]
async fn leaving_coalesces_backup_requests_runs_without_the_store_lock_and_reports_outcomes() {
    for (outcome, expected) in [(0, "idle"), (1, "done"), (2, "failed")] {
        let (mut state, _dir, _) = state_with(&[]);
        let backing = Arc::new(Backing {
            calls: Default::default(),
            store: Arc::clone(&state.store),
            outcome,
        });
        state.settings = Some(backing.clone());
        for _ in 0..8 {
            assert_eq!(
                crate::backup::leave(State(state.clone()), Json(crate::backup::Leave {}))
                    .await
                    .unwrap(),
                StatusCode::NO_CONTENT
            );
        }
        let mut view = serde_json::Value::Null;
        for _ in 0..250 {
            let result = crate::backup::status(State(state.clone())).await.unwrap().0;
            view = serde_json::to_value(result).unwrap();
            if view["state"] == expected {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert_eq!(view["state"], expected);
        assert_eq!(backing.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(!view.to_string().contains("private configuration"));
        if outcome == 2 {
            assert!(view["message"].as_str().unwrap().contains("leo backup"));
        }
    }
}

#[tokio::test]
async fn a_server_without_backup_settings_accepts_leave_without_starting_work() {
    let (state, _dir, _) = state_with(&[]);
    assert_eq!(
        crate::backup::leave(State(state.clone()), Json(crate::backup::Leave {}))
            .await
            .unwrap(),
        StatusCode::NO_CONTENT
    );
    let view = serde_json::to_value(crate::backup::status(State(state)).await.unwrap().0).unwrap();
    assert_eq!(view["at"], 0);
}
