use std::path::{Component, Path};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

pub struct Watch {
    _watcher: RecommendedWatcher,
    touched: Arc<AtomicBool>,
    broken: Arc<AtomicBool>,
}

impl Watch {
    pub fn start(notes_dir: &Path) -> Option<Watch> {
        let touched = Arc::new(AtomicBool::new(false));
        let broken = Arc::new(AtomicBool::new(false));
        let root = notes_dir.to_path_buf();
        let (seen, failed) = (Arc::clone(&touched), Arc::clone(&broken));
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| match event {
                Ok(event) => {
                    if event.paths.iter().any(|path| counts(&root, path)) {
                        seen.store(true, Ordering::Relaxed);
                    }
                }
                Err(_) => {
                    failed.store(true, Ordering::Relaxed);
                    seen.store(true, Ordering::Relaxed);
                }
            })
            .ok()?;
        watcher.watch(notes_dir, RecursiveMode::Recursive).ok()?;
        Some(Watch {
            _watcher: watcher,
            touched,
            broken,
        })
    }

    pub fn take(&self) -> bool {
        self.touched.swap(false, Ordering::Relaxed)
    }

    pub fn broken(&self) -> bool {
        self.broken.load(Ordering::Relaxed)
    }
}

fn counts(root: &Path, path: &Path) -> bool {
    let Ok(inside) = path.strip_prefix(root) else {
        return true;
    };
    !inside.components().any(|part| match part {
        Component::Normal(name) => name.to_string_lossy().starts_with('.'),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_folders_inside_the_notes_do_not_count_and_everything_else_does() {
        let root = Path::new("/notes");
        assert!(counts(root, Path::new("/notes/Ideas.md")));
        assert!(counts(root, Path::new("/notes/work/Plan.md")));
        assert!(counts(root, Path::new("/notes/work")));
        assert!(counts(root, Path::new("/elsewhere/Plan.md")));
        assert!(!counts(root, Path::new("/notes/.git/index")));
        assert!(!counts(root, Path::new("/notes/.trash/Old.md")));
        assert!(!counts(
            root,
            Path::new("/notes/work/.obsidian/workspace.json")
        ));
    }
}
