//! Host ownership and complete-output checks shared by native speech adapters.
use crate::Output;
use std::cell::{RefCell, RefMut};
use std::rc::Rc;

#[derive(Debug)]
pub struct EncodedAudio {
    pub rows: usize,
    pub width: usize,
    identity: Rc<()>,
}

#[derive(Default)]
pub(crate) struct Resident {
    execution: RefCell<()>,
    audio: RefCell<Option<Rc<()>>>,
}
impl Resident {
    pub fn lock(&self) -> Result<RefMut<'_, ()>, String> {
        self.execution
            .try_borrow_mut()
            .map_err(|_| "native speech session is already borrowed".into())
    }
    pub fn clear(&self) {
        self.audio.borrow_mut().take();
    }
    pub fn remember(&self, rows: usize, width: usize) -> EncodedAudio {
        let identity = Rc::new(());
        self.audio.replace(Some(Rc::clone(&identity)));
        EncodedAudio {
            rows,
            width,
            identity,
        }
    }
    pub fn validate(&self, audio: &EncodedAudio) -> Result<(), String> {
        if self
            .audio
            .borrow()
            .as_ref()
            .is_some_and(|current| Rc::ptr_eq(current, &audio.identity))
        {
            Ok(())
        } else {
            Err("native audio is stale or belongs to another speech model".into())
        }
    }
}

pub(crate) fn matrix(
    output: &Output,
    name: &str,
    width: usize,
) -> Result<(usize, Vec<f32>), String> {
    let tensor = output.get(name)?;
    if tensor.shape.len() != 2 || tensor.shape[0] <= 0 || tensor.shape[1] != width as i64 {
        return Err(format!(
            "native {name} has unexpected matrix shape {:?}",
            tensor.shape
        ));
    }
    let values = tensor.f32()?;
    if values.iter().any(|value| !value.is_finite()) {
        return Err(format!("native {name} contains nonfinite values"));
    }
    Ok((tensor.shape[0] as usize, values.to_vec()))
}

pub(crate) fn position(output: &Output, expected: usize) -> Result<(), String> {
    if output.get("position")?.i64()? == [expected as i64] {
        Ok(())
    } else {
        Err("native speech decoder position disagrees with the host session".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Tensor, Timings, Values};
    #[test]
    fn audio_handles_follow_their_own_model_and_current_encoding() {
        let owner = Resident::default();
        let other = Resident::default();
        let first = owner.remember(2, 3);
        assert!(owner.validate(&first).is_ok());
        assert!(other.validate(&first).is_err());
        let second = owner.remember(4, 3);
        assert!(owner.validate(&first).is_err());
        assert!(owner.validate(&second).is_ok());
        owner.clear();
        assert!(owner.validate(&second).is_err());
    }
    #[test]
    fn session_borrow_prevents_encoding_over_active_kv_state() {
        let owner = Resident::default();
        let guard = owner.lock().unwrap();
        assert!(owner.lock().is_err());
        drop(guard);
        assert!(owner.lock().is_ok());
    }
    #[test]
    fn complete_matrix_checks_include_last_value_and_width() {
        let mut output = Output {
            tensors: [(
                "logits".into(),
                Tensor {
                    shape: vec![1, 3],
                    data: Values::F32(vec![1., 2., 3.]),
                },
            )]
            .into(),
            timings: Timings::default(),
        };
        assert_eq!(matrix(&output, "logits", 3).unwrap().1, [1., 2., 3.]);
        assert!(matrix(&output, "logits", 2).is_err());
        output.tensors.get_mut("logits").unwrap().data = Values::F32(vec![1., 2., f32::NAN]);
        assert!(matrix(&output, "logits", 3).is_err());
    }
}
