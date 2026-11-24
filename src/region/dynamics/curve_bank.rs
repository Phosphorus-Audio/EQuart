use super::curve::{Curve, MAX_POINTS};
use crate::region::REGION_COUNT;
use nice_plug::params::persist::PersistentField;
use std::sync::{
    Mutex,
    atomic::{AtomicU64, Ordering::SeqCst},
};

pub(crate) const FIELD: &str = "spectral_transfer_curves";

#[derive(Default)]
struct Publication {
    revision: AtomicU64,
    words: [AtomicU64; MAX_POINTS],
}

impl Publication {
    fn write(&self, clock: &AtomicU64, curve: Curve) {
        self.revision.store(1, SeqCst);

        for (slot, word) in self.words.iter().zip(curve.encode()) {
            slot.store(word, SeqCst);
        }

        let revision = 2 * (clock.fetch_add(1, SeqCst) + 1);
        self.revision.store(revision, SeqCst);
    }
}

#[derive(Default)]
pub(crate) struct CurveBank {
    clock: AtomicU64,
    editor: [Publication; REGION_COUNT],
    restored: [Publication; REGION_COUNT],
    editing: Mutex<()>,
}

impl CurveBank {
    pub(crate) fn read(&self, index: usize) -> Option<Curve> {
        let editor = &self.editor[index];
        let restored = &self.restored[index];
        let a = editor.revision.load(SeqCst);
        let b = restored.revision.load(SeqCst);

        if a == 1 || b == 1 {
            return None;
        }
        if a == 0 && b == 0 {
            return Some(Curve::default());
        }

        let publication = if a > b { editor } else { restored };
        let words = publication.words.each_ref().map(|word| word.load(SeqCst));
        let stable = a == editor.revision.load(SeqCst) && b == restored.revision.load(SeqCst);
        stable.then(|| Curve::decode(words))
    }

    pub(crate) fn edit(&self, index: usize, curve: Curve) {
        let _editing = self
            .editing
            .lock()
            .expect("transfer curve editor lock poisoned");
        self.editor[index].write(&self.clock, curve);
    }
}

impl PersistentField<'_, [Curve; REGION_COUNT]> for CurveBank {
    fn set(&self, curves: [Curve; REGION_COUNT]) {
        // state restore is serialized by the host wrapper
        for (published, curve) in self.restored.iter().zip(curves) {
            published.write(&self.clock, curve.validated());
        }
    }

    fn map<F, R>(&self, f: F) -> R
    where
        F: Fn(&[Curve; REGION_COUNT]) -> R,
    {
        let curves = std::array::from_fn(|index| {
            loop {
                if let Some(curve) = self.read(index) {
                    break curve;
                }
                std::hint::spin_loop();
            }
        });
        f(&curves)
    }
}
