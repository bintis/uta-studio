use std::cell::RefCell;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::workflow::WorkflowExecution;

pub type EngineEventSink = Arc<dyn Fn(EngineLifecycleEvent) + Send + Sync + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineLifecycleKind {
    NodeStarted,
    NodeProgress,
    NodeCompleted,
    NodeFailed,
    Artifact,
    Warning,
    Degraded,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EngineLifecycleEvent {
    #[serde(rename = "type")]
    pub kind: EngineLifecycleKind,
    pub schema_version: u32,
    pub request_id: String,
    /// Engine-owned execution identity.
    pub node_id: String,
    /// Optional persisted Processing Studio presentation identity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation_node_id: Option<String>,
    pub capability_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub implementation: String,
    /// The resolved native backend actually dispatched for this node (e.g.
    /// `"ggml_vulkan"`, `"ggml_cpu"`, `"libtorch_xpu"`). `implementation` alone
    /// cannot distinguish these: the shared `uta-ggml-worker` process name is
    /// unchanged across all of them, so a crash log with no `backend` here is
    /// otherwise unable to say which native runtime actually ran.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    /// The GGML device class actually requested for this node (e.g. `"gpu"`
    /// for the Intel Arc B580 discrete adapter, `"integrated_gpu"` for the
    /// AMD Radeon iGPU, `"cpu"`). Only meaningful alongside a `ggml_*`
    /// `backend`; `libtorch_xpu` nodes always run on the Arc B580 regardless
    /// of this field. Exists so a crash log can show exactly which physical
    /// GPU each concurrently-running node was on, not just which backend
    /// family.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_class: Option<String>,
    /// Present only for measured worker progress. Overall DAG progress is never
    /// inferred from node order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_units_completed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_units_total: Option<u64>,
    /// Exact native worker task identity for measured progress correlation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker_task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact: Option<String>,
    /// Present only alongside `artifact`, and only for a worker output the
    /// caller might want to reuse on a future run (a Step 1 audio-chain
    /// stem) -- not every artifact frame names a real file (some just mark
    /// an in-memory evidence bundle as ready), so this stays optional even
    /// when `artifact` is set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub event_at_ms: i64,
}

#[derive(Clone)]
struct EventIdentity {
    request_id: String,
    node_id: String,
    presentation_node_id: Option<String>,
    capability_id: String,
    model_id: Option<String>,
    implementation: String,
    backend: Option<String>,
    device_class: Option<String>,
}

#[derive(Clone)]
struct EventContext {
    sink: EngineEventSink,
    request_id: String,
    workflow: Option<WorkflowExecution>,
    plan_nodes: Vec<(String, String)>,
}

thread_local! {
    static EVENT_CONTEXT: RefCell<Option<EventContext>> = const { RefCell::new(None) };
}

#[derive(Clone)]
pub(crate) struct EventSnapshot(Option<EventContext>);
pub(crate) struct EventScope {
    previous: Option<EventContext>,
    thread: std::marker::PhantomData<std::rc::Rc<()>>,
}
impl EventSnapshot {
    pub(crate) fn capture() -> Self {
        Self(EVENT_CONTEXT.with(|slot| slot.borrow().clone()))
    }
    pub(crate) fn enter(&self) -> EventScope {
        EventScope {
            previous: EVENT_CONTEXT.with(|slot| slot.replace(self.0.clone())),
            thread: std::marker::PhantomData,
        }
    }
}
impl Drop for EventScope {
    fn drop(&mut self) {
        EVENT_CONTEXT.with(|slot| slot.replace(self.previous.take()));
    }
}

pub(crate) fn with_event_sink<T>(
    request_id: &str,
    workflow: Option<WorkflowExecution>,
    plan_nodes: Vec<(String, String)>,
    sink: EngineEventSink,
    execute: impl FnOnce() -> T,
) -> T {
    let _scope = EventSnapshot(Some(EventContext {
        sink,
        request_id: request_id.to_string(),
        workflow,
        plan_nodes,
    }))
    .enter();
    execute()
}

pub(crate) struct LifecycleNodeGuard {
    identity: Option<EventIdentity>,
}

impl LifecycleNodeGuard {
    pub(crate) fn worker_progress(
        &self,
        fraction: f32,
        worker_task_id: impl Into<String>,
        message: impl Into<String>,
    ) {
        if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
            return;
        }
        if let Some(identity) = self.identity.as_ref() {
            emit(identity, EngineLifecycleKind::NodeProgress, |event| {
                event.progress = Some(fraction);
                event.worker_task_id = Some(worker_task_id.into());
                event.message = Some(message.into());
            });
        }
    }

    pub(crate) fn measured_progress(
        &self,
        fraction: f32,
        completed: u64,
        total: u64,
        worker_task_id: impl Into<String>,
        message: impl Into<String>,
    ) {
        if !fraction.is_finite()
            || !(0.0..=1.0).contains(&fraction)
            || total == 0
            || completed > total
        {
            return;
        }
        if let Some(identity) = self.identity.as_ref() {
            emit(identity, EngineLifecycleKind::NodeProgress, |event| {
                event.progress = Some(fraction);
                event.work_units_completed = Some(completed);
                event.work_units_total = Some(total);
                event.worker_task_id = Some(worker_task_id.into());
                event.message = Some(message.into());
            });
        }
    }

    pub(crate) fn artifact(&self, artifact: impl Into<String>) {
        if let Some(identity) = self.identity.as_ref() {
            emit(identity, EngineLifecycleKind::Artifact, |event| {
                event.artifact = Some(artifact.into());
            });
        }
    }

    /// Same as `artifact`, but also reports the real file the caller wrote
    /// it to -- for a Step 1 audio-chain stem, the app can capture this
    /// file into its own cache as soon as it exists, instead of only ever
    /// learning about it from a final result manifest that a later,
    /// unrelated node failure might prevent from ever being produced.
    pub(crate) fn artifact_with_path(&self, artifact: impl Into<String>, path: impl Into<String>) {
        if let Some(identity) = self.identity.as_ref() {
            emit(identity, EngineLifecycleKind::Artifact, |event| {
                event.artifact = Some(artifact.into());
                event.path = Some(path.into());
            });
        }
    }

    pub(crate) fn complete(mut self) {
        if let Some(identity) = self.identity.take() {
            emit(&identity, EngineLifecycleKind::NodeCompleted, |_| {});
        }
    }
}

