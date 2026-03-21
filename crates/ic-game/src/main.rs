// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

mod allocator;

/// Installs the canonical native allocator for the main game executable.
///
/// The design docs place allocator ownership at the app entry point, not in a
/// shared library crate, because the allocator is a process-wide runtime
/// choice. Native targets use `mimalloc`; `wasm32` intentionally keeps Rust's
/// default allocator and therefore does not define this override at all.
#[cfg(not(target_arch = "wasm32"))]
#[global_allocator]
static GLOBAL_ALLOCATOR: allocator::IcGameGlobalAllocator = allocator::IC_GAME_GLOBAL_ALLOCATOR;

fn main() {
    let allocator_name = allocator::allocator_backend_name();

    if let Err(error) = ic_game::run() {
        eprintln!("failed to start ic-game bootstrap ({allocator_name}): {error}");
        std::process::exit(1);
    }
}
