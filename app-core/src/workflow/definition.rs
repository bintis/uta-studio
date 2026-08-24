use std::time::{SystemTime, UNIX_EPOCH};

use crate::{config::AppConfig, library_db};

use super::{
    NodeCapability, StoredWorkflow, WorkflowCompileError, WorkflowDefinition,
    WorkflowExecutionSnapshot, WorkflowLayout, builtin_capabilities, compile_workflow,
    workflow_from_audio_settings,
};

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

pub fn list_workflow_capabilities() -> Vec<NodeCapability> {
    builtin_capabilities()
}

pub fn load_song_workflow(file_hash: &str) -> Result<StoredWorkflow, String> {
    if let Some((json, updated_at_ms)) =
        library_db::song_workflow_get(file_hash).map_err(|error| error.to_string())?
    {
        let mut stored: StoredWorkflow = serde_json::from_str(&json)
            .map_err(|error| format!("invalid saved workflow: {error}"))?;
        stored.updated_at_ms = updated_at_ms;
        return Ok(stored);
    }
    let config = AppConfig::load();
    let settings = config
        .audio_processing
        .as_ref()
        .cloned()
        .unwrap_or_else(|| {
            crate::audio_processing::AudioProcessingSettings::from_legacy_separator(
                config.separator(),
            )
        });
    Ok(StoredWorkflow {
        definition: workflow_from_audio_settings(file_hash, &settings),
        layout: WorkflowLayout::default(),
        updated_at_ms: 0,
    })
}

pub fn save_song_workflow(
    file_hash: &str,
    mut definition: WorkflowDefinition,
    layout: WorkflowLayout,
) -> Result<StoredWorkflow, String> {
    compile_workflow(&definition).map_err(|error| error.to_string())?;
    let existing_revision = library_db::song_workflow_get(file_hash)
        .map_err(|error| error.to_string())?
        .and_then(|(json, _)| serde_json::from_str::<StoredWorkflow>(&json).ok())
        .map(|stored| stored.definition.revision)
        .unwrap_or(0);
    definition.revision = existing_revision.saturating_add(1);
    let stored = StoredWorkflow {
        definition,
        layout,
        updated_at_ms: now_ms(),
    };
    let json = serde_json::to_string(&stored).map_err(|error| error.to_string())?;
    library_db::song_workflow_set(file_hash, &json, stored.updated_at_ms)
        .map_err(|error| error.to_string())?;
    Ok(stored)
}

pub fn preview_workflow_compile(
    definition: &WorkflowDefinition,
) -> Result<WorkflowExecutionSnapshot, WorkflowCompileError> {
    compile_workflow(definition)
}