impl Drop for LifecycleNodeGuard {
    fn drop(&mut self) {
        if let Some(identity) = self.identity.take() {
            emit(&identity, EngineLifecycleKind::NodeFailed, |event| {
                event.message = Some("execution ended before node completion".to_string());
            });
        }
    }
}

pub(crate) fn begin_node(
    node_id: impl Into<String>,
    capability_id: impl Into<String>,
    model_id: Option<&str>,
    implementation: impl Into<String>,
) -> LifecycleNodeGuard {
    begin_node_for_presentation(
        node_id,
        capability_id,
        model_id,
        implementation,
        None,
        None,
        None,
    )
}

pub(crate) fn begin_node_for_presentation(
    node_id: impl Into<String>,
    capability_id: impl Into<String>,
    model_id: Option<&str>,
    implementation: impl Into<String>,
    presentation_node_id: Option<&str>,
    backend: Option<&str>,
    device_class: Option<&str>,
) -> LifecycleNodeGuard {
    let node_id = node_id.into();
    let capability_id = capability_id.into();
    let implementation = implementation.into();
    let backend = backend.map(str::to_string);
    let device_class = device_class.map(str::to_string);
    let identity = EVENT_CONTEXT.with(|slot| {
        let context = slot.borrow();
        let context = context.as_ref()?;
        let raw_node_id = context
            .plan_nodes
            .iter()
            .find(|(planned_id, _)| planned_id == &node_id)
            .or_else(|| {
                context
                    .plan_nodes
                    .iter()
                    .find(|(_, planned_capability)| planned_capability == &capability_id)
            })
            .map(|(planned_id, _)| planned_id.clone())
            .unwrap_or(node_id);
        let presentation_node_id = presentation_node_id.map(str::to_string).or_else(|| {
            context.workflow.as_ref().and_then(|workflow| {
                workflow.presentation_node_for_engine_execution(&capability_id, model_id)
            })
        });
        Some(EventIdentity {
            request_id: context.request_id.clone(),
            node_id: raw_node_id,
            presentation_node_id,
            capability_id,
            model_id: model_id.map(str::to_string),
            implementation,
            backend,
            device_class,
        })
    });
    if let Some(identity) = identity.as_ref() {
        emit(identity, EngineLifecycleKind::NodeStarted, |_| {});
    }
    LifecycleNodeGuard { identity }
}

pub(crate) fn emit_degraded(message: impl Into<String>) {
    emit_run_message(EngineLifecycleKind::Degraded, message.into());
}

