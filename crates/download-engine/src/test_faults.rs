//! Debug-only fault injection used by integration tests.
//!
//! This module exists only in debug builds. It is not part of the stable API and must
//! never be used by the desktop application.

use crate::DownloadId;
use std::{
    collections::{HashMap, HashSet},
    io,
    sync::{Arc, Mutex, OnceLock},
};
use tokio::sync::oneshot;

/// A one-shot gate that blocks a worker until the test releases it.
pub struct Gate {
    hit_tx: Mutex<Option<oneshot::Sender<()>>>,
    hit_rx: Mutex<Option<oneshot::Receiver<()>>>,
    release_tx: Mutex<Option<oneshot::Sender<()>>>,
    release_rx: Mutex<Option<oneshot::Receiver<()>>>,
}

impl Gate {
    fn new() -> Arc<Self> {
        let (hit_tx, hit_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        Arc::new(Self {
            hit_tx: Mutex::new(Some(hit_tx)),
            hit_rx: Mutex::new(Some(hit_rx)),
            release_tx: Mutex::new(Some(release_tx)),
            release_rx: Mutex::new(Some(release_rx)),
        })
    }

    /// Wait until the worker has reached the gate.
    pub async fn wait_until_blocked(&self) {
        let receiver = self
            .hit_rx
            .lock()
            .unwrap()
            .take()
            .expect("gate can only be awaited once");
        let _ = receiver.await;
    }

    /// Allow the worker to continue.
    pub fn release(&self) {
        if let Some(sender) = self.release_tx.lock().unwrap().take() {
            let _ = sender.send(());
        }
    }

    async fn block(&self) {
        if let Some(sender) = self.hit_tx.lock().unwrap().take() {
            let _ = sender.send(());
        }
        let receiver = self
            .release_rx
            .lock()
            .unwrap()
            .take()
            .expect("gate can only be released once");
        let _ = receiver.await;
    }
}

#[derive(Default)]
struct Hooks {
    checkpoint_gates: HashMap<DownloadId, Arc<Gate>>,
    publication_entry_gates: HashMap<DownloadId, Arc<Gate>>,
    publication_gates: HashMap<DownloadId, Arc<Gate>>,
    fail_checkpoint: HashSet<DownloadId>,
    fail_publication: HashSet<DownloadId>,
}

fn hooks() -> &'static Mutex<Hooks> {
    static HOOKS: OnceLock<Mutex<Hooks>> = OnceLock::new();
    HOOKS.get_or_init(|| Mutex::new(Hooks::default()))
}

/// Block the next checkpoint for this download.
pub fn block_checkpoint(id: DownloadId) -> Arc<Gate> {
    let gate = Gate::new();
    hooks()
        .lock()
        .unwrap()
        .checkpoint_gates
        .insert(id, gate.clone());
    gate
}

/// Block publication before the engine takes the state lock.
pub fn block_publication_entry(id: DownloadId) -> Arc<Gate> {
    let gate = Gate::new();
    hooks()
        .lock()
        .unwrap()
        .publication_entry_gates
        .insert(id, gate.clone());
    gate
}

/// Block publication after `ready_to_publish` is durable but before linking.
pub fn block_publication(id: DownloadId) -> Arc<Gate> {
    let gate = Gate::new();
    hooks()
        .lock()
        .unwrap()
        .publication_gates
        .insert(id, gate.clone());
    gate
}

/// Fail the next checkpoint with a simulated disk-full error.
pub fn fail_checkpoint(id: DownloadId) {
    hooks().lock().unwrap().fail_checkpoint.insert(id);
}

/// Fail publication after `ready_to_publish` is durable.
pub fn fail_publication(id: DownloadId) {
    hooks().lock().unwrap().fail_publication.insert(id);
}

/// Remove every hook for a download.
pub fn clear(id: DownloadId) {
    let mut hooks = hooks().lock().unwrap();
    hooks.checkpoint_gates.remove(&id);
    hooks.publication_entry_gates.remove(&id);
    hooks.publication_gates.remove(&id);
    hooks.fail_checkpoint.remove(&id);
    hooks.fail_publication.remove(&id);
}

pub(crate) async fn checkpoint(id: DownloadId) -> Option<io::Error> {
    let gate = hooks().lock().unwrap().checkpoint_gates.remove(&id);
    if let Some(gate) = gate {
        gate.block().await;
        return None;
    }
    let fail = hooks().lock().unwrap().fail_checkpoint.remove(&id);
    fail.then(|| io::Error::other("injected disk full"))
}

pub(crate) async fn publication_entry(id: DownloadId) -> Option<io::Error> {
    let gate = hooks().lock().unwrap().publication_entry_gates.remove(&id);
    if let Some(gate) = gate {
        gate.block().await;
    }
    None
}

pub(crate) async fn publication(id: DownloadId) -> Option<io::Error> {
    let gate = hooks().lock().unwrap().publication_gates.remove(&id);
    if let Some(gate) = gate {
        gate.block().await;
        return None;
    }
    let fail = hooks().lock().unwrap().fail_publication.remove(&id);
    fail.then(|| io::Error::other("injected publication failure"))
}
