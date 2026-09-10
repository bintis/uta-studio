//! CPU preparation owned by one native task. Never stores graphs or device
//! tensors: those must stay inside their model/precision/backend owner.
use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::rc::Rc;

#[derive(Default)]
struct Context {
    prepared: HashMap<(TypeId, String), Rc<dyn Any>>,
    hits: usize,
    graph_hits: usize,
    resident_copy_bytes: u64,
    arena_hits: usize,
}
thread_local! { static CURRENT: RefCell<Option<Context>> = const { RefCell::new(None) }; }
pub struct Scope {
    previous: Option<Context>,
    thread: PhantomData<Rc<()>>,
}
impl Scope {
    pub fn enter(enabled: bool) -> Self {
        Self {
            previous: CURRENT.with(|current| current.replace(enabled.then(Context::default))),
            thread: PhantomData,
        }
    }
    pub fn arena_hits(&self) -> usize {
        CURRENT.with(|current| {
            current
                .borrow()
                .as_ref()
                .map_or(0, |context| context.arena_hits)
        })
    }
    pub fn resident_copy_bytes(&self) -> u64 {
        CURRENT.with(|current| {
            current
                .borrow()
                .as_ref()
                .map_or(0, |context| context.resident_copy_bytes)
        })
    }
    pub fn graph_hits(&self) -> usize {
        CURRENT.with(|current| {
            current
                .borrow()
                .as_ref()
                .map_or(0, |context| context.graph_hits)
        })
    }
    pub fn preparation_hits(&self) -> usize {
        CURRENT.with(|current| current.borrow().as_ref().map_or(0, |context| context.hits))
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        CURRENT.with(|current| current.replace(self.previous.take()));
    }
}
pub(crate) fn record_arena_reuse() {
    CURRENT.with(|current| {
        if let Some(context) = current.borrow_mut().as_mut() {
            context.arena_hits += 1;
        }
    });
}

pub(crate) fn record_resident_copy(bytes: usize) {
    CURRENT.with(|current| {
        if let Some(context) = current.borrow_mut().as_mut() {
            context.resident_copy_bytes = context.resident_copy_bytes.saturating_add(bytes as u64);
        }
    });
}

pub(crate) fn record_graph_reuse() {
    CURRENT.with(|current| {
        if let Some(context) = current.borrow_mut().as_mut() {
            context.graph_hits += 1;
        }
    });
}

pub(crate) fn enabled() -> bool {
    CURRENT.with(|current| current.borrow().is_some())
}
pub(crate) fn prepare<T: 'static>(name: String, build: impl FnOnce() -> T) -> Rc<T> {
    let key = (TypeId::of::<T>(), name);
    let found = CURRENT.with(|current| {
        let mut current = current.borrow_mut();
        let context = current.as_mut()?;
        let value = context.prepared.get(&key)?.clone().downcast::<T>().ok()?;
        context.hits += 1;
        Some(value)
    });
    if let Some(value) = found {
        return value;
    }
    let value = Rc::new(build());
    CURRENT.with(|current| {
        if let Some(context) = current.borrow_mut().as_mut() {
            context.prepared.insert(key, value.clone());
        }
    });
    value
}

/// Scratch is caller-thread local. Every transform replaces its full FFT input;
/// sharing the allocation does not share input features or normalization.
pub(crate) struct Scratch {
    pub buffer: Vec<rustfft::num_complex::Complex32>,
    pub work: Vec<rustfft::num_complex::Complex32>,
}
impl Scratch {
    pub(crate) fn new(length: usize, work: usize) -> Self {
        Self {
            buffer: vec![Default::default(); length],
            work: vec![Default::default(); work],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preparation_is_task_owned_and_disabled_nested_scopes_do_not_inherit_it() {
        let scope = Scope::enter(true);
        let first = prepare("fixture".into(), || vec![1usize]);
        let weak = Rc::downgrade(&first);
        let second = prepare("fixture".into(), || panic!("must reuse"));
        assert!(Rc::ptr_eq(&first, &second));
        assert_eq!(scope.preparation_hits(), 1);
        {
            let _disabled = Scope::enter(false);
            let plain = prepare("fixture".into(), || vec![2usize]);
            assert!(!Rc::ptr_eq(&first, &plain));
        }
        assert!(Rc::ptr_eq(
            &first,
            &prepare("fixture".into(), || vec![3usize])
        ));
        drop(first);
        drop(second);
        assert!(weak.upgrade().is_some());
        drop(scope);
        assert!(weak.upgrade().is_none());
    }
}