/// Reorders two adjacent role-preserving audio transformations by rewriting
/// semantic edges. Layout coordinates are never consulted.
pub fn reorder_audio_transformation(
    definition: &mut WorkflowDefinition,
    node_id: &super::WorkflowNodeId,
    earlier: bool,
) -> Result<(), String> {
    let original = definition.clone();
    let capabilities = builtin_capabilities();
    let role_preserving = definition
        .nodes
        .iter()
        .filter_map(|node| {
            capabilities
                .iter()
                .find(|capability| capability.id == node.capability_id)
                .map(|capability| (node.instance_id.clone(), capability.preserves_audio_role))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let selected = role_preserving
        .get(node_id)
        .copied()
        .ok_or_else(|| "workflow node not found".to_string())?;
    if !selected {
        return Err("only role-preserving audio transformations can be reordered".to_string());
    }

    let selected_incoming = definition
        .edges
        .iter()
        .position(|edge| &edge.to.node == node_id && edge.to.port == "audio")
        .ok_or_else(|| "selected transformation has no audio input".to_string())?;
    let selected_outgoing = definition
        .edges
        .iter()
        .position(|edge| &edge.from.node == node_id && edge.from.port == "audio")
        .ok_or_else(|| "selected transformation has no audio output".to_string())?;

    if earlier {
        let previous_id = definition.edges[selected_incoming].from.node.clone();
        if !role_preserving.get(&previous_id).copied().unwrap_or(false) {
            return Err("the preceding node is a fixed branch boundary".to_string());
        }
        let previous_incoming = definition
            .edges
            .iter()
            .position(|edge| edge.to.node == previous_id && edge.to.port == "audio")
            .ok_or_else(|| "preceding transformation has no audio input".to_string())?;
        let upstream = definition.edges[previous_incoming].from.clone();
        definition.edges[previous_incoming].to.node = node_id.clone();
        definition.edges[previous_incoming].to.port = "audio".to_string();
        definition.edges[selected_incoming].from.node = node_id.clone();
        definition.edges[selected_incoming].from.port = "audio".to_string();
        definition.edges[selected_incoming].to.node = previous_id;
        definition.edges[selected_incoming].to.port = "audio".to_string();
        debug_assert_eq!(definition.edges[previous_incoming].from, upstream);
    } else {
        let next_id = definition.edges[selected_outgoing].to.node.clone();
        if !role_preserving.get(&next_id).copied().unwrap_or(false) {
            return Err("the following node is a fixed branch boundary".to_string());
        }
        let next_outgoing = definition
            .edges
            .iter()
            .position(|edge| edge.from.node == next_id && edge.from.port == "audio")
            .ok_or_else(|| "following transformation has no audio output".to_string())?;
        let downstream = definition.edges[next_outgoing].to.clone();
        definition.edges[selected_outgoing].from.node = next_id.clone();
        definition.edges[selected_outgoing].from.port = "audio".to_string();
        definition.edges[selected_outgoing].to.node = node_id.clone();
        definition.edges[selected_outgoing].to.port = "audio".to_string();
        definition.edges[next_outgoing].from.node = node_id.clone();
        definition.edges[next_outgoing].from.port = "audio".to_string();
        debug_assert_eq!(definition.edges[next_outgoing].to, downstream);
    }
    if let Err(error) = compile_workflow(definition) {
        *definition = original;
        return Err(error.to_string());
    }
    Ok(())
}

/// Inserts a second instance of a role-preserving transformation directly
/// after the selected instance. Downstream semantic audio edges and terminal
/// analyzer attachments move to the duplicate so the new instance is real.
pub fn duplicate_audio_transformation(
    definition: &mut WorkflowDefinition,
    node_id: &super::WorkflowNodeId,
) -> Result<super::WorkflowNodeId, String> {
    let original = definition.clone();
    let capabilities = builtin_capabilities();
    let source = definition
        .nodes
        .iter()
        .find(|node| &node.instance_id == node_id)
        .cloned()
        .ok_or_else(|| "workflow node not found".to_string())?;
    let capability = capabilities
        .iter()
        .find(|capability| capability.id == source.capability_id)
        .ok_or_else(|| "workflow capability is unavailable".to_string())?;
    if !capability.preserves_audio_role || !capability.allows_multiple_instances {
        return Err(
            "only repeatable role-preserving audio transformations can be duplicated".to_string(),
        );
    }
    let instance_id = (1..)
        .map(|suffix| super::WorkflowNodeId::new(format!("{}-copy-{suffix}", node_id.as_str())))
        .find(|candidate| {
            definition
                .nodes
                .iter()
                .all(|node| node.instance_id != *candidate)
        })
        .expect("an unused workflow instance suffix exists");
    let mut duplicate = source;
    duplicate.instance_id = instance_id.clone();
    duplicate.priority = duplicate.priority.saturating_sub(1);

    let mut redirected = false;
    for edge in &mut definition.edges {
        if edge.from.node == *node_id && edge.from.port == "audio" {
            edge.from.node = instance_id.clone();
            redirected = true;
        }
    }
    for binding in &mut definition.analyzer_bindings {
        if binding.source.node == *node_id && binding.source.port == "audio" {
            binding.source.node = instance_id.clone();
            redirected = true;
        }
    }
    if !redirected {
        return Err("selected transformation has no audio output".to_string());
    }
    definition.nodes.push(duplicate);
    definition.edges.push(super::WorkflowEdge {
        from: super::WorkflowPortRef {
            node: node_id.clone(),
            port: "audio".to_string(),
        },
        to: super::WorkflowPortRef {
            node: instance_id.clone(),
            port: "audio".to_string(),
        },
    });
    if let Err(error) = compile_workflow(definition) {
        *definition = original;
        return Err(error.to_string());
    }
    Ok(instance_id)
}

pub fn set_workflow_execution_policy(
    definition: &mut WorkflowDefinition,
    node_id: &super::WorkflowNodeId,
    policy: super::ExecutionPolicy,
) -> Result<(), String> {
    let original = definition.clone();
    let node = definition
        .nodes
        .iter_mut()
        .find(|node| &node.instance_id == node_id)
        .ok_or_else(|| "workflow node not found".to_string())?;
    node.execution_policy = policy;
    if let Err(error) = compile_workflow(definition) {
        *definition = original;
        return Err(error.to_string());
    }
    Ok(())
}

pub fn set_workflow_priority(
    definition: &mut WorkflowDefinition,
    node_id: &super::WorkflowNodeId,
    priority: i32,
) -> Result<(), String> {
    let node = definition
        .nodes
        .iter_mut()
        .find(|node| &node.instance_id == node_id)
        .ok_or_else(|| "workflow node not found".to_string())?;
    node.priority = priority.clamp(-100, 100);
    Ok(())
}

pub fn bind_workflow_analyzer(
    definition: &mut WorkflowDefinition,
    analyzer_node: &super::WorkflowNodeId,
    source: super::WorkflowPortRef,
) -> Result<(), String> {
    let original = definition.clone();
    let binding = definition
        .analyzer_bindings
        .iter_mut()
        .find(|binding| &binding.analyzer_node == analyzer_node)
        .ok_or_else(|| "selected node is not an analyzer attachment".to_string())?;
    binding.source = source;
    if let Err(error) = compile_workflow(definition) {
        *definition = original;
        return Err(error.to_string());
    }
    Ok(())
}
