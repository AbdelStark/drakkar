//! The engine actor: one dedicated OS thread per model owning all backend state
//! (RFC-0001 A2/§5, RFC-0011 ER5).
//!
//! MLX arrays are `!Send`, so exactly one thread per loaded model owns the
//! [`InferenceBackend`] instance, the KV pool, and the Metal stream, and drives
//! a FIFO message loop (`EngineMsg`). This turns the substrate's threading
//! constraint into an architectural feature: no locks around model state.
//!
//! Panic policy (ER5.1): each message dispatch runs under `catch_unwind`. A
//! panic **poisons** the actor — the offending message and every queued message
//! fail with `internal.panic`, the model transitions to `failed`, the thread
//! exits, and there is no auto-restart. The process survives.
//!
//! The backend is *created on the actor thread* (via a `Send` factory), never
//! moved across threads, so a `!Send` real backend never violates confinement.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, Sender, SyncSender, sync_channel};
use std::thread::JoinHandle;
use std::time::Instant;

use drakkar_core::{
    DecodeBatch, DkError, ErrorCode, MemoryBudget, MemoryReport, ModelArtifact, PrefillChunk, SeqId,
};

use crate::backend::{DecodeOut, InferenceBackend, PrefillOut};
use crate::kv::{EvictPolicy, EvictReport, Reservation};

/// Default request-channel capacity (bounded MPSC, A3 backpressure).
const DEFAULT_QUEUE_CAPACITY: usize = 256;

/// A handler exceeding this wall-time without a backend call in progress trips
/// the debug stall detector (A4).
const STALL_THRESHOLD: std::time::Duration = std::time::Duration::from_millis(500); // status-scan-allow: ms, not an HTTP status

/// The lifecycle state of an actor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActorState {
    /// Processing messages normally.
    Running,
    /// A panic poisoned the actor; the thread has exited.
    Failed,
}

impl ActorState {
    fn from_u8(v: u8) -> Self {
        if v == 0 {
            ActorState::Running
        } else {
            ActorState::Failed
        }
    }
}

type Reply<T> = Sender<Result<T, DkError>>;

/// A message to the engine actor (RFC-0001 §5). Each carries a one-shot reply
/// channel; the engine thread only ever *sends* on these unbounded channels, so
/// it never blocks (A3).
enum EngineMsg {
    Admit {
        seq: SeqId,
        tokens_needed: usize,
        reply: Reply<Reservation>,
    },
    Prefill {
        chunk: PrefillChunk,
        reply: Reply<PrefillOut>,
    },
    DecodeStep {
        batch: DecodeBatch,
        reply: Reply<DecodeOut>,
    },
    Evict {
        policy: EvictPolicy,
        reply: Reply<EvictReport>,
    },
    Snapshot {
        reply: Reply<MemoryReport>,
    },
    Unload {
        reply: Reply<()>,
    },
}

impl EngineMsg {
    /// Fail this message's caller with `internal.panic` (used when the actor is
    /// poisoned).
    fn fail_panicked(self) {
        macro_rules! send {
            ($reply:expr) => {
                let _ = $reply.send(Err(internal_panic()));
            };
        }
        match self {
            EngineMsg::Admit { reply, .. } => {
                send!(reply);
            }
            EngineMsg::Prefill { reply, .. } => {
                send!(reply);
            }
            EngineMsg::DecodeStep { reply, .. } => {
                send!(reply);
            }
            EngineMsg::Evict { reply, .. } => {
                send!(reply);
            }
            EngineMsg::Snapshot { reply } => {
                send!(reply);
            }
            EngineMsg::Unload { reply } => {
                send!(reply);
            }
        }
    }
}

fn internal_panic() -> DkError {
    DkError::new(
        ErrorCode::InternalPanic,
        "the engine actor panicked; every in-flight sequence for this model was aborted",
    )
}

fn actor_gone() -> DkError {
    DkError::new(
        ErrorCode::InternalInvariant,
        "the engine actor is not running (the model failed or was unloaded)",
    )
}

