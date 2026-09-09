//! Container migration for the model GGUFs that predate the Rust/upstream-GGML
//! runtime.
//!
//! Those files were written by a converter that recorded native PyTorch
//! dimension order, and the two conformer models additionally carry tensor
//! names at or above GGML's 64-character limit, so upstream GGML refuses to
//! open them at all:
//!
//! ```text
//! gguf_init_from_reader: tensor name 88 is too long: 64 >= 64
//! FireRed GGUF tensor shape mismatch: expected [3, 3, 1, 32], found [32, 1, 3, 3]
//! ```
//!
//! This rewrites the container and nothing else. Tensor payload bytes and
//! their offsets are copied verbatim, dimensions are reversed into GGML's
//! fastest-varying-first order, and structural path components are abbreviated
//! where a name would otherwise be too long. The result is byte-identical
//! weights in a container GGML can read.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const ALIGNMENT: u64 = 32;
const GGUF_VERSION: u32 = 3;
const GGML_TYPE_F32: u32 = 0;
const GGUF_VALUE_UINT32: u32 = 4;
const GGUF_VALUE_STRING: u32 = 8;
const MAX_TENSOR_NAME_BYTES: usize = 63;

/// What one model's historical container is allowed to contain, and how its
/// tensor names have to be shortened.
struct Profile {
    id: &'static str,
    architecture: &'static str,
    /// `Some` when the historical converter emitted an exact, known tensor
    /// count for this model.
    expected_tensors: Option<u64>,
    /// Applied in order; each entry replaces every occurrence.
    replacements: &'static [(&'static str, &'static str)],
    /// FireRed's container also carries `uint32` metadata; the two conformer
    /// containers are string-only.
    allow_uint32_metadata: bool,
}

const PROFILES: [Profile; 3] = [
    Profile {
        id: "stars",
        architecture: "stars",
        expected_tensors: Some(1_345),
        replacements: &[
            ("prosody_extractor_sentence", "pes"),
            ("prosody_extractor_utter", "peu"),
            ("prosody_extractor_note", "pen"),
            ("prosody_extractor_word", "pew"),
            ("prosody_extractor_ph", "pep"),
            ("feed_forward_macaron", "ffm"),
            ("feed_forward", "ff"),
            ("encoder_layers", "el"),
            ("freq_experts", "fe"),
            ("cmuencoder", "ce"),
            ("multihead_attn", "mha"),
            ("conv_module", "cm"),
            ("pointwise_conv1", "pw1"),
            ("pointwise_conv2", "pw2"),
            ("depthwise_conv", "dw"),
            ("norm_ff_macaron", "nfm"),
        ],
        allow_uint32_metadata: false,
    },
    Profile {
        id: "rosvot",
        architecture: "rosvot",
        expected_tensors: None,
        replacements: &[
            ("feed_forward_macaron", "ffm"),
            ("feed_forward", "ff"),
            ("encoder_layers", "el"),
            ("multihead_dot_attn", "mda"),
            ("multihead_attn", "mha"),
            ("conv_module", "cm"),
            ("pointwise_conv1", "pw1"),
            ("pointwise_conv2", "pw2"),
            ("depthwise_conv", "dw"),
            ("norm_ff_macaron", "nfm"),
        ],
        allow_uint32_metadata: false,
    },
    Profile {
        id: "firered",
        architecture: "firered_asr2_aed",
        expected_tensors: None,
        replacements: &[],
        allow_uint32_metadata: true,
    },
];

enum MetadataValue {
    Text(String),
    Unsigned(u32),
}

struct Tensor {
    name: String,
    dimensions: Vec<u64>,
    offset: u64,
}

struct Container {
    metadata: Vec<(String, MetadataValue)>,
    tensors: Vec<Tensor>,
    data_offset: u64,
}

