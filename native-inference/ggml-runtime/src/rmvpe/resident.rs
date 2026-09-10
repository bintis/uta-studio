//! One complete RMVPE window on its existing backend. All optional allocation
//! happens before the first compute; an execution failure is never retried.
use super::*;
use crate::resident::Matrix;
use std::collections::{BTreeMap, BTreeSet};

struct TensorGraph {
    run: GraphRun,
    input: TensorPtr,
    output: TensorPtr,
}
struct RecurrentGraph {
    run: GraphRun,
    input: TensorPtr,
    output: GruGraphOutput,
    used: bool,
}

pub(super) struct Window<'model> {
    model: &'model Rmvpe,
    frames: usize,
    chunks: Vec<(usize, usize)>,
    // Graphs, especially the head's external input references, drop before
    // the resident matrices declared below them.
    cnn: Option<TensorGraph>,
    recurrent: BTreeMap<(usize, usize), RecurrentGraph>,
    head: TensorGraph,
    cnn_output: Matrix<'model>,
    forward: Matrix<'model>,
    backward: Matrix<'model>,
    hidden: Matrix<'model>,
}
impl<'model> Window<'model> {
    pub fn new(model: &'model Rmvpe, frames: usize) -> Result<Self, String> {
        let backend = &model.backend;
        let api = model.api();
        let cnn_output = Matrix::new(backend, GRU_INPUT, frames)?;
        let forward = Matrix::new(backend, GRU_HIDDEN, frames)?;
        let backward = Matrix::new(backend, GRU_HIDDEN, frames)?;
        let hidden = Matrix::new(backend, GRU_HIDDEN, 1)?;
        let chunks = chunk_plan(frames);

        let mut run = GraphRun::new(
            Arc::clone(&backend.runtime),
            64 * 1024 * 1024,
            frames * 30 + 20_000,
        )?;
        let input = ggml!(
            api,
            ggml_new_tensor_2d(run.context, GGML_TYPE_F32, frames as i64, MEL_BINS as i64)
        );
        ggml!(api, ggml_set_input(input));
        let output = model.build_cnn_head(run.context, run.graph, input, frames)?;
        run.allocate(backend)?;
        let cnn = TensorGraph { run, input, output };

        let mut recurrent = BTreeMap::new();
        for length in chunks
            .iter()
            .map(|(_, length)| *length)
            .collect::<BTreeSet<_>>()
        {
            for direction in 0..2 {
                let mut run = GraphRun::new(
                    Arc::clone(&backend.runtime),
                    32 * 1024 * 1024,
                    length * 40 + 2000,
                )?;
                let input = ggml!(
                    api,
                    ggml_new_tensor_2d(run.context, GGML_TYPE_F32, GRU_INPUT as i64, length as i64)
                );
                ggml!(api, ggml_set_input(input));
                let output =
                    model.build_gru_chunk(run.context, run.graph, input, length, direction)?;
                run.allocate(backend)?;
                recurrent.insert(
                    (direction, length),
                    RecurrentGraph {
                        run,
                        input,
                        output,
                        used: false,
                    },
                );
            }
        }
        let mut run = GraphRun::new(Arc::clone(&backend.runtime), 8 * 1024 * 1024, 200)?;
        // Concatenation is a device graph copy, not a strided tensor_copy:
        // the latter requires identical layouts. Arithmetic is unchanged.
        let input = ggml!(
            api,
            ggml_concat(run.context, forward.tensor, backward.tensor, 0)
        );
        let output = model.build_output_head(run.context, run.graph, input)?;
        run.allocate(backend)?;
        let head = TensorGraph { run, input, output };
        Ok(Self {
            model,
            frames,
            chunks,
            cnn: Some(cnn),
            recurrent,
            head,
            cnn_output,
            forward,
            backward,
            hidden,
        })
    }