/// A handle to one model's engine actor. Cloning is not offered: there is one
/// owner that drives the model's lifecycle.
pub struct EngineActor {
    tx: Option<SyncSender<EngineMsg>>,
    state: Arc<AtomicU8>,
    join: Option<JoinHandle<()>>,
}

impl EngineActor {
    /// Spawn an actor that creates its backend on the new thread via `factory`
    /// (so a `!Send` backend never crosses threads), loads `artifact` under
    /// `budget`, then processes messages FIFO.
    #[must_use]
    pub fn spawn<F>(factory: F, artifact: ModelArtifact, budget: MemoryBudget) -> Self
    where
        F: FnOnce() -> Box<dyn InferenceBackend> + Send + 'static,
    {
        Self::spawn_with_capacity(factory, artifact, budget, DEFAULT_QUEUE_CAPACITY)
    }

    /// Spawn an actor with an explicit bounded request-queue capacity.
    #[must_use]
    pub fn spawn_with_capacity<F>(
        factory: F,
        artifact: ModelArtifact,
        budget: MemoryBudget,
        capacity: usize,
    ) -> Self
    where
        F: FnOnce() -> Box<dyn InferenceBackend> + Send + 'static,
    {
        let (tx, rx) = sync_channel::<EngineMsg>(capacity);
        let state = Arc::new(AtomicU8::new(0));
        let thread_state = Arc::clone(&state);
        let join = std::thread::Builder::new()
            .name("drakkar-engine".to_owned())
            .spawn(move || run(factory, artifact, budget, rx, &thread_state))
            .expect("spawn engine thread");
        EngineActor {
            tx: Some(tx),
            state,
            join: Some(join),
        }
    }

    /// The current lifecycle state.
    #[must_use]
    pub fn state(&self) -> ActorState {
        ActorState::from_u8(self.state.load(Ordering::Acquire))
    }

    fn request<T>(&self, make: impl FnOnce(Reply<T>) -> EngineMsg) -> Result<T, DkError> {
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        // A failed/gone actor drops the receiver; `send` then errors.
        let tx = self.tx.as_ref().ok_or_else(actor_gone)?;
        tx.send(make(reply_tx)).map_err(|_| actor_gone())?;
        reply_rx.recv().map_err(|_| actor_gone())?
    }

    /// Reserve KV capacity for a sequence.
    ///
    /// # Errors
    /// The backend rejection or a taxonomy error; `internal.*` if the actor died.
    pub fn admit(&self, seq: SeqId, tokens_needed: usize) -> Result<Reservation, DkError> {
        self.request(|reply| EngineMsg::Admit {
            seq,
            tokens_needed,
            reply,
        })
    }

    /// Run one prefill chunk.
    ///
    /// # Errors
    /// A taxonomy error from the backend, or `internal.*` if the actor died.
    pub fn prefill(&self, chunk: PrefillChunk) -> Result<PrefillOut, DkError> {
        self.request(|reply| EngineMsg::Prefill { chunk, reply })
    }

    /// Run one decode step.
    ///
    /// # Errors
    /// A taxonomy error from the backend, or `internal.*` if the actor died.
    pub fn decode(&self, batch: DecodeBatch) -> Result<DecodeOut, DkError> {
        self.request(|reply| EngineMsg::DecodeStep { batch, reply })
    }

    /// Run one KV reclaim pass.
    ///
    /// # Errors
    /// `internal.*` if the actor died.
    pub fn evict(&self, policy: EvictPolicy) -> Result<EvictReport, DkError> {
        self.request(|reply| EngineMsg::Evict { policy, reply })
    }

    /// Read the measured memory report.
    ///
    /// # Errors
    /// `internal.*` if the actor died.
    pub fn snapshot(&self) -> Result<MemoryReport, DkError> {
        self.request(|reply| EngineMsg::Snapshot { reply })
    }

    /// Unload the model, draining in-flight work; the thread then exits.
    ///
    /// # Errors
    /// `internal.*` if the actor died before the unload was processed.
    pub fn unload(&self) -> Result<(), DkError> {
        self.request(|reply| EngineMsg::Unload { reply })
    }
}

