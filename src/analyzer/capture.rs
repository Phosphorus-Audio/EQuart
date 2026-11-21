use rtrb::{Consumer, Producer, RingBuffer, chunks::WriteChunk};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU32, Ordering},
};

use super::{Analyzer, AnalyzerSettings, SpectrumFrame};

const CAPACITY: usize = 32_768;
pub(crate) type Frame = [[f32; 2]; 2];

pub(crate) struct Publisher {
    writer: Producer<Frame>,
    epoch: Arc<AtomicU32>,
    active_rate: f32,
    gap: bool,
}

pub(crate) struct Receiver {
    reader: Mutex<Consumer<Frame>>,
    epoch: Arc<AtomicU32>,
}

pub(crate) fn channel() -> (Publisher, Receiver) {
    let (writer, reader) = RingBuffer::new(CAPACITY);
    let epoch = Arc::new(AtomicU32::new(0));
    (
        Publisher {
            writer,
            epoch: epoch.clone(),
            active_rate: 0.0,
            gap: false,
        },
        Receiver {
            reader: Mutex::new(reader),
            epoch,
        },
    )
}

impl Publisher {
    pub(crate) fn reset(&mut self) {
        self.active_rate = 0.0;
        self.gap = false;
        self.epoch.fetch_add(1, Ordering::Release);
    }

    pub(crate) fn reserve(&mut self, rate: f32, frames: usize) -> Option<WriteChunk<'_, Frame>> {
        if self.active_rate != rate {
            self.active_rate = rate;
            self.gap = false;
            self.epoch.fetch_add(1, Ordering::Release);
        }

        let slots = self.writer.slots();
        let count = slots.min(frames);
        if count == 0 {
            self.gap |= frames != 0;
            return None;
        }

        if self.gap {
            self.epoch.fetch_add(1, Ordering::Release);
        }
        self.gap = count < frames;
        Some(
            self.writer
                .write_chunk(count)
                .expect("reader can only free slots"),
        )
    }

    pub(crate) fn suspend(&mut self) {
        if self.active_rate != 0.0 {
            self.reset();
        }
    }
}

#[derive(Default)]
pub(crate) struct DisplaySpectrum {
    analyzers: Option<[Analyzer; 2]>,
    visible: bool,
    epoch: u32,
}

impl DisplaySpectrum {
    pub(crate) fn frame(&self) -> SpectrumFrame<'_> {
        match &self.analyzers {
            Some(analyzers) if self.visible => SpectrumFrame::from_analyzers(analyzers.each_ref()),
            _ => SpectrumFrame::default(),
        }
    }

    pub(crate) fn update(
        &mut self,
        receiver: &Receiver,
        rate: f32,
        settings: AnalyzerSettings,
        enabled: bool,
    ) {
        #[cfg(feature = "analyzer-profile")]
        let read_timer = super::profile::Timer::new(super::profile::READ);
        // the consumer lock is only shared by editor instances
        let mut reader = receiver.reader.lock().unwrap();
        let epoch = receiver.epoch.load(Ordering::Acquire);
        let count = reader.slots();
        let chunk = reader
            .read_chunk(count)
            .expect("writer cannot remove frames");

        if !enabled || rate == 0.0 || self.epoch != epoch {
            chunk.commit_all();
            let reset = self.epoch != epoch || self.visible;
            self.epoch = epoch;

            if reset && let Some(analyzers) = &mut self.analyzers {
                for analyzer in analyzers {
                    analyzer.reset();
                }
            }

            self.visible = false;
            return;
        }

        let analyzers = self
            .analyzers
            .get_or_insert_with(|| std::array::from_fn(|_| Analyzer::new(rate)));
        for analyzer in analyzers.iter_mut() {
            analyzer.retune(rate);
            analyzer.apply_settings(settings);
        }

        let (first, second) = chunk.as_slices();
        for &[input, output] in first.iter().chain(second) {
            analyzers[0].ingest(input);
            analyzers[1].ingest(output);
        }
        chunk.commit_all();
        drop(reader);
        #[cfg(feature = "analyzer-profile")]
        drop(read_timer);

        for analyzer in analyzers.iter_mut() {
            analyzer.refresh();
        }

        self.visible = receiver.epoch.load(Ordering::Acquire) == epoch;
    }
}
