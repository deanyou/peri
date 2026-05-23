//! jemalloc allocator tuning for high-churn workloads.
//!
//! Default jemalloc settings prioritize throughput over memory footprint.
//! The agent event pipeline produces ~680k transient allocations per turn
//! (serde JSON serialize/deserialize, string cloning). This causes arena
//! slab fragmentation where dirty pages accumulate faster than the default
//! decay purge can reclaim them.
//!
//! Configuration applied:
//! - `dirty_decay_ms: 200` — purge freed arena pages after 200ms (default: 1000ms+)
//! - `background_thread: true` — enable background purge thread (default: disabled)
//! - `lg_tcache_max: 16` — limit thread cache to objects ≤64KB (default: unlimited)

/// Configure jemalloc for aggressive memory reclamation.
///
/// Must be called **before** creating the tokio runtime, ideally at the
/// very start of `main()`. Writes are best-effort — missing keys or
/// unsupported platforms are silently ignored.
// Called from main.rs (bin target) via peri_tui::jemalloc_config::configure_jemalloc().
// Clippy's dead_code lint fires on lib targets even when used by the bin target.
#[allow(dead_code)]
#[cfg(not(target_os = "windows"))]
pub fn configure_jemalloc() {
    use tracing::{debug, warn};

    // Advance epoch to ensure stats are fresh
    let _ = tikv_jemalloc_ctl::epoch::advance();

    // 1. dirty_decay_ms — time before freed dirty pages are purged
    //    Default is 10000ms on many builds; we set 200ms for aggressive reclamation.
    //    Lower values increase CPU overhead from madvise syscalls but prevent
    //    the observed ~27MB dirty extent accumulation per turn.
    match unsafe { tikv_jemalloc_ctl::raw::write(b"arenas.dirty_decay_ms\0", 200i64) } {
        Ok(()) => debug!("jemalloc: arenas.dirty_decay_ms = 200"),
        Err(e) => warn!("jemalloc: failed to set dirty_decay_ms: {}", e),
    }

    // 2. background_thread — enables a background thread per arena that
    //    proactively purges dirty pages. Without this, purge only happens
    //    during foreground allocations (the "lazy" purge path), which can't
    //    keep up with our churn rate.
    match unsafe { tikv_jemalloc_ctl::raw::write(b"background_thread\0", true) } {
        Ok(()) => debug!("jemalloc: background_thread = true"),
        Err(e) => warn!("jemalloc: failed to enable background_thread: {}", e),
    }

    // 3. lg_tcache_max — log2 of max cached allocation size in thread caches.
    //    Default is ~23 (8MB), which means large allocations linger in tcache.
    //    Setting to 16 (64KB) limits tcache to small objects, reducing the
    //    5-7MB tcache_bytes overhead observed in heapdumps.
    match unsafe { tikv_jemalloc_ctl::raw::write(b"arenas.lg_tcache_max\0", 16usize) } {
        Ok(()) => debug!("jemalloc: arenas.lg_tcache_max = 16 (64KB)"),
        Err(e) => warn!("jemalloc: failed to set lg_tcache_max: {}", e),
    }
}

#[cfg(target_os = "windows")]
pub fn configure_jemalloc() {
    // jemalloc not used on Windows (system allocator instead)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_configure_jemalloc_does_not_panic() {
        // configure_jemalloc should be safe to call, even multiple times
        configure_jemalloc();
        configure_jemalloc();
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn test_dirty_decay_ms_is_set() {
        configure_jemalloc();
        let _ = tikv_jemalloc_ctl::epoch::advance();
        let val: i64 = unsafe { tikv_jemalloc_ctl::raw::read(b"arenas.dirty_decay_ms\0") }
            .expect("should read dirty_decay_ms");
        assert_eq!(val, 200, "dirty_decay_ms should be 200ms after configure");
    }
}
