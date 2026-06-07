//! Sequence, rate limit, and outbound numbering tests.

use blvm_mesh::manager::MeshManager;
use blvm_mesh::packet::{MeshPacket, PacketType};
use blvm_mesh::rate_limit::RateLimiter;
use blvm_mesh::routing_policy::MeshMode;
use blvm_mesh::test_support::TestNodeAPI;
use std::sync::Arc;

fn id(byte: u8) -> [u8; 32] {
    [byte; 32]
}

#[tokio::test]
async fn outbound_packets_get_monotonic_sequence() {
    let mgr = MeshManager::new_for_test(
        true,
        MeshMode::Open,
        id(1),
        Arc::new(TestNodeAPI::default()),
    )
    .await;
    mgr.add_peer_with_id("peer:1", Some(id(2))).unwrap();

    mgr.route_packet(&MeshPacket::new(
        PacketType::Paid,
        id(1),
        id(2),
        b"a".to_vec(),
    ))
    .await
    .unwrap();
    mgr.route_packet(&MeshPacket::new(
        PacketType::Paid,
        id(1),
        id(2),
        b"b".to_vec(),
    ))
    .await
    .unwrap();

    assert_eq!(mgr.next_sequence(), 3);
}

#[test]
fn rate_limiter_blocks_over_threshold() {
    let limiter = RateLimiter::new(2, 60);
    limiter.check_and_record("peer").unwrap();
    limiter.check_and_record("peer").unwrap();
    assert!(limiter.check_and_record("peer").is_err());
}

#[tokio::test]
async fn manager_ingress_rate_limit() {
    let mgr = MeshManager::new_for_test_with_rate_limit(
        true,
        MeshMode::Open,
        5,
        Arc::new(TestNodeAPI::default()),
        2,
    )
    .await;

    let mut p1 = MeshPacket::new(PacketType::Paid, id(9), mgr.node_id(), b"x".to_vec());
    p1.sequence = 1;
    mgr.handle_incoming_packet(&p1, Some("attacker:1"))
        .await
        .unwrap();

    let mut p2 = MeshPacket::new(PacketType::Paid, id(9), mgr.node_id(), b"y".to_vec());
    p2.sequence = 2;
    mgr.handle_incoming_packet(&p2, Some("attacker:1"))
        .await
        .unwrap();

    let mut p3 = MeshPacket::new(PacketType::Paid, id(9), mgr.node_id(), b"z".to_vec());
    p3.sequence = 3;
    let err = mgr
        .handle_incoming_packet(&p3, Some("attacker:1"))
        .await
        .unwrap_err();
    assert!(matches!(err, blvm_mesh::error::MeshError::RateLimited(_)));
}

#[tokio::test]
async fn ingress_rejects_duplicate_sequence_from_source() {
    let mgr = MeshManager::new_for_test(
        true,
        MeshMode::Open,
        id(1),
        Arc::new(TestNodeAPI::default()),
    )
    .await;
    let mut p = MeshPacket::new(PacketType::Paid, id(9), id(1), b"1".to_vec());
    p.sequence = 5;
    mgr.handle_incoming_packet(&p, Some("remote:1"))
        .await
        .unwrap();
    let err = mgr
        .handle_incoming_packet(&p, Some("remote:1"))
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        blvm_mesh::error::MeshError::ReplayDetected(_)
    ));
}
