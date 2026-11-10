use serde::Serialize;
use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RunStatus {
    Succeeded,
    Failed,
    Cancelled,
    Rejected,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ExecEvent {
    RunStarted {
        run_id: String,
    },
    NodeStarted {
        node_id: String,
    },
    NodeChunk {
        node_id: String,
        text: String,
    },
    NodeSucceeded {
        node_id: String,
        output: Map<String, Value>,
    },
    NodeFailed {
        node_id: String,
        message: String,
    },
    NodeSkipped {
        node_id: String,
    },
    RunFinished {
        run_id: String,
        status: RunStatus,
    },
}

pub type EventSink = tokio::sync::mpsc::UnboundedSender<ExecEvent>;
