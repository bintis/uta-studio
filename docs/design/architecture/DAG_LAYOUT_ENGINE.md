# Dynamic Workflow DAG layout engine

## Scope

Uta! Studio renders Advanced Graph with native Bevy UI. The graph is a view of
the exact compiled Processing Studio workflow attached to Engine Preview and
Execution. No fixed presentation topology, special-case route geometry or
synthetic export node participates in graph construction.

The implementation is split across:

```text
desktop/src/studio/analysis_model.rs
  renderer-facing workflow nodes, bindings, terminal outputs and state

desktop/src/studio/analysis_model/workflow.rs
  exact Workflow wire/Engine-plan projection

desktop/src/studio/analysis_layout.rs
  variable node sizing, generic layered geometry, cache and orthogonal routing

desktop/src/studio/analysis_layout_order.rs
  stable Sugiyama crossing minimization

desktop/src/studio/analysis_layout_tests.rs
  deterministic generic geometry and routing regressions
```

## Authoritative sources

Source priority is:

1. an explicitly selected historical run's frozen Engine request and plan;
2. the selected song's queued or active frozen Engine request and plan;
3. the current in-memory compiled Processing Studio workflow when no execution
   context exists.

Queued task projection retains the accepted request and plan from the durable
Engine queue intent. A queued, active or historical context without a compiled
Workflow snapshot never falls back to the mutable current draft; the UI reports
that the compiled graph is unavailable.

## Projection rules

- every workflow instance becomes one compute node, including duplicates;
- semantic bindings determine edges;
- analyzer attachments retain their exact producer, consumer, ports and semantic type;
- terminal outputs remain node-owned port facts and never become synthetic nodes;
- disabled bindings remain visible as inactive dashed edges;
- priority is node metadata and never creates an edge;
- Engine states map directly to Ready, Deferred, Disabled, Profile skipped and
  Not requested;
- MINI is an exact subgraph that removes inactive nodes and dangling edges; it
  never creates shortcut dependencies;
- runtime events overlay nodes only by exact `node_id`; display-stage text never
  selects a node;
- historical event routes overlay the frozen plan without marking the last
  historical node as live-running;
- terminal export/package actions are outside the execution DAG.

## Layout pipeline

```text
WorkflowExecutionWireV1 + optional exact Engine workflow plan
        |
        v
RenderGraph nodes + semantic bindings
        |
        v
localized variable-size LayoutNodeSpec
        |
        v
validated topological order + longest-path ranks
        |
        v
stable weighted-median/transpose crossing minimization
        |
        v
variable-width columns with centered node stacks
        |
        v
orthogonal routing with distributed ports
        |
        v
RoutedGraph -> Bevy entities
```

The layout never infers placement from node ids. Runtime state and selection do
not enter the geometry cache key; topology, localized geometry, edge order and
stable node order do.

## Regression coverage

Focused tests cover arbitrary `workflow.*` ids, cycles failing closed,
variable-size non-overlap, routed endpoint integrity, localized cache identity,
duplicate/reordered transformations, exact analyzer attachments, terminal
outputs, disabled bindings, conditional node states, exact runtime overlays and
historical/current snapshot isolation.
