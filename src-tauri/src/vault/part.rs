//! A store's copy of its part of the vault's data (see [`Part`]).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard};

use super::{Data, Vault};
use crate::error::Result;

/// A store's copy of its part of the vault's data, taken out of it once (a store may check and
/// repair what it reads), which answers queries without going back to the data. The copy is
/// read again by itself whenever the vault's data has been replaced as a whole since
/// (unlocked, made or reset, see [`Vault::generation`]), so that nothing has to tell each
/// store; [`Part::update`] replaces it with what it wrote.
pub struct Part<T> {
    vault: Arc<Vault>,
    /// Takes the store's part out of the data; given the default data while the vault is
    /// locked.
    take: fn(&Data) -> T,
    copy: RwLock<T>,
    /// The vault's generation the copy was read at.
    read_at: AtomicU64,
    /// Held by a change from writing it to replacing the copy, so that an earlier change
    /// never replaces a later one's copy.
    writing: Mutex<()>,
}

impl<T> Part<T> {
    pub fn new(vault: Arc<Vault>, take: fn(&Data) -> T) -> Self {
        let read_at = vault.generation();
        let copy = Self::take_from(&vault, take);
        Self { vault, take, copy: RwLock::new(copy), read_at: AtomicU64::new(read_at), writing: Mutex::new(()) }
    }

    fn take_from(vault: &Vault, take: fn(&Data) -> T) -> T {
        match vault.data() {
            Ok(data) => take(&data),
            Err(_) => take(&Data::default()),
        }
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
                *copy = Self::take_from(&self.vault, self.take);
                self.read_at.store(generation, Ordering::SeqCst);
            }
        }
        self.copy.read().unwrap()
    }

    /// Changes the store's part, and anything else in the data, and writes them (see
    /// [`Vault::update`]); then keeps the part as changed. `change` gets the part as taken out
    /// of the data as it is now, not the copy, which may be older, and the data, into which it
    /// puts the part back.
    pub fn update<R>(&self, change: impl FnOnce(&mut T, &mut Data) -> Result<R>) -> Result<R> {
        let _writing = self.writing.lock().unwrap();
        let mut changed = None;
        let value = self.vault.update(|data| {
            let mut part = (self.take)(data);
            let value = change(&mut part, data)?;
            // Stays put while the vault's state is held.
            changed = Some((part, self.vault.generation()));
            Ok(value)
        })?;
        if let Some((part, generation)) = changed {
            *self.copy.write().unwrap() = part;
            self.read_at.store(generation, Ordering::SeqCst);
        }
        Ok(value)
    }
}
