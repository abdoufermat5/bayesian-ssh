//! Per-session output channel: the chunk wire type, backpressure and sink.

use serde::Serialize;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use tauri::ipc::Channel;

/// One batch of PTY output delivered on a session's output channel.
///
/// `bytes` is the UTF-8 byte length of `data`: the frontend acks exactly those
/// bytes once xterm has consumed the chunk, which is what releases the reader
/// thread's backpressure budget.
#[derive(Clone, Debug, Serialize)]
pub struct PtyChunk {
    pub data: String,
    pub bytes: usize,
}

/// Output the reader thread may hand to a channel before it parks and waits
/// for the frontend to catch up.
pub(super) const OUTPUT_HIGH_WATER_BYTES: usize = 1024 * 1024;
/// Once acks bring the outstanding bytes down to this, the reader resumes.
pub(super) const OUTPUT_LOW_WATER_BYTES: usize = 256 * 1024;
/// How often a parked reader wakes up to re-check for cancellation, so a
/// closed session can never leave it blocked forever.
pub(super) const OUTPUT_WAIT_POLL: Duration = Duration::from_millis(250);

/// Byte counter behind the output channel's flow control.
///
/// The reader records what it handed to the frontend, `ack_pty` subtracts what
/// xterm has consumed, and the reader parks once the outstanding amount reaches
/// [`OUTPUT_HIGH_WATER_BYTES`], resuming when it is back at
/// [`OUTPUT_LOW_WATER_BYTES`]. Deliberately free of Tauri types so the
/// thresholds are unit-testable.
#[derive(Default)]
pub struct FlowControl {
    state: Mutex<FlowState>,
    wake: Condvar,
}

#[derive(Default)]
pub(super) struct FlowState {
    /// Whether a sink is installed; without one nothing is counted or waited.
    attached: bool,
    /// Bytes sent on the sink that the frontend has not acked yet.
    unacked: usize,
}

impl FlowControl {
    fn lock(&self) -> std::sync::MutexGuard<'_, FlowState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A sink was installed (or replaced): the new consumer has not received
    /// any of the outstanding bytes, so the budget starts from zero again.
    fn attach(&self) {
        let mut state = self.lock();
        state.attached = true;
        state.unacked = 0;
        drop(state);
        self.wake.notify_all();
    }

    /// The sink went away (dock/detach/close/dead channel): stop counting and
    /// release a parked reader.
    fn detach(&self) {
        let mut state = self.lock();
        state.attached = false;
        state.unacked = 0;
        drop(state);
        self.wake.notify_all();
    }

    /// Account for a chunk handed to the sink. Without a sink nothing is
    /// counted, mirroring the fact that no wait can happen either.
    fn record(&self, bytes: usize) {
        let mut state = self.lock();
        if !state.attached {
            return;
        }
        state.unacked = state.unacked.saturating_add(bytes);
    }

    /// The frontend processed `bytes`: release that much budget.
    pub(super) fn ack(&self, bytes: usize) {
        let mut state = self.lock();
        state.unacked = state.unacked.saturating_sub(bytes);
        drop(state);
        self.wake.notify_all();
    }

    #[cfg(test)]
    fn unacked(&self) -> usize {
        self.lock().unacked
    }

    /// Whether the consumer is far enough behind to stop the reader: only a
    /// full high-water mark triggers a pause, so a single chunk above the low
    /// mark still goes through.
    fn congested(&self) -> bool {
        let state = self.lock();
        state.attached && state.unacked >= OUTPUT_HIGH_WATER_BYTES
    }

    /// Park while the consumer is behind, re-checking [`OUTPUT_WAIT_POLL`] so a
    /// cancelled session or a cleared sink never blocks the reader forever.
    pub(super) fn wait_while_congested(&self, stop: impl Fn() -> bool) {
        if !self.congested() {
            return;
        }
        loop {
            if stop() {
                return;
            }
            let state = self.lock();
            if !state.attached || state.unacked <= OUTPUT_LOW_WATER_BYTES {
                return;
            }
            // Nothing panics while the lock is held, so a poisoned mutex only
            // means "woken up": keep the guard and re-check the condition.
            let state = self
                .wake
                .wait_timeout(state, OUTPUT_WAIT_POLL)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
            drop(state);
        }
    }
}

/// Per-session output sink: the channel currently attached (the main window or
/// one pop-out window) plus its flow-control counter.
///
/// Cloning is cheap and shares the same sink, so the reader thread and the
/// commands always see the same channel. `None` means nobody is listening:
/// output then only lands in the replay buffer and the recorder.
#[derive(Clone, Default)]
pub struct OutputSink(Arc<OutputSinkInner>);

#[derive(Default)]
pub(super) struct OutputSinkInner {
    channel: Mutex<Option<Channel<PtyChunk>>>,
    flow: FlowControl,
}

impl OutputSink {
    pub fn new(channel: Channel<PtyChunk>) -> Self {
        let sink = Self::default();
        sink.set(channel);
        sink
    }

    fn lock_channel(&self) -> std::sync::MutexGuard<'_, Option<Channel<PtyChunk>>> {
        self.0
            .channel
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Install (or replace) the channel.
    pub fn set(&self, channel: Channel<PtyChunk>) {
        *self.lock_channel() = Some(channel);
        self.0.flow.attach();
    }

    /// Stop delivering: the session was docked, detached, reopened in a pop-out
    /// window, closed, or its channel died. Returns the channel that was
    /// attached, so a caller whose handover failed can put it back.
    pub fn take(&self) -> Option<Channel<PtyChunk>> {
        let previous = self.lock_channel().take();
        self.0.flow.detach();
        previous
    }

    /// Stop delivering and drop the channel.
    pub fn clear(&self) {
        let _ = self.take();
    }

