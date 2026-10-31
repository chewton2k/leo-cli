use anyhow::{ensure, Result};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, OnceLock,
};
const INSTALLING: usize = usize::MAX;
const MOST_ACTIVITIES: usize = 256;
#[derive(Clone, Default)]
pub struct Gate(Arc<AtomicUsize>);
pub struct Lease {
    gate: Gate,
    installing: bool,
}
impl Drop for Lease {
    fn drop(&mut self) {
        if self.installing {
            self.gate.0.store(0, Ordering::Release);
        } else {
            self.gate.0.fetch_sub(1, Ordering::AcqRel);
        }
    }
}
pub fn global() -> &'static Gate {
    static GATE: OnceLock<Gate> = OnceLock::new();
    GATE.get_or_init(Gate::default)
}
impl Gate {
    pub fn begin(&self) -> Result<Lease> {
        #[allow(deprecated)]
        let admitted = self
            .0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < MOST_ACTIVITIES).then(|| n + 1)
            });
        ensure!(admitted.is_ok(),"Leo is installing an update or still finishing an operation. Try recording or dictation again when it finishes.");
        Ok(Lease {
            gate: self.clone(),
            installing: false,
        })
    }
    pub fn install(&self) -> Result<Lease> {
        ensure!(
            self.0
                .compare_exchange(0, INSTALLING, Ordering::AcqRel, Ordering::Acquire)
                .is_ok(),
            "Finish recording or dictation before installing this update."
        );
        Ok(Lease {
            gate: self.clone(),
            installing: true,
        })
    }
    pub fn installing(&self) -> bool {
        self.0.load(Ordering::Acquire) == INSTALLING
    }
    pub fn busy(&self) -> bool {
        self.0.load(Ordering::Acquire) != 0
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn capture_and_install_exclude_each_other_in_both_orders() {
        let gate = super::Gate::default();
        let first = gate.begin().unwrap();
        let second = gate.begin().unwrap();
        assert!(gate.install().is_err());
        drop(first);
        assert!(gate.install().is_err());
        drop(second);
        let install = gate.install().unwrap();
        assert!(gate.begin().is_err());
        drop(install);
        assert!(gate.begin().is_ok());
    }
    #[test]
    fn leases_are_bounded_and_release_on_error_paths() {
        let gate = super::Gate::default();
        let mut active = Vec::new();
        for _ in 0..256 {
            active.push(gate.begin().unwrap());
        }
        assert!(gate.begin().is_err());
        drop(active);
        assert!(!gate.busy());
        assert!(gate.install().is_ok());
    }
}
