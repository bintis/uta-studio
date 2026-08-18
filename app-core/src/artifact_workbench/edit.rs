use super::*;

pub fn begin_artifact_edit(reference: &ArtifactRef) -> Result<ArtifactEditDraft, String> {
    let inspection = inspect_artifact(reference)?;
    let revision = inspection.artifact;
    let bytes = bounded_read(&revision.path, 8 * 1024 * 1024)?;
    let (draft_kind, output_kind, working_copy) = match revision.kind {
        ArtifactKind::LyricsInput => (
            ArtifactDraftKind::Lyrics,
            ArtifactKind::LyricsInput,
            ArtifactDraftContent::Text(artifact_editor_text(reference)?),
        ),
        ArtifactKind::RecognizedText => (
            ArtifactDraftKind::Lyrics,
            ArtifactKind::LyricsInput,
            ArtifactDraftContent::Text(artifact_editor_text(reference)?),
        ),
        ArtifactKind::TimedTranscript | ArtifactKind::AsrSegments => (
            ArtifactDraftKind::TimedTranscript,
            ArtifactKind::TimedTranscript,
            ArtifactDraftContent::Json(
                serde_json::from_slice(&bytes).map_err(|error| error.to_string())?,
            ),
        ),
        _ => return Err("this artifact kind does not have a safe draft editor".to_string()),
    };
    let original_active_revision_id =
        load_active_artifact(&reference.file_hash, output_kind).map(|active| active.id);
    let validation = validate_draft_content(draft_kind, &working_copy);
    Ok(ArtifactEditDraft {
        source: reference.clone(),
        draft_kind,
        output_kind,
        original_content_hash: revision.content_hash,
        original_active_revision_id,
        working_copy,
        dirty: false,
        validation,
    })
}

impl ArtifactEditDraft {
    pub fn replace_text(&mut self, text: String) -> Result<(), String> {
        if self.draft_kind != ArtifactDraftKind::Lyrics {
            return Err("only a lyrics draft accepts plain text".to_string());
        }
        self.working_copy = ArtifactDraftContent::Text(text);
        self.dirty = true;
        self.validation = validate_draft_content(self.draft_kind, &self.working_copy);
        Ok(())
    }

    /// Replaces the lossless structured working copy. Unknown object fields
    /// are retained when callers edit the existing value in place.
    pub fn replace_json(&mut self, value: serde_json::Value) -> Result<(), String> {
        if self.draft_kind == ArtifactDraftKind::Lyrics {
            return Err("a lyrics draft accepts plain text".to_string());
        }
        self.working_copy = ArtifactDraftContent::Json(value);
        self.dirty = true;
        self.validation = validate_draft_content(self.draft_kind, &self.working_copy);
        Ok(())
    }
}

