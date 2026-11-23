use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};
use std::time::Instant;

pub const CAPTURE: usize = 0;
pub const FFT: usize = 1;
pub const SMOOTH: usize = 2;
pub const EXCHANGE: usize = 3;
pub const DRAW: usize = 4;
pub const FRAME: usize = 5;
pub const READ: usize = 6;
pub const HEATMAP: usize = 7;
const STAGES: usize = 8;
const BUCKET_NS: u64 = 10_000;
const BUCKETS: usize = 2048;

static SUM: [AtomicU64; STAGES] = [const { AtomicU64::new(0) }; STAGES];
static COUNT: [AtomicU64; STAGES] = [const { AtomicU64::new(0) }; STAGES];
static MAX: [AtomicU64; STAGES] = [const { AtomicU64::new(0) }; STAGES];
static FRAME_TIMES: [AtomicU64; BUCKETS] = [const { AtomicU64::new(0) }; BUCKETS];
static MODE: AtomicU32 = AtomicU32::new(0);
static FPS: AtomicU32 = AtomicU32::new(0);

#[unsafe(no_mangle)]
extern "C" fn equart_analyzer_profile(mode: u32, fps: u32) {
    MODE.store(mode, Relaxed);
    FPS.store(fps, Relaxed);
    for cells in [&SUM, &COUNT, &MAX] {
        for cell in cells {
            cell.store(0, Relaxed);
        }
    }
    for bucket in &FRAME_TIMES {
        bucket.store(0, Relaxed);
    }
}

#[unsafe(no_mangle)]
extern "C" fn equart_analyzer_metric(stage: usize, column: usize) -> u64 {
    match column {
        0 => COUNT[stage].load(Relaxed),
        1 => SUM[stage].load(Relaxed),
        3 | 4 if stage == FRAME => {
            let percent = if column == 3 { 95 } else { 99 };
            let count = COUNT[FRAME].load(Relaxed);
            if count == 0 {
                return 0;
            }

            let target = (count * percent).div_ceil(100);
            let mut cumulative = 0;

            for (i, bucket) in FRAME_TIMES.iter().enumerate() {
                cumulative += bucket.load(Relaxed);
                if cumulative >= target && i < BUCKETS - 1 {
                    return (i as u64 + 1) * BUCKET_NS;
                }
            }

            MAX[FRAME].load(Relaxed)
        }
        _ => MAX[stage].load(Relaxed),
    }
}

pub fn enabled(saved: bool) -> bool {
    match MODE.load(Relaxed) {
        1 => false,
        2 => true,
        _ => saved,
    }
}

pub fn fps() -> Option<u32> {
    let fps = FPS.load(Relaxed);
    (fps != 0).then_some(fps)
}


pub struct Timer(usize, Option<Instant>);

impl Timer {
    pub fn new(stage: usize) -> Self {
        Self(stage, (MODE.load(Relaxed) != 0).then(Instant::now))
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        if let Some(start) = self.1 {
            let ns = start.elapsed().as_nanos() as u64;
            SUM[self.0].fetch_add(ns, Relaxed);
            COUNT[self.0].fetch_add(1, Relaxed);
            MAX[self.0].fetch_max(ns, Relaxed);
            if self.0 == FRAME {
                let bucket = ((ns / BUCKET_NS) as usize).min(BUCKETS - 1);
                FRAME_TIMES[bucket].fetch_add(1, Relaxed);
            }
        }
    }
}
