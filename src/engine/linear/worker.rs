use super::{BandControl, CAPACITY, Convolution, Designer, Kernel, LinearQuality, SharedPlans};
use rtrb::{Consumer, Producer, RingBuffer};
use std::{
    io,
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

struct Job {
    bands: Vec<BandControl>,
    delta: Option<Range<usize>>,
    rate: f32,
    quality: LinearQuality,
    generation: u64,
    kernel: Option<Kernel>,
}

pub(super) struct Worker {
    requests: Producer<Job>,
    results: Consumer<Job>,
    spare: Option<Job>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Worker {
    pub(super) fn new(plans: &'static SharedPlans) -> io::Result<Self> {
        let (requests, mut incoming) = RingBuffer::<Job>::new(1);
        let (mut outgoing, results) = RingBuffer::<Job>::new(1);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);

        let thread = thread::Builder::new()
            .name("equart-fir".into())
            .spawn(move || {
                let mut work = None;

                while !stopped.load(Ordering::Acquire) {
                    let Ok(mut job) = incoming.pop() else {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    };

                    let (designer, convolution) = work.get_or_insert_with(|| {
                        (
                            Designer::new(plans),
                            Box::new(Convolution::new(&plans.convolution)),
                        )
                    });
                    let kernel = job.kernel.get_or_insert_with(Kernel::new);
                    designer.configure(&job.bands, job.delta.take(), job.rate, job.quality, kernel);
                    designer.design(2 * designer.center + 1);
                    designer.partition(convolution, kernel);

                    assert!(outgoing.push(job).is_ok(), "one FIR job may be in flight");
                }
            })?;

        Ok(Self {
            requests,
            results,
            spare: Some(Job {
                bands: Vec::with_capacity(CAPACITY),
                delta: None,
                rate: 0.0,
                quality: LinearQuality::default(),
                generation: 0,
                kernel: None,
            }),
            stop,
            thread: Some(thread),
        })
    }

    pub(super) fn submit(
        &mut self,
        bands: &[BandControl],
        delta: Option<Range<usize>>,
        rate: f32,
        quality: LinearQuality,
        generation: u64,
    ) -> bool {
        let Some(mut job) = self.spare.take() else {
            return false;
        };

        job.bands.clear();
        job.bands.extend_from_slice(bands);
        job.delta = delta;
        job.rate = rate;
        job.quality = quality;
        job.generation = generation;

        assert!(
            self.requests.push(job).is_ok(),
            "one FIR job may be in flight"
        );
        true
    }

    pub(super) fn receive(&mut self, generation: u64, next: &mut Kernel) -> bool {
        let Ok(mut job) = self.results.pop() else {
            return false;
        };
        let current = job.generation == generation;

        if current {
            let kernel = job.kernel.as_mut().expect("a completed job has a kernel");
            std::mem::swap(kernel, next);
        }

        self.spare = Some(job);
        current
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);

        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            nice_plug::nice_error!("The FIR designer stopped unexpectedly");
        }
    }
}
