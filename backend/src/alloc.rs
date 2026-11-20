//! Compteur d'allocation : ce que le tas contient **réellement**.
//!
//! La mémoire résidente d'un process mélange code, piles, `mmap` et tas : elle ne
//! dit pas « combien l'API garde pour ses données ». Un `GlobalAlloc` qui délègue
//! à l'allocateur système et tient deux compteurs répond à la question ; le prix
//! est un `fetch_add` par allocation, invisible devant le travail d'une requête.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

/// Octets vivants, et leur plus haut niveau depuis le démarrage.
static LIVE: AtomicU64 = AtomicU64::new(0);
static PEAK: AtomicU64 = AtomicU64::new(0);

struct Counting;

fn take(bytes: u64) {
    let now = LIVE.fetch_add(bytes, Ordering::Relaxed) + bytes;
    PEAK.fetch_max(now, Ordering::Relaxed);
}

/// Rend une allocation : jamais en dessous de zéro, même si un compteur a été
/// initialisé avant nous (bibliothèques système, tests).
fn give(bytes: u64) {
    let _ = LIVE.try_update(Ordering::Relaxed, Ordering::Relaxed, |live| {
        Some(live.saturating_sub(bytes))
    });
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            take(layout.size() as u64);
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            take(layout.size() as u64);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        give(layout.size() as u64);
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let grown = unsafe { System.realloc(pointer, layout, size) };
        if !grown.is_null() {
            let (before, after) = (layout.size() as u64, size as u64);
            if after >= before {
                take(after - before);
            } else {
                give(before - after);
            }
        }
        grown
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Octets actuellement alloués par le processus (tas ; hors piles et `mmap`).
pub fn live() -> u64 {
    LIVE.load(Ordering::Relaxed)
}

/// Plus haut niveau atteint depuis le démarrage.
pub fn peak() -> u64 {
    PEAK.load(Ordering::Relaxed)
}