pub(crate) fn run(args: &[String]) -> Result<(), String> {
    let (model, source, output) = match args {
        [model, source, output] => (model.as_str(), PathBuf::from(source), PathBuf::from(output)),
        _ => {
            return Err(format!(
                "usage: cargo xtask gguf <{}> SOURCE OUTPUT",
                PROFILES
                    .iter()
                    .map(|profile| profile.id)
                    .collect::<Vec<_>>()
                    .join("|")
            ));
        }
    };
    let profile = PROFILES
        .iter()
        .find(|profile| profile.id == model)
        .ok_or_else(|| format!("unknown model container profile: {model}"))?;
    migrate(profile, &source, &output)?;
    println!("{}", output.display());
    Ok(())
}

fn migrate(profile: &Profile, source: &Path, output: &Path) -> Result<(), String> {
    if !source.is_file() {
        return Err(format!("{} source GGUF is unavailable", profile.id));
    }
    if output.exists() {
        return Err(format!(
            "refusing to overwrite an existing file: {}",
            output.display()
        ));
    }
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| "output path has no parent directory".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("could not create {}: {error}", parent.display()))?;

    let file = File::open(source)
        .map_err(|error| format!("could not open {}: {error}", source.display()))?;
    let mut reader = BufReader::new(file);
    let container = read_container(profile, &mut reader)?;
    let names = canonical_names(profile, &container)?;

    // Publish atomically beside the destination so an interrupted migration
    // never leaves a half-written model artifact behind.
    let temporary = parent.join(format!(
        ".{}.migrating",
        output
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("model.gguf")
    ));
    let _ = std::fs::remove_file(&temporary);
    let result = write_container(&container, &names, &mut reader, &temporary);
    match result {
        Ok(()) => std::fs::rename(&temporary, output)
            .map_err(|error| format!("could not publish {}: {error}", output.display())),
        Err(error) => {
            let _ = std::fs::remove_file(&temporary);
            Err(error)
        }
    }
}

fn canonical_names(profile: &Profile, container: &Container) -> Result<Vec<String>, String> {
    let names: Vec<String> = container
        .tensors
        .iter()
        .map(|tensor| {
            let mut name = tensor.name.clone();
            for (long, short) in profile.replacements {
                name = name.replace(long, short);
            }
            name
        })
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    if sorted.len() != names.len() {
        return Err(format!("{} canonical tensor names collide", profile.id));
    }
    if let Some(long) = names.iter().find(|name| name.len() > MAX_TENSOR_NAME_BYTES) {
        return Err(format!(
            "{} tensor name is still too long for GGML: {long}",
            profile.id
        ));
    }
    Ok(names)
}

fn read_container(profile: &Profile, reader: &mut BufReader<File>) -> Result<Container, String> {
    let mut magic = [0_u8; 4];
    read_exact(reader, &mut magic, profile)?;
    if &magic != b"GGUF" {
        return Err(format!("{} source is not a GGUF container", profile.id));
    }
    let version = read_u32(reader, profile)?;
    if version != GGUF_VERSION {
        return Err(format!(
            "{} source GGUF version is not {GGUF_VERSION}: {version}",
            profile.id
        ));
    }
    let tensor_count = read_u64(reader, profile)?;
    if let Some(expected) = profile.expected_tensors {
        if tensor_count != expected {
            return Err(format!(
                "{} source tensor count is not {expected}: {tensor_count}",
                profile.id
            ));
        }
    }
    let metadata_count = read_u64(reader, profile)?;
    let mut metadata = Vec::new();
    let mut architecture = None;
    for _ in 0..metadata_count {
        let key = read_string(reader, profile)?;
        let value_type = read_u32(reader, profile)?;
        let value = match value_type {
            GGUF_VALUE_STRING => {
                let text = read_string(reader, profile)?;
                if key == "general.architecture" {
                    architecture = Some(text.clone());
                }
                MetadataValue::Text(text)
            }
            GGUF_VALUE_UINT32 if profile.allow_uint32_metadata => {
                MetadataValue::Unsigned(read_u32(reader, profile)?)
            }
            other => {
                return Err(format!(
                    "unsupported {} GGUF metadata type for {key}: {other}",
                    profile.id
                ));
            }
        };
        metadata.push((key, value));
    }
    if architecture.as_deref() != Some(profile.architecture) {
        return Err(format!(
            "{} source GGUF architecture is incompatible: {}",
            profile.id,
            architecture.as_deref().unwrap_or("<missing>")
        ));
    }

    let mut tensors = Vec::with_capacity(tensor_count as usize);
    for _ in 0..tensor_count {
        let name = read_string(reader, profile)?;
        let rank = read_u32(reader, profile)?;
        let mut dimensions = Vec::with_capacity(rank as usize);
        for _ in 0..rank {
            dimensions.push(read_u64(reader, profile)?);
        }
        let tensor_type = read_u32(reader, profile)?;
        if tensor_type != GGML_TYPE_F32 {
            return Err(format!("{} source tensor is not F32: {name}", profile.id));
        }
        let offset = read_u64(reader, profile)?;
        tensors.push(Tensor {
            name,
            dimensions,
            offset,
        });
    }
    let position = reader
        .stream_position()
        .map_err(|error| format!("could not read {} header position: {error}", profile.id))?;
    Ok(Container {
        metadata,
        tensors,
        data_offset: aligned(position),
    })
}

