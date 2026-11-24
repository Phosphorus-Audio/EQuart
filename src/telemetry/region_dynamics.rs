use crate::region::dynamics::{MaskResponse, Meter};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) const HZ: usize = 60;
pub(crate) const HISTORY: usize = 4 * HZ;
const STAMP: u64 = (1 << 24) - 1;

#[derive(Default)]
pub(crate) struct Response {
    stamp: AtomicU64,
    high: AtomicU64,
    gains: [AtomicU64; 4 * crate::region::REGION_COUNT],
    edges: [AtomicU64; 4 * crate::region::REGION_COUNT],
}

impl Response {
    pub(crate) fn write(&self, response: MaskResponse) {
        use Ordering::SeqCst;

        let stamp = self.stamp.load(SeqCst);
        let active = response.active();
        if !active && stamp & 2 == 0 {
            return;
        }

        self.stamp.store(stamp | 1, SeqCst);
        if active {
            self.high.store(pack_stereo(response.high), SeqCst);
            for ((gain, edge), (g, stereo)) in
                self.gains.iter().zip(&self.edges).zip(response.edges)
            {
                gain.store(pack_stereo(stereo), SeqCst);
                edge.store(u64::from(g.to_bits()), SeqCst);
            }
        }
        self.stamp.store(
            (stamp & !3).wrapping_add(4) | (u64::from(active) << 1),
            SeqCst,
        );
    }

    pub(crate) fn read_into(&self, response: &mut MaskResponse) {
        use Ordering::SeqCst;

        let stamp = self.stamp.load(SeqCst);
        if stamp & 1 != 0 {
            return;
        }
        if stamp & 2 == 0 {
            *response = MaskResponse::default();
            return;
        }

        let next = MaskResponse {
            high: unpack_stereo(self.high.load(SeqCst)),
            edges: std::array::from_fn(|index| {
                (
                    f32::from_bits(self.edges[index].load(SeqCst) as u32),
                    unpack_stereo(self.gains[index].load(SeqCst)),
                )
            }),
        };
        if stamp == self.stamp.load(SeqCst) {
            *response = next;
        }
    }
}

fn pack_stereo([left, right]: [f32; 2]) -> u64 {
    (u64::from(left.to_bits()) << 32) | u64::from(right.to_bits())
}

fn unpack_stereo(stereo: u64) -> [f32; 2] {
    [
        f32::from_bits((stereo >> 32) as u32),
        f32::from_bits(stereo as u32),
    ]
}

pub(crate) struct History {
    head: AtomicU64,
    latest: AtomicU64,
    samples: Box<[AtomicU64]>,
}

impl Default for History {
    fn default() -> Self {
        Self {
            head: AtomicU64::new(0),
            latest: AtomicU64::new(0),
            samples: (0..HISTORY).map(|_| AtomicU64::new(0)).collect(),
        }
    }
}

impl History {
    pub(crate) fn reset(&self) {
        self.head.store(0, Ordering::Relaxed);
        self.latest.store(0, Ordering::Relaxed);
        for sample in &self.samples {
            sample.store(0, Ordering::Relaxed);
        }
    }

    pub(crate) fn write(&self, frame: u64, rate: f32, meter: Meter, record: bool) {
        let tick = 1 + (frame as f64 * HZ as f64 / f64::from(rate)) as u64;
        let pack = |db: f32| ((db + 96.0) * 32.0).round().clamp(0.0, 4095.0) as u64;
        let packed = ((tick & STAMP) << 40)
            | (u64::from(meter.external_missing) << 36)
            | (pack(meter.input) << 24)
            | (pack(meter.output) << 12)
            | pack(meter.reduction);

        self.latest.store(packed, Ordering::Release);
        if record {
            self.samples[tick as usize % HISTORY].store(packed, Ordering::Release);
            self.head.store(tick, Ordering::Release);
        }
    }

    pub(crate) fn latest(&self) -> Option<Meter> {
        let packed = self.latest.load(Ordering::Acquire);
        (packed != 0).then(|| Self::unpack(packed))
    }

    pub(crate) fn read(&self) -> impl Iterator<Item = (usize, Meter)> + '_ {
        let head = self.head.load(Ordering::Acquire);
        (0..HISTORY).filter_map(move |i| {
            let ticks_ago = HISTORY - 1 - i;
            Some((i, self.at(head.checked_sub(ticks_ago as u64)?)?))
        })
    }

    fn at(&self, tick: u64) -> Option<Meter> {
        let packed = self.samples[tick as usize % HISTORY].load(Ordering::Acquire);
        if tick == 0 || packed == 0 || packed >> 40 != tick & STAMP {
            return None;
        }

        Some(Self::unpack(packed))
    }

    fn unpack(packed: u64) -> Meter {
        let unpack = |shift: u32| ((packed >> shift) & 4095_u64) as f32 / 32.0 - 96.0;
        Meter {
            input: unpack(24),
            output: unpack(12),
            reduction: unpack(0),
            external_missing: (packed >> 36) & 1 != 0,
        }
    }
}
