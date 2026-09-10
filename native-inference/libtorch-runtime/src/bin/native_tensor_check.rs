//! Explicit native diagnostic calls with complete, typed output evidence.
//! Input generation/comparison is separate; this process owns all inference.
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;
use uta_libtorch_runtime::{Backend, Library, Precision, Tensor, Values};

#[derive(Deserialize)]
struct Request {
    library: PathBuf,
    backend: String,
    device: u16,
    precision: String,
    resource: String,
    model_path: PathBuf,
    steps: Vec<Step>,
}
#[derive(Deserialize)]
struct Step {
    name: String,
    operation: String,
    inputs: BTreeMap<String, Tensor>,
}
#[derive(Serialize)]
struct SavedTensor {
    path: PathBuf,
    shape: Vec<i64>,
    dtype: &'static str,
    elements: usize,
    all_finite: bool,
}

fn emit(value: &impl Serialize) -> Result<(), String> {
    let mut output = std::io::stdout().lock();
    serde_json::to_writer(&mut output, value).map_err(|error| error.to_string())?;
    writeln!(output)
        .and_then(|_| output.flush())
        .map_err(|error| error.to_string())
}
fn save_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    serde_json::to_writer_pretty(&mut file, value).map_err(|error| error.to_string())?;
    file.write_all(b"\n")
        .and_then(|_| file.sync_all())
        .map_err(|error| error.to_string())
}
fn save_tensor(path: &Path, tensor: &Tensor) -> Result<SavedTensor, String> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    let mut file = BufWriter::new(file);
    let (dtype, elements, all_finite) = match &tensor.data {
        Values::F32(values) => {
            for value in values {
                file.write_all(&value.to_le_bytes())
                    .map_err(|error| error.to_string())?;
            }
            (
                "f32le",
                values.len(),
                values.iter().all(|value| value.is_finite()),
            )
        }
        Values::I64(values) => {
            for value in values {
                file.write_all(&value.to_le_bytes())
                    .map_err(|error| error.to_string())?;
            }
            ("i64le", values.len(), true)
        }
    };
    file.flush()
        .and_then(|_| file.get_ref().sync_all())
        .map_err(|error| error.to_string())?;
    Ok(SavedTensor {
        path: path.to_path_buf(),
        shape: tensor.shape.clone(),
        dtype,
        elements,
        all_finite,
    })
}
fn run() -> Result<(), String> {
    let arguments = std::env::args().collect::<Vec<_>>();
    if arguments.len() != 3 {
        return Err("usage: native_tensor_check REQUEST_JSON NEW_OUTPUT_DIRECTORY".into());
    }
    let bytes = fs::read(&arguments[1]).map_err(|error| error.to_string())?;
    let request: Request = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let backend = Backend::parse(&request.backend)?;
    let precision = Precision::parse(&request.precision)?;
    let root = Path::new(&arguments[2]);
    fs::create_dir(root)
        .map_err(|error| format!("new diagnostic directory {}: {error}", root.display()))?;
    let mut copy = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join("request.json"))
        .map_err(|error| error.to_string())?;
    copy.write_all(&bytes)
        .and_then(|_| copy.sync_all())
        .map_err(|error| error.to_string())?;
    emit(
        &json!({"event":"begin", "backend":request.backend, "device":request.device, "precision":request.precision, "resource":request.resource}),
    )?;
    let started = Instant::now();
    let library = Library::load(&request.library)?;
    let runtime_seconds = started.elapsed().as_secs_f64();
    let started = Instant::now();
    let model = library.open(
        &request.resource,
        &request.model_path,
        backend,
        request.device,
        precision,
    )?;
    let model_seconds = started.elapsed().as_secs_f64();
    emit(
        &json!({"event":"loaded", "runtime_seconds":runtime_seconds, "model_seconds":model_seconds, "build":library.build_info()}),
    )?;
    for (index, step) in request.steps.iter().enumerate() {
        // Caller/native tensor names are metadata, never filesystem paths.
        let directory = root.join(format!("step-{index}"));
        fs::create_dir(&directory).map_err(|error| error.to_string())?;
        let inputs = step
            .inputs
            .iter()
            .map(|(name, tensor)| tensor.input(name))
            .collect::<Vec<_>>();
        emit(&json!({"event":"step_begin", "name":step.name, "operation":step.operation}))?;
        let started = Instant::now();
        let output = model.forward(&step.operation, &inputs)?;
        let host_seconds = started.elapsed().as_secs_f64();
        let mut tensors = BTreeMap::new();
        for (offset, (name, tensor)) in output.tensors.iter().enumerate() {
            tensors.insert(
                name.clone(),
                save_tensor(&directory.join(format!("tensor-{offset}.bin")), tensor)?,
            );
        }
        let all_finite = tensors.values().all(|tensor| tensor.all_finite);
        let report = json!({"event":"step_complete", "name":step.name, "operation":step.operation,
            "host_seconds_including_input_validation_and_output_copy":host_seconds,
            "native_timings":output.timings, "tensors":tensors, "all_finite":all_finite});
        save_json(&directory.join("result.json"), &report)?;
        emit(&report)?;
        if !all_finite {
            return Err(format!(
                "non-finite output in diagnostic step {}",
                step.name
            ));
        }
    }
    drop(model); // Retained native library still owns every destructor symbol.
    save_json(
        &root.join("result.json"),
        &json!({"status":"completed", "steps":request.steps.len(),
        "scope":"explicit_native_tensor_calls_not_pipeline_or_perceptual_qualification",
        "backend":request.backend, "device":request.device, "precision":request.precision}),
    )?;
    emit(&json!({"event":"completed", "steps":request.steps.len()}))
}
fn main() {
    if let Err(error) = run() {
        let _ = emit(&json!({"event":"failed", "error":error}));
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_typed_outputs_preserve_bits_and_do_not_overwrite() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "uta-studio-native-tensor-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("floats.bin");
        let values = vec![-0.0_f32, 0.125, f32::INFINITY];
        let tensor = Tensor {
            shape: vec![3],
            data: Values::F32(values.clone()),
        };
        let saved = save_tensor(&path, &tensor).unwrap();
        assert_eq!(saved.elements, 3);
        assert!(!saved.all_finite);
        assert_eq!(
            fs::read(&path).unwrap(),
            values
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<_>>()
        );
        assert!(save_tensor(&path, &tensor).is_err());
        let path = root.join("integers.bin");
        let saved = save_tensor(
            &path,
            &Tensor {
                shape: vec![2],
                data: Values::I64(vec![i64::MIN, i64::MAX]),
            },
        )
        .unwrap();
        assert_eq!(saved.dtype, "i64le");
        assert_eq!(
            fs::read(&path).unwrap(),
            [i64::MIN, i64::MAX]
                .into_iter()
                .flat_map(i64::to_le_bytes)
                .collect::<Vec<_>>()
        );
        fs::remove_dir_all(root).unwrap();
    }
}
