//! The window's frame clock.
//!
//! Frames belong on the display's own cadence: some whole number of refreshes
//! apart, always the same number. The only clock on Windows that is both
//! precise and in step with the monitor is `DwmFlush`, which returns once per
//! composition -- 4.17 ms on a 240 Hz panel, where a window timer is quantised
//! to 15.6 ms and misses that tick every dozen frames or so.
//!
//! Which is why a request is counted in refreshes rather than measured against
//! a clock. `now + 16.667 ms` lands wherever in the refresh it happens to
//! fall, so the frame after it is four refreshes away one time and five the
//! next, and a picture that advances by four and then five looks like it is
//! stumbling. Asking for four refreshes gives four, every time.
//!
//! Waiting on them has to happen somewhere, and the message thread is the
//! wrong place: everything the window does -- input, host messages, the paint
//! itself -- is dispatched there, and in a plugin that thread belongs to the
//! host. A wait of one refresh per frame is enough to make the host's whole UI
//! feel dead, and one of sixteen milliseconds at sixty frames a second is
//! worse. So the waiting happens here, and the message thread only ever says
//! how soon it wants a frame.

use crate::wrappers::win32::window::{HWnd, PostMessageExt, SyncHwnd};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Hands out frames on the display's clock.
pub(crate) struct FramePacer {
    shared: Arc<PacerShared>,
    thread: Option<JoinHandle<()>>,
}

struct PacerShared {
    state: Mutex<State>,
    /// Signalled when a frame is asked for, and when the pacer is being shut
    /// down while it sleeps.
    wake: Condvar,
    hwnd: SyncHwnd,
    stopped: AtomicBool,
    /// How long one refresh lasts, as last measured, in nanoseconds. Zero
    /// until two compositions have been seen.
    refresh_nanos: AtomicU64,
}

#[derive(Default)]
struct State {
    /// Refreshes still owed before the next frame may be painted, or `None`
    /// when nobody is waiting for one. Requests keep the smallest: a burst --
    /// the audio thread, egui, an input event -- still only buys one frame.
    owed: Option<u32>,
    /// The last refresh boundary a frame was posted on. Requests are counted
    /// from here rather than from when they arrive, so the frames stay on the
    /// same grid however long the paint itself took.
    posted: Option<Instant>,
}

impl FramePacer {
    pub fn new(hwnd: HWnd) -> Self {
        let shared = Arc::new(PacerShared {
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
            hwnd: hwnd.into(),
            stopped: AtomicBool::new(false),
            refresh_nanos: AtomicU64::new(0),
        });
        let thread = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("baseview-frame-pacer".to_owned())
                .spawn(move || pace(&shared))
                .ok()
        };
        Self { shared, thread }
    }

    /// Ask for a frame once `duration` has passed. Returns at once; nothing is
    /// waited for on this thread.
    ///
    /// The duration is rounded to whole refreshes, and never fewer than one: a
    /// frame cannot be shown sooner than the next composition, and a request
    /// for less than that is asking to paint twice inside one refresh, which
    /// only ever looks worse.
    pub fn request(&self, duration: Duration) {
        let refresh = self.shared.refresh();
        let wanted = refreshes_for(refresh, duration);

        let mut state = self.shared.state();
        // The caller means "a frame's worth after the last one", not "a
        // frame's worth after I got round to asking": the paint that followed
        // the last frame has already spent part of the interval, and counting
        // it twice is what turns an even sixty into a stumble.
        let spent = state.spent_refreshes(refresh, Instant::now());
        let owed = wanted.saturating_sub(spent).max(1);
        if merge(state.owed, owed) {
            state.owed = Some(owed);
            drop(state);
            self.shared.wake.notify_one();
        }
    }

    /// Stop handing out frames. The thread notices within a refresh.
    pub fn shutdown(&self) {
        self.shared.stopped.store(true, Ordering::Release);
        self.shared.wake.notify_all();
    }
}

