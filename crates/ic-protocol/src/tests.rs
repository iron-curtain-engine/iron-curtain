// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2025–present Iron Curtain contributors

//! Tests for the shared protocol contract.

use super::*;

/// Verifies that wrapped player IDs preserve total ordering semantics.
///
/// This matters because future deterministic code will sort and compare IDs, so
/// the newtype must not get in the way of ordinary ordering behavior.
#[test]
fn player_id_is_ordered() {
    assert!(PlayerId(1) < PlayerId(2));
}

/// Proves that a tick batch can be serialized and deserialized without losing
/// any fairness or payload information.
///
/// `serde_yaml` is used here as a human-readable proof format. The exact wire
/// encoding may change later, but any serializer used with these derives should
/// preserve the same data contract.
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

    let serialized = serde_yaml::to_string(&orders).expect("tick batch should serialize");
    let deserialized: TickOrders =
        serde_yaml::from_str(&serialized).expect("serialized tick batch should deserialize");

    assert_eq!(orders, deserialized);
}

/// Proves that client-branded messages keep both their sender identity and
/// payload when serialized.
///
/// This protects the type-level trust boundary encoded by [`FromClient<T>`].
#[test]
fn from_client_round_trip_preserves_sender_and_payload() {
    let message = FromClient {
        sender: PlayerId(7),
        payload: PlayerOrder::Noop,
    };

    let serialized =
        serde_yaml::to_string(&message).expect("client-branded message should serialize");
    let deserialized: FromClient<PlayerOrder> = serde_yaml::from_str(&serialized)
        .expect("serialized client-branded message should deserialize");

    assert_eq!(message, deserialized);
}

/// Proves that message lane markers survive serialization unchanged.
///
/// Network code will eventually use these lanes to pick delivery behavior, so a
/// serialization mismatch here would corrupt protocol intent.
#[test]
fn message_lane_round_trip_preserves_delivery_category() {
    let lane = MessageLane::Timing;

    let serialized = serde_yaml::to_string(&lane).expect("lane marker should serialize");
    let deserialized: MessageLane =
        serde_yaml::from_str(&serialized).expect("serialized lane marker should deserialize");

    assert_eq!(lane, deserialized);
}
