// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! # ic-protocol — Shared Wire Types
//!
//! This crate defines the serializable types that cross the simulation/network
//! boundary. Both `ic-sim` and `ic-net` depend on `ic-protocol` — they never
//! depend on each other directly.
//!
//! ## Architecture Context
//!
//! `ic-protocol` is the shared boundary crate in the Iron Curtain architecture.
//! It contains `PlayerOrder`, `TimestampedOrder`, `TickOrders`, `MessageLane`,
//! and directional wrappers (`FromClient<T>`, `FromServer<T>`).
//!
//! Design decisions: D006 (pluggable networking), D008 (sub-tick timestamps),
//! D012 (order validation).
//!
//! See: <https://iron-curtain-engine.github.io/iron-curtain-design-docs/02-ARCHITECTURE.html>

use serde::{Deserialize, Serialize};

// ── ID Newtypes ──────────────────────────────

/// Unique identifier for a player in a match.
///
/// Never use bare `u32` for player identification — all domain IDs
/// are wrapped newtypes (Invariant 4 in AGENTS.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PlayerId(pub u32);

/// Simulation tick number (monotonically increasing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TickNumber(pub u64);

// ── Sub-Tick Timestamp ───────────────────────

/// Fractional offset within a tick (0..=65535), used for sub-tick ordering (D008).
///
/// CS2-inspired: orders issued at different moments within the same tick
/// are resolved in timestamp order for fairness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SubTickTimestamp(pub u16);

// ── Player Orders ────────────────────────────

/// A command issued by a player, to be validated and applied by the simulation.
///
/// The simulation validates all orders deterministically (D012) — invalid orders
/// are rejected identically by all clients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PlayerOrder {
    /// No operation — used as a keepalive or placeholder.
    Noop,
    // Additional order variants will be added in Phase 2 (M2: G6–G10).
}

/// A player order annotated with sub-tick timing for fair ordering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimestampedOrder {
    /// The player who issued this order.
    pub player: PlayerId,
    /// Sub-tick offset for ordering within the same tick.
    pub sub_tick: SubTickTimestamp,
    /// The actual order payload.
    pub order: PlayerOrder,
}

/// All orders for a single simulation tick, sorted by sub-tick timestamp.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TickOrders {
    /// The tick these orders belong to.
    pub tick: TickNumber,
    /// Orders sorted by `sub_tick` timestamp (ascending).
    pub orders: Vec<TimestampedOrder>,
}

// ── Message Lanes ────────────────────────────

/// Logical communication channels between client and relay/server.
///
/// Different message types flow on different lanes with different
/// reliability and priority characteristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MessageLane {
    /// Game orders — reliable, ordered delivery.
    Orders,
    /// Chat messages — reliable, unordered.
    Chat,
    /// Voice data — unreliable, low-latency.
    Voice,
    /// Timing/sync metadata — reliable, ordered.
    Timing,
}

// ── Directional Wrappers ─────────────────────

/// Wraps a message to indicate it was sent from a client.
///
/// Prevents accidentally processing a client message as a server message
/// (Invariant 4: type-level direction branding).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FromClient<T> {
    /// The player who sent this message.
    pub sender: PlayerId,
    /// The message payload.
    pub payload: T,
}

/// Wraps a message to indicate it was sent from the server/relay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FromServer<T> {
    /// The message payload.
    pub payload: T,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_id_is_ordered() {
        assert!(PlayerId(1) < PlayerId(2));
    }

    #[test]
    fn tick_orders_round_trip() {
        let orders = TickOrders {
            tick: TickNumber(42),
            orders: vec![TimestampedOrder {
                player: PlayerId(1),
                sub_tick: SubTickTimestamp(100),
                order: PlayerOrder::Noop,
            }],
        };

        let serialized = serde_yaml::to_string(&orders).unwrap();
        let deserialized: TickOrders = serde_yaml::from_str(&serialized).unwrap();
        assert_eq!(orders, deserialized);
    }
}
