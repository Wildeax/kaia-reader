//! Process-wide store registry. A handle is `(generation << 32) | (slot + 1)`.
//!
//! Closing a store empties its slot and bumps the slot's generation, so an old
//! handle no longer matches. Handle 0 never matches because the low half is
//! at least 1.

use std::sync::{Mutex, MutexGuard, PoisonError};

use kaia_storage::Store;

use crate::error::Failure;

struct Slot {
    generation: u32,
    store: Option<Store>,
}

struct Registry {
    slots: Vec<Slot>,
    /// Indexes of empty slots, reused before the vector grows.
    free: Vec<usize>,
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    slots: Vec::new(),
    free: Vec::new(),
});

fn lock() -> MutexGuard<'static, Registry> {
    // A panic in another call poisons the lock. The registry stays consistent
    // because every change is a single field write, so take the inner value.
    REGISTRY.lock().unwrap_or_else(PoisonError::into_inner)
}

fn encode(index: usize, generation: u32) -> u64 {
    // Slots are bounded by open stores, far below u32::MAX, so the index fits.
    (u64::from(generation) << 32) | (index as u64 + 1)
}

fn decode(handle: u64) -> Option<(usize, u32)> {
    let low = handle & 0xFFFF_FFFF;
    if low == 0 {
        return None;
    }
    let index = usize::try_from(low - 1).ok()?;
    Some((index, (handle >> 32) as u32))
}

/// Registers an open store and returns its handle.
pub fn insert(store: Store) -> u64 {
    let mut reg = lock();
    let index = match reg.free.pop() {
        Some(index) => index,
        None => {
            reg.slots.push(Slot {
                generation: 1,
                store: None,
            });
            reg.slots.len() - 1
        }
    };
    let slot = &mut reg.slots[index];
    slot.store = Some(store);
    encode(index, slot.generation)
}

/// Runs `f` on the store behind `handle`. The registry lock is held for the
/// whole call, so calls on different stores run one at a time.
pub fn with_store<T>(handle: u64, f: impl FnOnce(&mut Store) -> Result<T, Failure>) -> Result<T, Failure> {
    let mut reg = lock();
    let (index, generation) = decode(handle).ok_or_else(Failure::bad_handle)?;
    let slot = reg.slots.get_mut(index).ok_or_else(Failure::bad_handle)?;
    if slot.generation != generation {
        return Err(Failure::bad_handle());
    }
    let store = slot.store.as_mut().ok_or_else(Failure::bad_handle)?;
    f(store)
}

/// Closes the store behind `handle`. The handle is stale afterwards.
pub fn close(handle: u64) -> Result<(), Failure> {
    let mut reg = lock();
    let (index, generation) = decode(handle).ok_or_else(Failure::bad_handle)?;
    let valid = reg
        .slots
        .get(index)
        .is_some_and(|slot| slot.generation == generation && slot.store.is_some());
    if !valid {
        return Err(Failure::bad_handle());
    }
    let slot = &mut reg.slots[index];
    let closed = slot.store.take();
    slot.generation = slot.generation.wrapping_add(1);
    reg.free.push(index);
    // Drop the store after the bookkeeping, still under the lock.
    drop(closed);
    Ok(())
}
