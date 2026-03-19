// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! # `ic-protocol` — shared wire types
//!
//! This crate defines the serializable data that crosses the simulation and
//! networking boundary. It exists so `ic-sim` and `ic-net` can share one stable
//! vocabulary without importing each other directly.
//!
//! In practical terms, this is the "language on the wire": player IDs, tick
//! numbers, player orders, directional wrappers, and lane markers. Because
//! these types are serialized and exchanged between subsystems, changes here are
//! architectural changes, not local refactors.
//!
//! Design decisions:
//! - D006: pluggable networking
//! - D008: sub-tick timestamps
//! - D012: deterministic order validation

use serde::{Deserialize, Serialize};

/// Unique identifier for one player in a match.
///
/// Iron Curtain's type-safety rules ban bare integers for domain IDs. Wrapping
/// the raw `u32` prevents accidental mix-ups such as passing a player ID where a
/// unit ID or team ID will eventually be expected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PlayerId(pub u32);

/// Monotonic simulation tick number.
///
/// The simulation advances in discrete ticks. Any protocol message that refers
/// to authoritative game time should do so through this wrapper instead of a
/// plain integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TickNumber(pub u64);

/// Fractional position inside a single simulation tick.
///
/// D008 introduces sub-tick ordering so two commands issued during the same
/// simulation tick can still be resolved fairly and deterministically. The raw
/// `u16` gives a compact, serializable ordering key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SubTickTimestamp(pub u16);

/// Command issued by a player and later validated by the simulation.
///
/// This enum is intentionally small today because the simulation crate does not
/// exist yet. As real gameplay systems arrive, new variants will be added here
/// and become part of the shared protocol contract between client, replay, and
/// network layers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PlayerOrder {
    /// No gameplay action.
    ///
    /// This placeholder variant is useful in early infrastructure work because
    /// it lets the engine exercise ordering, serialization, and replay plumbing
    /// without needing full gameplay commands first.
    Noop,
}

/// Player command annotated with enough metadata to order it fairly.
///
/// This wrapper keeps the actual gameplay command (`order`) separate from the
/// routing and fairness metadata (`player`, `sub_tick`) that the network and
/// simulation layers need to process it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimestampedOrder {
    /// Player who issued the command.
    pub player: PlayerId,
    /// Relative position inside the tick used for deterministic same-tick order resolution.
    pub sub_tick: SubTickTimestamp,
    /// Gameplay command payload.
    pub order: PlayerOrder,
}

/// Batch of all commands that belong to one simulation tick.
///
/// Networking, replay, and lockstep code will move these batches around instead
/// of shipping individual commands in isolation. The `orders` vector is assumed
/// to be sorted by ascending `sub_tick` before the simulation consumes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TickOrders {
    /// Tick the batch belongs to.
    pub tick: TickNumber,
    /// Commands for that tick, ordered by fairness timestamp.
    pub orders: Vec<TimestampedOrder>,
}

/// Logical communication channel for a protocol message.
///
/// The future networking stack will use lanes to group messages by delivery
/// requirements. Keeping the lane as a typed enum makes those categories
/// explicit instead of scattering ad-hoc string or integer tags through the
/// network code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MessageLane {
    /// Gameplay orders that must arrive reliably and in order.
    Orders,
    /// Human chat traffic that should be reliable but does not need gameplay sequencing.
    Chat,
    /// Voice traffic where low latency matters more than guaranteed delivery.
    Voice,
    /// Timing and synchronization metadata shared between peers and relay.
    Timing,
}

/// Message branded as originating from a client.
///
/// The wrapper is a type-level guardrail. Code that expects a server-originated
/// message cannot accidentally accept a client-originated payload with the same
/// inner shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FromClient<T> {
    /// Player who sent the payload.
    pub sender: PlayerId,
    /// Actual message body.
    pub payload: T,
}

/// Message branded as originating from the server or relay.
///
/// Like [`FromClient<T>`], this wrapper exists to encode trust direction in the
/// type system instead of relying on comments or calling conventions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FromServer<T> {
    /// Actual message body.
    pub payload: T,
}

#[cfg(test)]
mod tests;