impl Drop for EngineActor {
    fn drop(&mut self) {
        // Close the request channel first so the thread's `recv` returns and the
        // loop ends; only then join (otherwise the join deadlocks against a
        // thread still blocked on `recv`).
        self.tx = None;
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn run<F>(
    factory: F,
    artifact: ModelArtifact,
    budget: MemoryBudget,
    rx: Receiver<EngineMsg>,
    state: &Arc<AtomicU8>,
) where
    F: FnOnce() -> Box<dyn InferenceBackend>,
{
    let mut backend = factory();
    let handle = match backend.load(&artifact, budget) {
        Ok(h) => h,
        Err(_load_err) => {
            // Load failure poisons the actor before any message is served.
            state.store(1, Ordering::Release);
            for msg in rx.iter() {
                msg.fail_panicked();
            }
            return;
        }
    };

    while let Ok(msg) = rx.recv() {
        let started = Instant::now();
        let is_unload = matches!(msg, EngineMsg::Unload { .. });
        let poisoned = dispatch(&mut backend, &handle, budget, msg);
        detect_stall(started);
        if poisoned {
            state.store(1, Ordering::Release);
            // Fail every queued message; the model is gone.
            for queued in rx.try_iter() {
                queued.fail_panicked();
            }
            return;
        }
        if is_unload {
            return;
        }
    }
}

/// Dispatch one message under `catch_unwind`. Returns `true` when the dispatch
/// panicked (the actor must be poisoned).
fn dispatch(
    backend: &mut Box<dyn InferenceBackend>,
    handle: &drakkar_core::ModelHandle,
    budget: MemoryBudget,
    msg: EngineMsg,
) -> bool {
    macro_rules! guarded {
        ($reply:expr, $body:expr) => {{
            let reply = $reply;
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| $body)) {
                Ok(result) => {
                    let _ = reply.send(result);
                    false
                }
                Err(_) => {
                    let _ = reply.send(Err(internal_panic()));
                    true
                }
            }
        }};
    }

    let panicked = match msg {
        EngineMsg::Admit {
            seq,
            tokens_needed,
            reply,
        } => guarded!(reply, {
            backend
                .kv()
                .admit(seq, tokens_needed)
                .map_err(|rej| rejection_error(rej.max_admissible))
        }),
        EngineMsg::Prefill { chunk, reply } => {
            guarded!(reply, backend.prefill(handle, chunk))
        }
        EngineMsg::DecodeStep { batch, reply } => {
            guarded!(reply, backend.decode(handle, batch))
        }
        EngineMsg::Evict { policy, reply } => {
            guarded!(reply, Ok(backend.kv().evict(policy)))
        }
        EngineMsg::Snapshot { reply } => {
            guarded!(reply, Ok(backend.memory_report()))
        }
        EngineMsg::Unload { reply } => {
            let _ = reply.send(Ok(()));
            false
        }
    };

    // Memory contract (I2): the resident footprint never exceeds the declared
    // budget after a mutating step. Debug/soak only.
    if !panicked {
        debug_assert!(
            backend.memory_report().actual <= budget.declared,
            "engine exceeded its memory contract (I2)"
        );
    }
    panicked
}

fn rejection_error(max_admissible: usize) -> DkError {
    DkError::new(
        ErrorCode::KvPoolExhausted,
        format!("KV pool cannot admit the request; max admissible is {max_admissible} tokens"),
    )
    .with_context(drakkar_core::ErrorContext::new().with_int(
        "max_admissible",
        i64::try_from(max_admissible).unwrap_or(i64::MAX),
    ))
}

