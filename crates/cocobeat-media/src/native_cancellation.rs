//! One-attempt native import cancellation; constructing a handle never initializes ORT

use ort::session::{RunOptions, builder::LoadCanceler};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Default)]
pub struct NativeBeatCancellation(Arc<Inner>);

#[derive(Default)]
struct Inner {
    requested: AtomicBool,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    started: bool,
    publishing: bool,
    finished: bool,
    requested_ns: Option<u128>,
    late_request_ns: Option<u128>,
    observed: Option<(&'static str, u128)>,
    backend: Option<Backend>,
    backend_requests: Vec<serde_json::Value>,
    backend_returns: Vec<serde_json::Value>,
    finished_ns: Option<u128>,
}

enum Backend {
    Load(LoadCanceler),
    Run(Arc<RunOptions>),
}

impl Backend {
    fn request(&self) -> Result<(), String> {
        match self {
            Self::Load(handle) => handle.cancel(),
            Self::Run(handle) => handle.terminate(),
        }
        .map_err(|error| error.to_string())
    }

    fn phase(&self) -> &'static str {
        match self {
            Self::Load(_) => "ORT model load",
            Self::Run(_) => "ORT chunk Run",
        }
    }
}

fn now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

impl NativeBeatCancellation {
    /// Returns whether the request was accepted before final publication
    /// Repeated requests are idempotent; the signal thread never releases Session ownership
    pub fn request(&self) -> bool {
        let mut state = self.0.state.lock().unwrap();
        if state.publishing || state.finished {
            state.late_request_ns.get_or_insert_with(now);
            return false;
        }
        if self.0.requested.swap(true, Ordering::AcqRel) {
            return true;
        }
        state.requested_ns = Some(now());
        Self::request_backend(&mut state);
        true
    }

    fn request_backend(state: &mut State) {
        if let Some(backend) = &state.backend {
            let phase = backend.phase();
            let result = backend.request();
            // Ok is only the safe API request result; load cancellation is best effort
            state.backend_requests.push(serde_json::json!({
                "phase":phase,"returned_ns":now(),"request_result":result.err(),
                "scope":"request API returned; actual backend result recorded separately"
            }));
        }
    }

    pub(crate) fn begin(&self) -> Result<(), String> {
        let mut state = self.0.state.lock().unwrap();
        if state.started {
            return Err(
                "Native cancellation handle is one-attempt; retry with a new handle".into(),
            );
        }
        state.started = true;
        Ok(())
    }

    pub(crate) fn check(&self, phase: &'static str) -> Result<(), String> {
        if !self.0.requested.load(Ordering::Acquire) {
            return Ok(());
        }
        let mut state = self.0.state.lock().unwrap();
        state.observed.get_or_insert_with(|| (phase, now()));
        Err(format!("Native import cancellation observed at {phase}"))
    }

    fn register(&self, backend: Backend) -> Registration<'_> {
        let mut state = self.0.state.lock().unwrap();
        assert!(state.backend.is_none());
        state.backend = Some(backend);
        // Cover a request which arrived before this load/run was registered
        if self.0.requested.load(Ordering::Acquire) {
            Self::request_backend(&mut state);
        }
        Registration(self)
    }

    pub(crate) fn register_load(&self, backend: LoadCanceler) -> Registration<'_> {
        self.register(Backend::Load(backend))
    }

    pub(crate) fn register_run(&self, backend: Arc<RunOptions>) -> Registration<'_> {
        self.register(Backend::Run(backend))
    }

    pub(crate) fn backend_return(&self, phase: &'static str, failure: Option<String>) {
        self.0
            .state
            .lock()
            .unwrap()
            .backend_returns
            .push(serde_json::json!({
                "phase":phase,"returned_ns":now(),"actual_failure":failure
            }));
    }

    /// Only the final outer bundle rename is publication; an inner package stays owned staging
    pub(crate) fn publish<T>(
        &self,
        publish: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        {
            let mut state = self.0.state.lock().unwrap();
            if self.0.requested.load(Ordering::Acquire) {
                state
                    .observed
                    .get_or_insert_with(|| ("outer publication gate", now()));
                return Err("Native import cancellation observed at outer publication gate".into());
            }
            state.publishing = true;
        }
        // Once publication wins, a late Ctrl+C cannot change rename's real success/error
        publish()
    }

    pub(crate) fn finish(&self) {
        let mut state = self.0.state.lock().unwrap();
        assert!(state.backend.is_none());
        state.finished = true;
        state.finished_ns = Some(now());
    }

    /// Local software request/call history; it is not a physical signal or latency measurement
    pub fn diagnostics(&self) -> serde_json::Value {
        let state = self.0.state.lock().unwrap();
        serde_json::json!({
            "request_accepted":self.0.requested.load(Ordering::Acquire),
            "requested_ns":state.requested_ns,"late_request_ns":state.late_request_ns,
            "clock":"local wall-clock epoch nanoseconds; measure latency separately with Instant",
            "observed_checkpoint":state.observed.map(|(phase,time)| serde_json::json!({"phase":phase,"observed_ns":time})),
            "backend_registered":state.backend.as_ref().map(Backend::phase),
            "backend_requests":state.backend_requests,"backend_returns":state.backend_returns,
            "publication_started":state.publishing,"finished":state.finished,"finished_ns":state.finished_ns
        })
    }
}

pub(crate) struct Registration<'a>(&'a NativeBeatCancellation);

impl Drop for Registration<'_> {
    fn drop(&mut self) {
        self.0.0.state.lock().unwrap().backend = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn early_and_repeated_requests_are_observed_without_ort_initialization() {
        let cancel = NativeBeatCancellation::default();
        assert!(cancel.request());
        assert!(cancel.request());
        cancel.begin().unwrap();
        assert!(cancel.check("source snapshot").is_err());
        let mut published = false;
        assert!(
            cancel
                .publish(|| {
                    published = true;
                    Ok(())
                })
                .is_err()
        );
        assert!(!published);
        cancel.finish();
        assert!(!cancel.request());
        assert_eq!(
            cancel.diagnostics()["observed_checkpoint"]["phase"],
            "source snapshot"
        );
        assert!(cancel.begin().is_err());
        NativeBeatCancellation::default().begin().unwrap();
    }

    #[test]
    fn publication_wins_and_preserves_the_actual_io_failure() {
        let cancel = NativeBeatCancellation::default();
        cancel.begin().unwrap();
        let result: Result<(), String> = cancel.publish(|| {
            assert!(!cancel.request());
            Err("actual rename failure".into())
        });
        assert_eq!(result.unwrap_err(), "actual rename failure");
        cancel.finish();
        assert!(cancel.diagnostics()["requested_ns"].is_null());
        assert!(!cancel.diagnostics()["late_request_ns"].is_null());
    }
    #[test]
    fn simultaneous_request_and_publication_have_one_winner() {
        for _ in 0..32 {
            let cancel = NativeBeatCancellation::default();
            cancel.begin().unwrap();
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let signal = cancel.clone();
            let ready = Arc::clone(&barrier);
            let worker = std::thread::spawn(move || {
                ready.wait();
                signal.request()
            });
            barrier.wait();
            let published = cancel.publish(|| Ok(()));
            let accepted = worker.join().unwrap();
            assert_eq!(published.is_err(), accepted);
            cancel.finish();
        }
    }
}
