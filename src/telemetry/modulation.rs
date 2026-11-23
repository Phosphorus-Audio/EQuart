use crate::modulation::Live;
use std::sync::atomic::{AtomicU32, Ordering, fence};

#[derive(Default)]
pub(super) struct Published {
    revision: AtomicU32,
    values: [AtomicU32; 9],
}

impl Published {
    pub(super) fn write(&self, data: Option<Live>) {
        let revision = self.revision.load(Ordering::Relaxed);
        if data.is_none() && revision & 2 == 0 {
            return;
        }

        self.revision.store(revision | 1, Ordering::Relaxed);
        fence(Ordering::Release);

        if let Some(data) = data {
            for (slot, value) in self.values.iter().zip([
                data.frequency,
                data.gain,
                data.q,
                data.mapping_input,
                data.mapping_base,
                data.phase,
                data.lfo_offset,
                data.random,
                data.lfo_base,
            ]) {
                slot.store(value.to_bits(), Ordering::Relaxed);
            }
        }

        let next = (revision & !3).wrapping_add(4) | (u32::from(data.is_some()) << 1);
        self.revision.store(next, Ordering::Release);
    }

    pub(super) fn read(&self) -> Option<Live> {
        let revision = self.revision.load(Ordering::Acquire);
        if revision & 3 != 2 {
            return None;
        }

        let [
            frequency,
            gain,
            q,
            mapping_input,
            mapping_base,
            phase,
            lfo_offset,
            random,
            lfo_base,
        ] = self
            .values
            .each_ref()
            .map(|slot| f32::from_bits(slot.load(Ordering::Relaxed)));
        fence(Ordering::Acquire);

        (self.revision.load(Ordering::Relaxed) == revision).then_some(Live {
            frequency,
            gain,
            q,
            mapping_input,
            mapping_base,
            phase,
            lfo_offset,
            random,
            lfo_base,
        })
    }
}