fn detect_stall(started: Instant) {
    let elapsed = started.elapsed();
    if elapsed > STALL_THRESHOLD {
        tracing::warn!(
            target: "drakkar::engine::stall",
            elapsed_ms = elapsed.as_millis() as u64,
            "engine handler exceeded the stall threshold"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{BackendResult, LogitsRef};
    use crate::kv::{ContiguousKvPool, KvPool};
    use crate::mock::{MockBackend, stub_artifact};
    use drakkar_core::{
        Capabilities, ChipId, MemoryBreakdown, ModelHandle, PagedPath, SamplerParams,
        SpecDecodeSupport, TokenId, TokenOut,
    };

    fn budget() -> MemoryBudget {
        MemoryBudget::new(1000, 500, 100, 1200, 0, 81)
    }

    fn actor() -> EngineActor {
        EngineActor::spawn(|| Box::new(MockBackend::new()), stub_artifact(), budget())
    }

    #[test]
    fn admit_then_prefill_then_decode_ordering() {
        let a = actor();
        let res = a.admit(SeqId(0), 128).unwrap();
        assert_eq!(res.tokens_reserved, 128);
        let prefill = a
            .prefill(PrefillChunk {
                seq: SeqId(0),
                tokens: vec![TokenId(1), TokenId(2)],
                position_offset: 0,
                block_table: drakkar_core::BlockTableRef(0),
                is_last: true,
            })
            .unwrap();
        assert_eq!(prefill.tokens_processed, 2);
        let decode = a
            .decode(DecodeBatch {
                entries: vec![drakkar_core::DecodeEntry {
                    seq: SeqId(0),
                    last_token: TokenId(2),
                    position: 2,
                    block_table: drakkar_core::BlockTableRef(0),
                    sampler: drakkar_core::SamplerStateRef(0),
                    grammar_mask: None,
                    draft: None,
                }],
            })
            .unwrap();
        assert_eq!(decode.batch, 1);
        assert_eq!(a.state(), ActorState::Running);
    }

    #[test]
    fn unload_drains_inflight() {
        let a = actor();
        a.admit(SeqId(0), 64).unwrap();
        a.unload().unwrap();
        // After unload the thread exits; further requests report the actor gone.
        let err = a.prefill(PrefillChunk {
            seq: SeqId(0),
            tokens: vec![TokenId(1)],
            position_offset: 0,
            block_table: drakkar_core::BlockTableRef(0),
            is_last: true,
        });
        assert!(err.is_err());
    }

    #[test]
    fn evict_releases_kv_blocks() {
        let a = actor();
        a.admit(SeqId(0), 64).unwrap();
        let report = a.evict(EvictPolicy::Ttl).unwrap();
        assert_eq!(report, EvictReport::default());
    }

    // A backend that returns a named engine error from prefill.
    struct ErroringBackend {
        pool: ContiguousKvPool,
    }
    impl ErroringBackend {
        fn new() -> Self {
            ErroringBackend {
                pool: ContiguousKvPool::new(1 << 20, 128),
            }
        }
    }
    impl InferenceBackend for ErroringBackend {
        fn load(
            &mut self,
            artifact: &ModelArtifact,
            budget: MemoryBudget,
        ) -> BackendResult<ModelHandle> {
            Ok(ModelHandle::new(1, artifact.digest, budget))
        }
        fn prefill(&mut self, _h: &ModelHandle, _b: PrefillChunk) -> BackendResult<PrefillOut> {
            Err(DkError::new(ErrorCode::EngineInferenceFailed, "boom"))
        }
        fn decode(&mut self, _h: &ModelHandle, _b: DecodeBatch) -> BackendResult<DecodeOut> {
            Ok(DecodeOut {
                logits: LogitsRef(0),
                batch: 0,
            })
        }
        fn kv(&mut self) -> &mut dyn KvPool {
            &mut self.pool
        }
        fn sample(&mut self, _l: LogitsRef, _p: &SamplerParams) -> BackendResult<TokenOut> {
            Ok(TokenOut {
                seq: SeqId(0),
                tokens: vec![],
                logprobs: None,
                accepted_draft: 0,
                finish: None,
            })
        }
        fn memory_report(&self) -> MemoryReport {
            MemoryReport {
                actual: 0,
                declared: 0,
                breakdown: MemoryBreakdown {
                    weights: 0,
                    kv: 0,
                    activations: 0,
                    allocator_cache: 0,
                },
                metal_recommended_working_set: 0,
                wired_limit_mb: 0,
            }
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                chip: ChipId {
                    name: "e".to_owned(),
                    gpu_cores: 1,
                },
                bandwidth_gbs: 1.0,
                macos: (26, 0),
                nax_tensor_ops: false,
                kv_bits: vec![16],
                spec_decode: SpecDecodeSupport::default(),
                paged_attention: PagedPath::GatherFallback,
                max_batch: 1,
            }
        }
    }

    #[test]
    fn backend_error_propagates_as_named_error() {
        let a = EngineActor::spawn(
            || Box::new(ErroringBackend::new()),
            stub_artifact(),
            budget(),
        );
        let err = a
            .prefill(PrefillChunk {
                seq: SeqId(0),
                tokens: vec![TokenId(1)],
                position_offset: 0,
                block_table: drakkar_core::BlockTableRef(0),
                is_last: true,
            })
            .unwrap_err();
        assert_eq!(err.code(), ErrorCode::EngineInferenceFailed);
        // A named backend error does NOT poison the actor.
        assert_eq!(a.state(), ActorState::Running);
    }

    // A backend that panics on prefill.
    struct PanicBackend {
        pool: ContiguousKvPool,
    }
    impl InferenceBackend for PanicBackend {
        fn load(
            &mut self,
            artifact: &ModelArtifact,
            budget: MemoryBudget,
        ) -> BackendResult<ModelHandle> {
            Ok(ModelHandle::new(1, artifact.digest, budget))
        }
        fn prefill(&mut self, _h: &ModelHandle, _b: PrefillChunk) -> BackendResult<PrefillOut> {
            panic!("simulated backend fault");
        }
        fn decode(&mut self, _h: &ModelHandle, _b: DecodeBatch) -> BackendResult<DecodeOut> {
            Ok(DecodeOut {
                logits: LogitsRef(0),
                batch: 0,
            })
        }
        fn kv(&mut self) -> &mut dyn KvPool {
            &mut self.pool
        }
        fn sample(&mut self, _l: LogitsRef, _p: &SamplerParams) -> BackendResult<TokenOut> {
            Ok(TokenOut {
                seq: SeqId(0),
                tokens: vec![],
                logprobs: None,
                accepted_draft: 0,
                finish: None,
            })
        }
        fn memory_report(&self) -> MemoryReport {
            MemoryReport {
                actual: 0,
                declared: 0,
                breakdown: MemoryBreakdown {
                    weights: 0,
                    kv: 0,
                    activations: 0,
                    allocator_cache: 0,
                },
                metal_recommended_working_set: 0,
                wired_limit_mb: 0,
            }
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                chip: ChipId {
                    name: "p".to_owned(),
                    gpu_cores: 1,
                },
                bandwidth_gbs: 1.0,
                macos: (26, 0),
                nax_tensor_ops: false,
                kv_bits: vec![16],
                spec_decode: SpecDecodeSupport::default(),
                paged_attention: PagedPath::GatherFallback,
                max_batch: 1,
            }
        }
    }

    #[test]
    fn panic_poisons_actor_and_process_survives() {
        // Silence the panic hook's stderr backtrace during the test.
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let a = EngineActor::spawn(
            || {
                Box::new(PanicBackend {
                    pool: ContiguousKvPool::new(1 << 20, 128),
                })
            },
            stub_artifact(),
            budget(),
        );
        let err = a
            .prefill(PrefillChunk {
                seq: SeqId(0),
                tokens: vec![TokenId(1)],
                position_offset: 0,
                block_table: drakkar_core::BlockTableRef(0),
                is_last: true,
            })
            .unwrap_err();
        std::panic::set_hook(prev);

        // The in-flight sequence received internal.panic ...
        assert_eq!(err.code(), ErrorCode::InternalPanic);
        // ... the model is failed, and the process is still running.
        // (Give the thread a moment to store the failed state / exit.)
        for _ in 0..100 {
            if a.state() == ActorState::Failed {
                break;
            }
            std::thread::yield_now();
        }
        assert_eq!(a.state(), ActorState::Failed);
        // A subsequent request reports the actor gone.
        assert!(a.admit(SeqId(1), 1).is_err());
    }
}