    /// Hand one chunk to the attached channel. No sink means the chunk only
    /// lives in the replay buffer.
    pub(super) fn send(&self, data: &str) {
        // The channel mutex is held across the send so a concurrent `set`/
        // `clear` cannot make us drop the channel that replaced this one.
        let mut channel = self.lock_channel();
        let Some(sink) = channel.as_ref() else {
            return;
        };
        let chunk = PtyChunk {
            data: data.to_string(),
            bytes: data.len(),
        };
        // Count before sending: an ack for this chunk may arrive while the
        // send is still in flight, and it must not be lost under the counter.
        self.0.flow.record(chunk.bytes);
        if sink.send(chunk).is_err() {
            // The webview is gone (window closed or reloading): drop the sink
            // so the reader stops accounting against a dead channel.
            *channel = None;
            self.0.flow.detach();
        }
    }

    /// The frontend consumed `bytes`.
    pub(super) fn ack(&self, bytes: usize) {
        self.0.flow.ack(bytes);
    }

    /// Park while the frontend is behind on this sink (see [`FlowControl`]).
    pub(super) fn wait_while_congested(&self, stop: impl Fn() -> bool) {
        self.0.flow.wait_while_congested(stop);
    }
}

#[cfg(test)]
mod tests {
    use super::{FlowControl, OUTPUT_HIGH_WATER_BYTES, OUTPUT_LOW_WATER_BYTES, OUTPUT_WAIT_POLL};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
    use std::sync::Arc;
    use std::time::Duration;

    /// Start a waiter thread and signal when `wait_while_congested` returns.
    fn spawn_waiter(
        flow: &Arc<FlowControl>,
        stop: impl Fn() -> bool + Send + 'static,
    ) -> Receiver<()> {
        let (tx, rx) = channel();
        let flow = Arc::clone(flow);
        std::thread::spawn(move || {
            flow.wait_while_congested(stop);
            let _ = tx.send(());
        });
        rx
    }

    /// Waiting longer than one poll interval without a signal proves the
    /// reader is really parked.
    fn assert_still_parked(rx: &Receiver<()>) {
        assert_eq!(
            rx.recv_timeout(OUTPUT_WAIT_POLL + Duration::from_millis(100)),
            Err(RecvTimeoutError::Timeout),
            "reader resumed before the frontend caught up"
        );
    }

    fn assert_released(rx: &Receiver<()>) {
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)),
            Ok(()),
            "reader stayed parked"
        );
    }

    #[test]
    fn reader_parks_at_high_water_and_resumes_at_low_water() {
        let flow = Arc::new(FlowControl::default());
        flow.attach();
        flow.record(OUTPUT_HIGH_WATER_BYTES);
        let waiter = spawn_waiter(&flow, || false);
        assert_still_parked(&waiter);

        // One byte above the low-water mark is still too much.
        flow.ack(OUTPUT_HIGH_WATER_BYTES - OUTPUT_LOW_WATER_BYTES - 1);
        assert_eq!(flow.unacked(), OUTPUT_LOW_WATER_BYTES + 1);
        assert_still_parked(&waiter);

        flow.ack(1);
        assert_eq!(flow.unacked(), OUTPUT_LOW_WATER_BYTES);
        assert_released(&waiter);
    }

    #[test]
    fn reader_only_parks_once_the_high_water_mark_is_reached() {
        let flow = Arc::new(FlowControl::default());
        flow.attach();
        // One byte short of the high-water mark: deliver it and keep going.
        flow.record(OUTPUT_HIGH_WATER_BYTES - 1);
        let waiter = spawn_waiter(&flow, || false);
        assert_released(&waiter);

        flow.record(1);
        let waiter = spawn_waiter(&flow, || false);
        assert_still_parked(&waiter);
        flow.ack(usize::MAX);
        assert_released(&waiter);
    }

    #[test]
    fn nothing_is_counted_or_waited_for_without_a_sink() {
        let flow = Arc::new(FlowControl::default());
        flow.record(OUTPUT_HIGH_WATER_BYTES * 4);
        // No sink attached: output is nobody's problem, so it is neither
        // counted nor waited for.
        assert_eq!(flow.unacked(), 0);
        let waiter = spawn_waiter(&flow, || false);
        assert_released(&waiter);
    }

    #[test]
    fn replacing_or_clearing_the_sink_resets_and_releases() {
        for clear in [true, false] {
            let flow = Arc::new(FlowControl::default());
            flow.attach();
            flow.record(OUTPUT_HIGH_WATER_BYTES);
            let waiter = spawn_waiter(&flow, || false);
            assert_still_parked(&waiter);

            // The budget belongs to the consumer that left.
            if clear {
                flow.detach();
            } else {
                flow.attach();
            }
            assert_eq!(flow.unacked(), 0);
            assert_released(&waiter);
        }
    }

    #[test]
    fn ack_saturates_instead_of_underflowing() {
        let flow = FlowControl::default();
        flow.attach();
        flow.record(OUTPUT_LOW_WATER_BYTES);
        // A late or duplicated ack must not wrap the counter into a huge value
        // that parks the reader forever.
        flow.ack(usize::MAX);
        assert_eq!(flow.unacked(), 0);
        flow.ack(64);
        assert_eq!(flow.unacked(), 0);
    }

    #[test]
    fn cancelled_reader_leaves_the_wait() {
        let flow = Arc::new(FlowControl::default());
        flow.attach();
        flow.record(OUTPUT_HIGH_WATER_BYTES);
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancelled);
        let waiter = spawn_waiter(&flow, move || flag.load(Ordering::SeqCst));
        assert_still_parked(&waiter);

        // Polled, no notify needed: close_pty may only flip the flag.
        cancelled.store(true, Ordering::SeqCst);
        assert_released(&waiter);
    }
}