pub(crate) fn emit_warning(message: impl Into<String>) {
    emit_run_message(EngineLifecycleKind::Warning, message.into());
}

fn emit_run_message(kind: EngineLifecycleKind, message: String) {
    let identity = EVENT_CONTEXT.with(|slot| {
        let context = slot.borrow();
        let context = context.as_ref()?;
        Some(EventIdentity {
            request_id: context.request_id.clone(),
            node_id: "analysis-run".to_string(),
            presentation_node_id: None,
            capability_id: "analysis.run".to_string(),
            model_id: None,
            implementation: "uta-analysis-engine".to_string(),
            backend: None,
            device_class: None,
        })
    });
    if let Some(identity) = identity {
        emit(&identity, kind, |event| event.message = Some(message));
    }
}

fn emit(
    identity: &EventIdentity,
    kind: EngineLifecycleKind,
    update: impl FnOnce(&mut EngineLifecycleEvent),
) {
    let mut event = EngineLifecycleEvent {
        kind,
        schema_version: 1,
        request_id: identity.request_id.clone(),
        node_id: identity.node_id.clone(),
        presentation_node_id: identity.presentation_node_id.clone(),
        capability_id: identity.capability_id.clone(),
        model_id: identity.model_id.clone(),
        implementation: identity.implementation.clone(),
        backend: identity.backend.clone(),
        device_class: identity.device_class.clone(),
        progress: None,
        work_units_completed: None,
        work_units_total: None,
        worker_task_id: None,
        artifact: None,
        path: None,
        message: None,
        event_at_ms: now_ms(),
    };
    update(&mut event);
    EVENT_CONTEXT.with(|slot| {
        if let Some(context) = slot.borrow().as_ref() {
            (context.sink)(event);
        }
    });
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn lifecycle_progress_is_measured_and_ordered() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let target = Arc::clone(&events);
        with_event_sink(
            "request",
            None,
            Vec::new(),
            Arc::new(move |event| target.lock().unwrap().push(event)),
            || {
                let node = begin_node("pitch", "pitch.track", Some("rmvpe"), "ggml_vulkan");
                node.worker_progress(0.2, "rmvpe-task-7", "preparing windows");
                node.measured_progress(0.25, 2, 8, "rmvpe-task-7", "frame batch");
                node.artifact("pitch_evidence");
                node.complete();
            },
        );
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 5);
        assert_eq!(events[0].kind, EngineLifecycleKind::NodeStarted);
        assert_eq!(events[1].progress, Some(0.2));
        assert_eq!(events[1].worker_task_id.as_deref(), Some("rmvpe-task-7"));
        assert_eq!(events[2].progress, Some(0.25));
        assert_eq!(events[2].work_units_completed, Some(2));
        assert_eq!(events[2].work_units_total, Some(8));
        assert_eq!(events[2].worker_task_id.as_deref(), Some("rmvpe-task-7"));
        assert_eq!(events[3].artifact.as_deref(), Some("pitch_evidence"));
        assert_eq!(events[4].kind, EngineLifecycleKind::NodeCompleted);
    }

    #[test]
    fn explicit_presentation_identity_keeps_duplicate_capability_runs_distinct() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let target = Arc::clone(&events);
        with_event_sink(
            "request",
            None,
            Vec::new(),
            Arc::new(move |event| target.lock().unwrap().push(event)),
            || {
                begin_node_for_presentation(
                    "audio.denoise",
                    "audio.denoise",
                    Some("melband_roformer_denoise_aufr33"),
                    "openvino",
                    Some("workflow.cleanup_copy"),
                    Some("libtorch_xpu"),
                    Some("gpu"),
                )
                .complete();
            },
        );
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|event| {
            event.node_id == "audio.denoise"
                && event.presentation_node_id.as_deref() == Some("workflow.cleanup_copy")
                && event.capability_id == "audio.denoise"
                && event.backend.as_deref() == Some("libtorch_xpu")
                && event.device_class.as_deref() == Some("gpu")
        }));
    }

    #[test]
    fn backend_and_device_class_are_absent_when_not_supplied() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let target = Arc::clone(&events);
        with_event_sink(
            "request",
            None,
            Vec::new(),
            Arc::new(move |event| target.lock().unwrap().push(event)),
            || {
                begin_node("decode", "audio.decode", None, "ffmpeg").complete();
            },
        );
        let events = events.lock().unwrap();
        assert!(
            events
                .iter()
                .all(|event| event.backend.is_none() && event.device_class.is_none())
        );
    }
}
