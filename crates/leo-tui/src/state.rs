use super::*;

pub(super) struct Navigation {
    pub(super) current_dir: String,
    pub(super) numbering: Vec<String>,
    pub(super) filter: Option<String>,
    pub(super) recent: recent::Recent,
    pub(super) note_sel: usize,
    pub(super) marked: Vec<String>,
    pub(super) dir_sel: usize,
    pub(super) focus: Pane,
    pub(super) preview_scroll: u16,
    pub(super) answer_sources: Vec<String>,
}

impl Navigation {
    pub(super) fn new(store: &Store) -> Self {
        Self {
            current_dir: String::new(),
            numbering: action::numbering_for(store, ""),
            filter: None,
            recent: recent::Recent::load(),
            note_sel: 0,
            marked: Vec::new(),
            dir_sel: 0,
            focus: Pane::Notes,
            preview_scroll: 0,
            answer_sources: Vec::new(),
        }
    }
}

#[derive(Default)]
pub(super) struct Writing {
    pub(super) editing: Option<editor::Editor>,
    pub(super) edit_uncommitted: bool,
}

pub(super) struct Jobs {
    pub(super) asking: Option<Asking>,
    pub(super) recording: Option<Recording>,
    pub(super) last_change: Instant,
    pub(super) pushing: Option<(task::Job, view::progress::Progress, Instant)>,
    pub(super) checking: Option<(task::Job, view::progress::Progress, Instant)>,
    pub(super) update: Option<std::sync::mpsc::Receiver<String>>,
    pub(super) model_download: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    pub(super) usage_check: Option<std::sync::mpsc::Receiver<()>>,
    pub(super) last_usage_check: Option<Instant>,
    pub(super) last_disk_check: Option<Instant>,
    pub(super) last_push: Option<Instant>,
    pub(super) unpushed: Option<usize>,
    pub(super) busy: Option<(view::progress::Progress, Instant)>,
}

impl Default for Jobs {
    fn default() -> Self {
        Self {
            asking: None,
            recording: None,
            last_change: Instant::now(),
            pushing: None,
            checking: None,
            update: None,
            model_download: None,
            usage_check: None,
            last_usage_check: None,
            last_disk_check: None,
            last_push: None,
            unpushed: None,
            busy: None,
        }
    }
}
