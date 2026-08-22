use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const NATIVE_WORKER_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerCommand {
    Run {
        protocol: u32,
        task_id: String,
        node_id: String,
        model_id: String,
        input_artifacts: Vec<PathBuf>,
        output_dir: PathBuf,
        config: serde_json::Value,
    },
    Cancel {
        protocol: u32,
        task_id: String,
    },
    Quit {
        protocol: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerFrame {
    Ready {
        protocol: u32,
        component: String,
        runtime_recipe_digest: String,
    },
    Progress {
        task_id: String,
        fraction: f32,
        #[serde(default)]
        message: String,
    },
    Output {
        task_id: String,
        artifact: String,
        path: PathBuf,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        media_type: Option<String>,
    },
    Done {
        task_id: String,
        status: String,
    },
    Error {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
        code: String,
        message: String,
        #[serde(default)]
        retryable: bool,
    },
}