static DRAFT_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn commit_artifact_edit(
    cache: &CacheDir,
    draft: &ArtifactEditDraft,
    options: ArtifactSaveOptions,
) -> Result<ArtifactDraftCommit, String> {
    if !draft.dirty {
        return Err("the artifact draft has no changes".to_string());
    }
    if draft.validation.status == ArtifactHealthStatus::Invalid {
        return Err(format!(
            "artifact draft is invalid: {}",
            draft.validation.messages.join(" ")
        ));
    }
    let source = revision_by_id(&draft.source.file_hash, &draft.source.revision_id)
        .ok_or_else(|| "the source revision was deleted while the draft was open".to_string())?;
    if source.content_hash != draft.original_content_hash {
        return Err("the source revision changed while the draft was open".to_string());
    }
    let current_active =
        load_active_artifact(&draft.source.file_hash, draft.output_kind).map(|active| active.id);
    if current_active != draft.original_active_revision_id && !options.fork_from_old_revision {
        return Err(
            "the Active revision changed while the draft was open; reopen it or explicitly fork from the old revision"
                .to_string(),
        );
    }

    let bytes = match &draft.working_copy {
        ArtifactDraftContent::Text(text) => serde_json::to_vec_pretty(&serde_json::json!({
            "lines": text.lines().collect::<Vec<_>>()
        }))
        .map_err(|error| error.to_string())?,
        ArtifactDraftContent::Json(value) => {
            serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?
        }
    };
    let draft_dir = cache.path.join(".artifact-drafts");
    std::fs::create_dir_all(&draft_dir).map_err(|error| error.to_string())?;
    let sequence = DRAFT_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let extension = "json";
    let temporary = draft_dir.join(format!(
        ".{}-{}-{}.{extension}.tmp",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        sequence
    ));
    let write_result = (|| -> Result<(), String> {
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        file.write_all(&bytes).map_err(|error| error.to_string())?;
        file.flush().map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())
    })();
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    let store = ArtifactStore::new(&cache.path)?;
    let captured = store.capture(&draft.source.file_hash, draft.output_kind, &temporary);
    let _ = std::fs::remove_file(&temporary);
    let (path, content_hash, byte_size) = captured?;
    let producer = match draft.draft_kind {
        ArtifactDraftKind::Lyrics => "user.lyrics_editor",
        ArtifactDraftKind::TimedTranscript => "user.timed_transcript_editor",
        ArtifactDraftKind::StructuredJson => "user.artifact_editor",
    };
    let revision = ArtifactRevision {
        id: format!(
            "{}:{}:{}",
            draft.source.file_hash,
            kind_string(draft.output_kind),
            content_hash
        ),
        file_hash: draft.source.file_hash.clone(),
        kind: draft.output_kind,
        path,
        content_hash,
        producer_node: AnalysisNodeId::new(producer),
        input_revisions: vec![draft.source.revision_id.clone()],
        config_hash: format!("source:{}", draft.source.revision_id),
        algorithm_version: format!("artifact-edit-v1/app-{}", env!("CARGO_PKG_VERSION")),
        created_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64,
        byte_size,
        active: false,
        legacy: false,
        invalidated: false,
    };
    record_artifact_revision(&revision)?;
    if options.set_active {
        crate::analysis_artifact::set_active_artifact_revision(
            &cache.path,
            &revision.file_hash,
            revision.kind,
            &revision.id,
        )?;
    }
    let downstream_impact = (options.mode == ArtifactSaveMode::SaveAndRunDownstream)
        .then(|| preview_kind_downstream_impact(&revision.file_hash, revision.kind))
        .transpose()?;
    Ok(ArtifactDraftCommit {
        revision,
        downstream_impact,
        requires_downstream_confirmation: options.mode == ArtifactSaveMode::SaveAndRunDownstream,
    })
}

pub fn artifact_editor_text(reference: &ArtifactRef) -> Result<String, String> {
    let inspection = inspect_artifact(reference)?;
    let revision = inspection.artifact;
    let bytes = bounded_read(&revision.path, 2 * 1024 * 1024)?;
    match revision.kind {
        ArtifactKind::LyricsInput => {
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
                && let Some(lines) = value.get("lines").and_then(|value| value.as_array())
            {
                return Ok(lines
                    .iter()
                    .filter_map(|line| line.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"));
            }
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
        ArtifactKind::RecognizedText => {
            let value: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if let Some(text) = value.get("text").and_then(|value| value.as_str()) {
                return Ok(text.to_string());
            }
            if let Some(segments) = value.get("segments").and_then(|value| value.as_array()) {
                return Ok(segments
                    .iter()
                    .filter_map(|segment| segment.get("text").and_then(|value| value.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n"));
            }
            Ok(serde_json::to_string_pretty(&value).unwrap_or_default())
        }
        ArtifactKind::TimedTranscript | ArtifactKind::AsrSegments => {
            let value: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            let Some(segments) = value.get("segments").and_then(|value| value.as_array()) else {
                return Ok(serde_json::to_string_pretty(&value).unwrap_or_default());
            };
            let mut lines = Vec::new();
            for segment in segments {
                let text = segment
                    .get("text")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .trim();
                if text.is_empty() {
                    continue;
                }
                let seconds = segment
                    .get("start")
                    .and_then(|value| value.as_f64())
                    .unwrap_or(0.0)
                    .max(0.0);
                let centiseconds = (seconds * 100.0).round() as u64;
                lines.push(format!(
                    "[{:02}:{:02}.{:02}]{}",
                    centiseconds / 6000,
                    centiseconds / 100 % 60,
                    centiseconds % 100,
                    text
                ));
            }
            Ok(lines.join("\n"))
        }
        _ => Err("this artifact kind does not provide an editable lyrics working copy".to_string()),
    }
}