    pub fn run(&mut self, mel: &[f32]) -> Result<Vec<f32>, String> {
        let backend = &self.model.backend;
        let api = self.model.api();
        let cnn = self.cnn.take().expect("resident window executes once");
        set_f32(api, cnn.input, mel)?;
        cnn.run.compute(backend)?;
        self.cnn_output.copy_from(cnn.output, 0, self.frames)?;
        drop(cnn); // Only the compact output survives its final encoder consumer.
        for direction in 0..2 {
            // A new direction starts from exactly the same +0 hidden state as
            // the ordinary host path, never the previous direction's state.
            self.hidden.write(&vec![0.0; GRU_HIDDEN])?;
            for index in 0..self.chunks.len() {
                let index = if direction == 0 {
                    index
                } else {
                    self.chunks.len() - 1 - index
                };
                let (start, length) = self.chunks[index];
                let graph = self
                    .recurrent
                    .get_mut(&(direction, length))
                    .expect("prepared exact GRU shape");
                self.cnn_output.copy_to(graph.input, start, length)?;
                self.hidden.copy_to(graph.output.hidden_input, 0, 1)?;
                if graph.used {
                    crate::acceleration::record_graph_reuse();
                }
                graph.run.compute(backend)?;
                graph.used = true;
                self.hidden.copy_from(graph.output.hidden_final, 0, 1)?;
                let output = if direction == 0 {
                    &self.forward
                } else {
                    &self.backward
                };
                output.copy_from(graph.output.output, start, length)?;
            }
        }
        self.recurrent.clear(); // Hidden-state/chunk graph consumers have finished.
        self.head.run.compute(backend)?;
        get_f32(api, self.head.output)
    }
}

fn chunk_plan(frames: usize) -> Vec<(usize, usize)> {
    (0..frames)
        .step_by(GRU_CHUNK_FRAMES)
        .map(|start| (start, GRU_CHUNK_FRAMES.min(frames - start)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "explicit native CPU reference with a read-only RMVPE GGUF"]
    fn native_weighted_rmvpe_matches_ordinary_across_inputs_and_short_tails() {
        let directory = std::env::var("UTA_STUDIO_GGML_TEST_LIBRARY_DIR").unwrap();
        let path = std::env::var("UTA_STUDIO_GGML_TEST_MODEL_PATH").unwrap();
        let runtime = GgmlRuntime::load(Path::new(&directory)).unwrap();
        let device = runtime
            .devices()
            .unwrap()
            .into_iter()
            .find(|device| device.kind == crate::DeviceKind::Cpu)
            .unwrap();
        let model = Rmvpe::load(runtime, &device, Path::new(&path)).unwrap();
        for (index, frames) in [
            GRU_CHUNK_FRAMES * 2,
            GRU_CHUNK_FRAMES + GRU_CHUNK_FRAMES / 2,
            GRU_CHUNK_FRAMES * 2,
        ]
        .into_iter()
        .enumerate()
        {
            let mel = (0..MEL_BINS * frames)
                .map(|sample| -4.0 + (sample % 31) as f32 * 0.02 - index as f32)
                .collect::<Vec<_>>();
            let ordinary = {
                let _scope = crate::acceleration::Scope::enter(false);
                model.run_window(&mel, frames).unwrap()
            };
            let scope = crate::acceleration::Scope::enter(true);
            let retained = model.run_window(&mel, frames).unwrap();
            assert!(
                scope.resident_copy_bytes() > 0,
                "must consume real retained intermediates, not skip optional allocation"
            );
            if frames == GRU_CHUNK_FRAMES * 2 {
                assert!(scope.graph_hits() >= 2);
            }
            assert!(
                ordinary
                    .iter()
                    .chain(&retained)
                    .all(|value| value.is_finite())
            );
            assert_eq!(
                ordinary
                    .iter()
                    .map(|value| value.to_bits())
                    .collect::<Vec<_>>(),
                retained
                    .iter()
                    .map(|value| value.to_bits())
                    .collect::<Vec<_>>()
            );
            eprintln!(
                "RMVPE weighted CPU parity: frames={frames}, compared_values={}, resident_copy_bytes={}",
                ordinary.len(),
                scope.resident_copy_bytes()
            );
        }
    }

    #[test]
    fn resident_chunk_plan_preserves_both_directions_and_the_short_tail() {
        let frames = GRU_CHUNK_FRAMES * 2 + GRU_CHUNK_FRAMES / 2;
        let chunks = chunk_plan(frames);
        let forward = chunks
            .iter()
            .flat_map(|&(start, length)| start..start + length)
            .collect::<Vec<_>>();
        let backward = chunks
            .iter()
            .rev()
            .flat_map(|&(start, length)| (start..start + length).rev())
            .collect::<Vec<_>>();
        assert_eq!(forward, (0..frames).collect::<Vec<_>>());
        assert_eq!(backward, (0..frames).rev().collect::<Vec<_>>());
        assert_eq!(chunks.last().unwrap().1, GRU_CHUNK_FRAMES / 2);
    }
}
