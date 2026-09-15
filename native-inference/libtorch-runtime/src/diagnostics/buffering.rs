use super::{DurableWrite, io};
use std::time::Duration;

const BUFFER_BYTES: usize = 64 * 1024;
const SYNC_INTERVAL: Duration = Duration::from_secs(1);

/// Only scheduling details may wait for a batch. Device/model/forward/error
/// boundaries remain durable before returning to the caller. Do not make every
/// '*_begin' buffered: initialization failures need their original intent.
pub(super) fn is_detail(phase: &str) -> bool {
    phase.starts_with("qwen_")
        || matches!(
            phase,
            "roformer_stage_await"
                | "roformer_stage_complete"
                | "roformer_attention_block_begin"
                | "roformer_band_split_begin"
                | "roformer_mask_band_begin"
                | "roformer_operator_begin"
                | "weight_upload_progress"
        )
}

pub(super) fn focus_matches(focus: Option<&str>, detail: &str) -> bool {
    focus
        .filter(|focus| !focus.is_empty())
        .is_some_and(|focus| {
            detail == focus
                || detail
                    .strip_prefix(focus)
                    .is_some_and(|suffix| suffix.starts_with('.') || suffix.starts_with(' '))
        })
}

/// Disk persistence only: this never controls a native device wait or tensor
/// lifetime. Qwen's strict operator triplets previously forced three disk syncs
/// for every already-synchronized small GPU step in both ASR and alignment.
/// Keep all records, but amortize only these known details between durable
/// layer/attention entries and completed outer attention tiles.
/// An explicitly focused trace remains synchronous at every matching event.
pub(super) fn requires_sync(phase: &str, detail: &str, focus: Option<&str>) -> bool {
    if focus_matches(focus, detail) {
        return true;
    }
    match phase {
        "qwen_encoder_layer_begin" | "qwen_decoder_layer_begin" | "qwen_attention_begin" => true,
        "qwen_stage_complete"
            if matches!(
                detail,
                "encoder.attention_window" | "decoder.attention_tile"
            ) =>
        {
            true
        }
        "attention_operator_begin" | "attention_operator_await" | "attention_operator_complete" => {
            // Do not broaden this to every attention_* event or every *_begin.
            // Unknown scopes/operators and all error phases retain persistence.
            !matches!(
                detail.split_ascii_whitespace().next(),
                Some(
                    "qwen.strict.output_allocate"
                        | "qwen.strict.kv_pack"
                        | "qwen.strict.query_view"
                        | "qwen.strict.scores"
                        | "qwen.strict.softmax"
                        | "qwen.strict.values"
                )
            )
        }
        _ => !is_detail(phase),
    }
}

#[derive(Default)]
pub(super) struct RecordBuffer {
    bytes: Vec<u8>,
    last_sync: Duration,
}

impl RecordBuffer {
    pub(super) fn append(
        &mut self,
        writer: &mut impl DurableWrite,
        record: &serde_json::Value,
        durable: bool,
        elapsed: Duration,
    ) -> io::Result<()> {
        // Encode before changing the buffer or writing a partial JSON record.
        let mut encoded = serde_json::to_vec(record).map_err(io::Error::other)?;
        encoded.push(b'\n');
        if !self.bytes.is_empty() && self.bytes.len().saturating_add(encoded.len()) > BUFFER_BYTES {
            self.flush(writer, false, elapsed)?;
        }
        self.bytes.extend_from_slice(&encoded);
        // Time is observed on the producer's next event, not by a background
        // timer. An unreturned/hung operator can leave a longer unsynced tail.
        let synchronize = durable || elapsed.saturating_sub(self.last_sync) >= SYNC_INTERVAL;
        if synchronize || self.bytes.len() >= BUFFER_BYTES {
            self.flush(writer, synchronize, elapsed)?;
        }
        Ok(())
    }

    fn flush(
        &mut self,
        writer: &mut impl DurableWrite,
        synchronize: bool,
        elapsed: Duration,
    ) -> io::Result<()> {
        writer.write_all(&self.bytes)?;
        writer.flush()?;
        if synchronize {
            writer.sync_record()?;
            self.last_sync = elapsed;
        }
        self.bytes.clear();
        Ok(())
    }
}
