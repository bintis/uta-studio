use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerCommand {
    Run {
        protocol: u32,
        task_id: String,
        node_id: String,
        model_id: String,
        input_artifacts: Vec<PathBuf>,
        output_dir: PathBuf,
        #[serde(default)]
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

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerFrame<'a> {
    Ready {
        protocol: u32,
        component: &'a str,
        runtime_recipe_digest: &'a str,
    },
    Progress {
        task_id: &'a str,
        fraction: f32,
        message: &'a str,
    },
    Output {
        task_id: &'a str,
        artifact: &'a str,
        path: &'a std::path::Path,
        media_type: &'a str,
    },
    Done {
        task_id: &'a str,
        status: &'a str,
    },
    Error {
        task_id: Option<&'a str>,
        code: &'a str,
        message: &'a str,
        retryable: bool,
    },
}

pub fn command_protocol(command: &WorkerCommand) -> u32 {
    match command {
        WorkerCommand::Run { protocol, .. }
        | WorkerCommand::Cancel { protocol, .. }
        | WorkerCommand::Quit { protocol } => *protocol,
    }
}

pub fn emit(frame: WorkerFrame<'_>) -> Result<(), String> {
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &frame).map_err(|error| error.to_string())?;
    stdout.write_all(b"\n").map_err(|error| error.to_string())?;
    stdout.flush().map_err(|error| error.to_string())
}