impl Drop for FramePacer {
    fn drop(&mut self) {
        self.shutdown();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl PacerShared {
    fn state(&self) -> MutexGuard<'_, State> {
        // A panic in the pacing thread leaves the count as it was; the next
        // request can go on using it.
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    /// The refresh period, or sixty hertz until one has been measured. The
    /// error lasts the first couple of frames only, and a request rounded to
    /// the wrong refresh is still a frame at about the right time.
    fn refresh(&self) -> Duration {
        match self.refresh_nanos.load(Ordering::Relaxed) {
            0 => SIXTY_HERTZ,
            nanos => Duration::from_nanos(nanos),
        }
    }
}

impl State {
    /// How many refreshes have gone by since the last frame was posted.
    fn spent_refreshes(&self, refresh: Duration, now: Instant) -> u32 {
        let Some(posted) = self.posted else { return 0 };
        let elapsed = now.saturating_duration_since(posted).as_secs_f64();
        (elapsed / refresh.as_secs_f64()).floor() as u32
    }
}

/// The thread: sleep until a frame is wanted, count the refreshes out, post.
fn pace(shared: &PacerShared) {
    'frame: loop {
        // Until somebody wants a frame there is nothing to count down to, and
        // the thread costs nothing at all.
        {
            let mut state = shared.state();
            loop {
                if shared.stopped() {
                    return;
                }
                if state.owed.is_some() {
                    break;
                }
                state = shared.wake.wait(state).unwrap_or_else(|p| p.into_inner());
            }
        }

        loop {
            if shared.stopped() {
                return;
            }
            wait_for_composition(shared);

            let mut state = shared.state();
            match state.owed {
                // Somebody else painted the frame this count was for, or the
                // window was told to stop drawing. Back to waiting.
                None => continue 'frame,
                Some(1) => {
                    state.owed = None;
                    state.posted = Some(Instant::now());
                    drop(state);
                    shared.hwnd.post_frame_due();
                    continue 'frame;
                }
                Some(left) => state.owed = Some(left - 1),
            }
        }
    }
}

/// Wait out one display refresh, and note how long it turned out to be.
fn wait_for_composition(shared: &PacerShared) {
    use windows_sys::Win32::Graphics::Dwm::DwmFlush;

    let before = Instant::now();
    if unsafe { DwmFlush() } != 0 {
        // No compositor to wait for. A sixty hertz tick is as close to a
        // frame as such a system is going to give.
        std::thread::sleep(SIXTY_HERTZ);
    }
    let elapsed = before.elapsed();

    // A compositor that has gone away reports back at once -- microseconds --
    // and a window that was occluded for a while in seconds; a flush that was
    // itself descheduled is just as telling. None of those is a refresh rate,
    // and a rate taken from one of them would round every later request to
    // the wrong number of refreshes. Anything between a kilohertz and ten
    // hertz is a real refresh and is believed.
    if elapsed < MIN_REFRESH || elapsed > MAX_REFRESH {
        return;
    }
    // Smoothed, because one flush is one sample and the count of refreshes a
    // request is worth is a round(): a single short sample would tip sixteen
    // milliseconds from four refreshes to five, and the frame rate would
    // wander by twenty percent. Four frames' worth of history keeps the
    // answer steady and still follows a window dragged to another monitor.
    let nanos = elapsed.as_nanos() as u64;
    let previous = shared.refresh_nanos.load(Ordering::Relaxed);
    let smoothed = if previous == 0 {
        nanos
    } else {
        (previous * 3 + nanos) / 4
    };
    shared.refresh_nanos.store(smoothed, Ordering::Relaxed);
}

/// What a refresh is taken to be before one has been measured -- and what to
/// fall back on when the compositor cannot be waited for at all.
const SIXTY_HERTZ: Duration = Duration::from_micros(16_667);

/// The shortest and longest flush that count as one refresh. A thousand hertz
/// at the top end of the range keeps every plausible panel inside it; ten
/// hertz at the bottom leaves out a stagger rather than a slow display.
const MIN_REFRESH: Duration = Duration::from_millis(1);
const MAX_REFRESH: Duration = Duration::from_millis(100);

/// How many refreshes a request for `duration` is worth, given the display's
/// refresh period.
fn refreshes_for(refresh: Duration, duration: Duration) -> u32 {
    let refreshes = (duration.as_secs_f64() / refresh.as_secs_f64()).round();
    (refreshes.max(1.0) as u32).max(1)
}

/// Whether a request for `wanted` refreshes has to be recorded, given what is
/// already owed. The earliest frame wins: two frames due at different times
/// are still one frame, painted at the first of them.
fn merge(owed: Option<u32>, wanted: u32) -> bool {
    owed.is_none_or(|existing| wanted < existing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_earliest_request_wins() {
        assert!(merge(None, 4), "an empty slot takes any request");
        assert!(
            merge(Some(4), 1),
            "a frame asked for sooner must not wait for the one already owed"
        );
        assert!(
            !merge(Some(1), 4),
            "a frame asked for later must not pull the due one back"
        );
        assert!(
            !merge(Some(4), 4),
            "and a second request for the same refresh adds nothing"
        );
    }

    /// Requests are rounded to whole refreshes, so sixteen and two thirds of a
    /// millisecond is four refreshes on a 240 Hz panel -- exactly sixty frames
    /// a second, evenly -- and never zero, which would be a second paint
    /// inside the refresh already being composed.
    #[test]
    fn a_duration_becomes_a_whole_number_of_refreshes() {
        let fast = Duration::from_nanos(4_167_000); // 240 Hz
        let sixty = Duration::from_nanos(16_667_000);

        assert_eq!(refreshes_for(fast, Duration::ZERO), 1, "never inside a refresh");
        assert_eq!(
            refreshes_for(fast, Duration::from_micros(16_667)),
            4,
            "sixty a second on a 240 Hz panel is every fourth refresh"
        );
        assert_eq!(refreshes_for(fast, Duration::from_millis(120)), 29);

        assert_eq!(refreshes_for(sixty, Duration::ZERO), 1);
        assert_eq!(refreshes_for(sixty, Duration::from_micros(16_667)), 1);
        assert_eq!(refreshes_for(sixty, Duration::from_micros(33_333)), 2);
    }
}
