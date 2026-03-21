// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Global allocator selection for the `ic-game` executable.
//!
//! The design docs settle this at the engine-entrypoint level rather than in
//! content, render, or protocol libraries. The reason is scope: the global
//! allocator is a process-wide runtime choice, so the top-level executable is
//! the right place to own it.
//!
//! Current canonical policy from the performance docs:
//!
//! - desktop and mobile native targets use `mimalloc`
//! - `wasm32` keeps Rust's default allocator (`dlmalloc`)
//! - the counting allocator wrapper is a later CI/debug feature, not the
//!   baseline runtime path we install here
//!
//! Keeping the target-selection logic in one tiny module gives tests a narrow,
//! deterministic proof surface without forcing the rest of the game-client
//! bootstrap to know allocator details.

/// Human-readable allocator label for the current compilation target.
///
/// This gives tests and diagnostics a simple stable contract to assert against
/// without needing platform-specific heap introspection.
pub(crate) const fn allocator_backend_name() -> &'static str {
    if cfg!(target_arch = "wasm32") {
        "dlmalloc (Rust default)"
    } else {
        "mimalloc"
    }
}

/// Returns whether this build installs a custom global allocator override.
///
/// This helper exists only for tests. Production code does not need this
/// boolean because the `#[cfg(not(target_arch = "wasm32"))]` gate on the
/// actual global allocator static is the authoritative runtime switch.
///
/// Native targets do install the override because the performance design
/// explicitly picks `mimalloc`. `wasm32` does not, because Rust already routes
/// that target through its standard `dlmalloc` setup.
#[cfg(test)]
pub(crate) const fn installs_custom_global_allocator() -> bool {
    !cfg!(target_arch = "wasm32")
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) type IcGameGlobalAllocator = mimalloc::MiMalloc;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) const IC_GAME_GLOBAL_ALLOCATOR: IcGameGlobalAllocator = mimalloc::MiMalloc;

#[cfg(test)]
mod tests {
    use super::*;

    /// Proves that native `ic-game` builds follow the performance-doc decision
    /// to install `mimalloc` as the executable's global allocator.
    ///
    /// The test asserts both the human-readable target label and, on native
    /// builds, the concrete allocator type alias that `main.rs` uses for the
    /// `#[global_allocator]` static.
    #[test]
    fn native_targets_select_mimalloc_as_the_global_allocator() {
        #[cfg(not(target_arch = "wasm32"))]
        {
            assert!(installs_custom_global_allocator());
            assert_eq!(allocator_backend_name(), "mimalloc");
            assert!(
                std::any::type_name::<IcGameGlobalAllocator>().contains("mimalloc::MiMalloc"),
                "native builds should wire the executable allocator to mimalloc",
            );
        }
    }

    /// Proves that `wasm32` builds keep Rust's default allocator instead of
    /// trying to force the native `mimalloc` path onto a target where the
    /// design docs explicitly say to use the default.
    #[test]
    fn wasm_targets_keep_rusts_default_allocator() {
        #[cfg(target_arch = "wasm32")]
        {
            assert!(!installs_custom_global_allocator());
            assert_eq!(allocator_backend_name(), "dlmalloc (Rust default)");
        }
    }
}