fn write_container(
    container: &Container,
    names: &[String],
    reader: &mut BufReader<File>,
    destination: &Path,
) -> Result<(), String> {
    let file = File::create(destination)
        .map_err(|error| format!("could not create {}: {error}", destination.display()))?;
    let mut writer = BufWriter::new(file);
    write_all(&mut writer, b"GGUF")?;
    write_u32(&mut writer, GGUF_VERSION)?;
    write_u64(&mut writer, container.tensors.len() as u64)?;
    write_u64(&mut writer, container.metadata.len() as u64)?;
    for (key, value) in &container.metadata {
        write_string(&mut writer, key)?;
        match value {
            MetadataValue::Text(text) => {
                write_u32(&mut writer, GGUF_VALUE_STRING)?;
                write_string(&mut writer, text)?;
            }
            MetadataValue::Unsigned(number) => {
                write_u32(&mut writer, GGUF_VALUE_UINT32)?;
                write_u32(&mut writer, *number)?;
            }
        }
    }
    for (tensor, name) in container.tensors.iter().zip(names) {
        write_string(&mut writer, name)?;
        write_u32(&mut writer, tensor.dimensions.len() as u32)?;
        // The historical container recorded native PyTorch shapes; GGML
        // operations index the fastest-varying dimension first.
        for dimension in tensor.dimensions.iter().rev() {
            write_u64(&mut writer, *dimension)?;
        }
        write_u32(&mut writer, GGML_TYPE_F32)?;
        write_u64(&mut writer, tensor.offset)?;
    }
    let position = writer
        .stream_position()
        .map_err(|error| format!("could not measure the rewritten header: {error}"))?;
    let padding = vec![0_u8; (aligned(position) - position) as usize];
    write_all(&mut writer, &padding)?;

    reader
        .seek(SeekFrom::Start(container.data_offset))
        .map_err(|error| format!("could not seek to the tensor payload: {error}"))?;
    let mut buffer = vec![0_u8; 8 * 1024 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("could not read the tensor payload: {error}"))?;
        if read == 0 {
            break;
        }
        write_all(&mut writer, &buffer[..read])?;
    }
    let file = writer
        .into_inner()
        .map_err(|error| format!("could not flush the rewritten container: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("could not persist the rewritten container: {error}"))
}

fn aligned(value: u64) -> u64 {
    value.div_ceil(ALIGNMENT) * ALIGNMENT
}

fn read_exact(
    reader: &mut BufReader<File>,
    buffer: &mut [u8],
    profile: &Profile,
) -> Result<(), String> {
    reader
        .read_exact(buffer)
        .map_err(|error| format!("{} GGUF ended unexpectedly: {error}", profile.id))
}

fn read_u32(reader: &mut BufReader<File>, profile: &Profile) -> Result<u32, String> {
    let mut buffer = [0_u8; 4];
    read_exact(reader, &mut buffer, profile)?;
    Ok(u32::from_le_bytes(buffer))
}

fn read_u64(reader: &mut BufReader<File>, profile: &Profile) -> Result<u64, String> {
    let mut buffer = [0_u8; 8];
    read_exact(reader, &mut buffer, profile)?;
    Ok(u64::from_le_bytes(buffer))
}

fn read_string(reader: &mut BufReader<File>, profile: &Profile) -> Result<String, String> {
    let length = read_u64(reader, profile)?;
    let length = usize::try_from(length)
        .map_err(|_| format!("{} GGUF string length is invalid", profile.id))?;
    let mut buffer = vec![0_u8; length];
    read_exact(reader, &mut buffer, profile)?;
    String::from_utf8(buffer).map_err(|_| format!("{} GGUF string is not UTF-8", profile.id))
}

fn write_all(writer: &mut BufWriter<File>, bytes: &[u8]) -> Result<(), String> {
    writer
        .write_all(bytes)
        .map_err(|error| format!("could not write the rewritten container: {error}"))
}

fn write_u32(writer: &mut BufWriter<File>, value: u32) -> Result<(), String> {
    write_all(writer, &value.to_le_bytes())
}

fn write_u64(writer: &mut BufWriter<File>, value: u64) -> Result<(), String> {
    write_all(writer, &value.to_le_bytes())
}

fn write_string(writer: &mut BufWriter<File>, value: &str) -> Result<(), String> {
    write_u64(writer, value.len() as u64)?;
    write_all(writer, value.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(id: &str) -> &'static Profile {
        PROFILES.iter().find(|profile| profile.id == id).unwrap()
    }

    #[test]
    fn alignment_rounds_up_to_the_gguf_boundary() {
        assert_eq!(aligned(0), 0);
        assert_eq!(aligned(1), 32);
        assert_eq!(aligned(32), 32);
        assert_eq!(aligned(33), 64);
    }

    #[test]
    fn conformer_names_shorten_below_the_ggml_limit() {
        let stars = profile("stars");
        let container = Container {
            metadata: Vec::new(),
            tensors: vec![Tensor {
                name: "model.prosody_extractor_sentence.encoder_layers.0.feed_forward_macaron.pointwise_conv1.weight".to_string(),
                dimensions: vec![1],
                offset: 0,
            }],
            data_offset: 0,
        };
        let names = canonical_names(stars, &container).unwrap();
        assert_eq!(names, ["model.pes.el.0.ffm.pw1.weight"]);
        assert!(names[0].len() <= MAX_TENSOR_NAME_BYTES);
    }

    #[test]
    fn firered_keeps_its_names_and_rejects_an_overlong_one() {
        let firered = profile("firered");
        let container = Container {
            metadata: Vec::new(),
            tensors: vec![Tensor {
                name: "encoder.input_preprocessor.conv.0.weight".to_string(),
                dimensions: vec![32, 1, 3, 3],
                offset: 0,
            }],
            data_offset: 0,
        };
        assert_eq!(
            canonical_names(firered, &container).unwrap(),
            ["encoder.input_preprocessor.conv.0.weight"]
        );

        let overlong = Container {
            metadata: Vec::new(),
            tensors: vec![Tensor {
                name: "a".repeat(MAX_TENSOR_NAME_BYTES + 1),
                dimensions: vec![1],
                offset: 0,
            }],
            data_offset: 0,
        };
        assert!(canonical_names(firered, &overlong).is_err());
    }

    #[test]
    fn colliding_canonical_names_are_rejected() {
        let rosvot = profile("rosvot");
        let container = Container {
            metadata: Vec::new(),
            tensors: vec![
                Tensor {
                    name: "model.feed_forward.weight".to_string(),
                    dimensions: vec![1],
                    offset: 0,
                },
                Tensor {
                    name: "model.ff.weight".to_string(),
                    dimensions: vec![1],
                    offset: 0,
                },
            ],
            data_offset: 0,
        };
        assert!(canonical_names(rosvot, &container).is_err());
    }
}
