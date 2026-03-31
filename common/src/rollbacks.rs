use std::marker::PhantomData;

use crate::BlockNumber;
use anyhow::{anyhow, Result};

//
// Storage Trait (with associated type)
//
pub trait StateStore {
    type State;

    fn load(&self) -> Option<Self::State>;
    fn save(&mut self, state: Self::State);
}

//
// RollbackChecker
//
pub struct RollbackChecker<T: StateStore> {
    capture_number: u64,
    store: T,
}

impl<T: StateStore> RollbackChecker<T>
where
    T::State: PartialEq + Clone,
{
    pub fn new(capture_number: BlockNumber, store: T) -> Self {
        Self {
            capture_number,
            store,
        }
    }

    pub fn check(&mut self, state: &T::State, actual_number: u64) -> Result<()> {
        if actual_number != self.capture_number {
            return Ok(());
        }

        match self.store.load() {
            Some(captured) if state != &captured => {
                Err(anyhow!("State mismatch at {}", self.capture_number))
            }
            Some(_) => Ok(()),
            None => {
                tracing::info!("Captured state at {}", self.capture_number);
                self.store.save(state.clone());
                Ok(())
            }
        }
    }
}

//
// In-memory store
//
#[derive(Default)]
pub struct RollbackMemoryStore<S> {
    state: Option<S>,
}

impl<S: Clone> StateStore for RollbackMemoryStore<S> {
    type State = S;

    fn load(&self) -> Option<S> {
        self.state.clone()
    }

    fn save(&mut self, state: S) {
        self.state = Some(state);
    }
}

//
// File store (disk-backed)
//
pub struct RollbackFileStore<S> {
    path: String,
    _marker: PhantomData<S>,
}

impl<S> RollbackFileStore<S> {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            _marker: PhantomData,
        }
    }
}

impl<S> StateStore for RollbackFileStore<S>
where
    S: serde::Serialize + serde::de::DeserializeOwned,
{
    type State = S;

    fn load(&self) -> Option<S> {
        std::fs::read(&self.path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok())
    }

    fn save(&mut self, state: S) {
        if let Ok(bytes) = serde_json::to_vec(&state) {
            let _ = std::fs::write(&self.path, bytes);
        }
    }
}
