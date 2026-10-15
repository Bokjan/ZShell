//! A store's copy of its part of the vault's data (see [`Part`]).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock, RwLockReadGuard};

use super::{Data, Vault};

/// A store's copy of its part of the vault's data, which answers queries without waiting for
/// the vault (held while it writes its file). The copy is read again by itself whenever the
/// vault's data has been replaced as a whole since (unlocked, made or reset, see
/// [`Vault::generation`]), so that nothing has to tell each store; the store replaces it
/// after each change it writes.
pub struct Part<T> {
    vault: Arc<Vault>,
    /// Takes the store's part out of the data; given the default data while the vault is
    /// locked.
    take: fn(Data) -> T,
    copy: RwLock<T>,
    /// The vault's generation the copy was read at.
    read_at: AtomicU64,
}

impl<T> Part<T> {
    pub fn new(vault: Arc<Vault>, take: fn(Data) -> T) -> Self {
        let read_at = vault.generation();
        let copy = take(vault.data().unwrap_or_default());
        Self { vault, take, copy: RwLock::new(copy), read_at: AtomicU64::new(read_at) }
    }

    pub fn vault(&self) -> &Arc<Vault> {
        &self.vault
    }

    /// The copy, read again first if the vault's data has been replaced since. Not to be
    /// called while holding it.
    pub fn read(&self) -> RwLockReadGuard<'_, T> {
        let generation = self.vault.generation();
        if self.read_at.load(Ordering::SeqCst) != generation {
            let mut copy = self.copy.write().unwrap();
            // Read once only by the queries that found it out of date together. Read after
            // the generation: a vault replaced in between is read again next time.
            if self.read_at.load(Ordering::SeqCst) != generation {
                *copy = (self.take)(self.vault.data().unwrap_or_default());
                self.read_at.store(generation, Ordering::SeqCst);
            }
        }
        self.copy.read().unwrap()
    }

    /// Replaces the copy with what the store has just written.
    pub fn replace(&self, value: T) {
        *self.copy.write().unwrap() = value;
    }
}
