//! Mesh integration tests (routing, discovery smoke).

use blvm_mesh::api::MeshModuleAPI;
use blvm_mesh::config::{MeshConfig, MeshPeerEntry};
use blvm_mesh::discovery::{DiscoveryMessage, RouteDiscovery};
use blvm_mesh::identity::{MeshIdentity, PROTOCOL_HELLO};
use blvm_mesh::manager::MeshManager;
use blvm_mesh::network::serialize_mesh_packet;
use blvm_mesh::packet::{MeshPacket, PacketMetadata, PacketType};
use blvm_mesh::payment_proof::PaymentProof;
use blvm_mesh::routing::{RoutingEntry, RoutingTable};
use blvm_mesh::routing_policy::MeshMode;
use blvm_mesh::test_support::TestNodeAPI;
use blvm_node::module::inter_module::api::ModuleAPI;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

fn id(byte: u8) -> [u8; 32] {
    [byte; 32]
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[tokio::test]
async fn handle_mesh_packet_api_round_trip() {
    let local = id(9);
    let manager =
        MeshManager::new_for_test(true, MeshMode::Open, local, Arc::new(TestNodeAPI::default())).await;
    let api = MeshModuleAPI::new(Arc::new(manager));
    let packet = MeshPacket::new(PacketType::Paid, id(1), local, b"api".to_vec());
    let wire = serialize_mesh_packet(&packet).unwrap();
    let params = bincode::serialize(&(wire, "127.0.0.1:9000".to_string())).unwrap();
    let out = api
        .handle_request("handle_mesh_packet", &params, "test")
        .await
        .unwrap();
    assert_eq!(out, bincode::serialize(&true).unwrap());
}

#[tokio::test]
async fn mesh_identity_node_id_is_ed25519_pubkey() {
    let identity = MeshIdentity::from_seed(42);
    let node_id = identity.node_id();
    let payload: [u8; 32] = identity.hello_payload().try_into().unwrap();
    assert_eq!(node_id, payload);
}

#[tokio::test]
async fn load_configured_peers_from_structured_config() {
    let manager =
        MeshManager::new_for_test(true, MeshMode::Open, id(1), Arc::new(TestNodeAPI::default())).await;
    let config = MeshConfig {
        enabled: true,
        peers: vec![MeshPeerEntry {
            address: "10.0.0.2:8333".to_string(),
            node_id_hex: Some(hex::encode(id(2))),
        }],
        ..Default::default()
    };
    manager.load_configured_peers(&config).unwrap();
    let peers = manager.list_direct_peers();
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].0, id(2));
}

#[tokio::test]
async fn mesh_hello_updates_routing_table_with_ed25519_id() {
    let local =
        MeshManager::new_for_test(true, MeshMode::Open, id(1), Arc::new(TestNodeAPI::default())).await;
    let peer_identity = MeshIdentity::from_seed(55);
    let peer_id = peer_identity.node_id();

    let mut packet = MeshPacket::new(
        PacketType::Paid,
        peer_id,
        id(1),
        peer_identity.hello_payload(),
    );
    packet.metadata = Some(PacketMetadata {
        protocol: Some(PROTOCOL_HELLO.to_string()),
        fields: Default::default(),
    });

    local
        .handle_incoming_packet(&packet, Some("10.0.0.5:8333"))
        .await
        .unwrap();
    let peers = local.list_direct_peers();
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].0, peer_id);
}

#[tokio::test]
async fn route_discovery_returns_path_when_intermediate_has_direct_peer() {
    let table = Arc::new(RoutingTable::new(3600));
    let discovery = RouteDiscovery::new(Arc::clone(&table), 10, 5);
    let now = now_secs();

    table.add_route(RoutingEntry {
        node_id: id(3),
        direct_address: None,
        next_hop: Some(id(3)),
        route_path: vec![id(2), id(3)],
        route_cost: 10,
        last_updated: now,
        quality_score: 0.9,
    });

    let request = DiscoveryMessage::RouteRequest {
        destination: id(3),
        source: id(1),
        request_id: 1,
        max_hops: 10,
    };
    let response = discovery
        .handle_route_request(&request, id(2))
        .await
        .unwrap()
        .expect("expected route response");

    match response {
        DiscoveryMessage::RouteResponse { route, .. } => {
            assert_eq!(route, vec![id(1), id(2), id(3)]);
        }
        other => panic!("unexpected discovery message: {:?}", other),
    }
}

#[tokio::test]
async fn three_node_forward_chain() {
    let b = MeshManager::new_for_test(true, MeshMode::Open, id(2), Arc::new(TestNodeAPI::default())).await;
    b.add_peer_with_id("127.0.0.1:3", Some(id(3))).unwrap();

    let packet = MeshPacket::new(PacketType::Paid, id(1), id(3), b"hop".to_vec());
    b.handle_incoming_packet(&packet, None).await.unwrap();
}

#[tokio::test]
async fn on_chain_settlement_proof_deserializes() {
    let proof = PaymentProof::OnChainSettlement {
        payment_request_id: "req-1".to_string(),
        tx_hash: [1u8; 32],
        amount_sats: 100,
        timestamp: 1_700_000_000,
    };
    let bytes = bincode::serialize(&proof).unwrap();
    let decoded: PaymentProof = bincode::deserialize(&bytes).unwrap();
    assert_eq!(decoded.amount_sats(), 100);
}

#[tokio::test]
async fn payment_gated_rejects_forward_without_proof() {
    let manager = MeshManager::new_for_test(
        true,
        MeshMode::PaymentGated,
        id(2),
        Arc::new(TestNodeAPI::default()),
    )
    .await;
    manager.add_peer_with_id("127.0.0.1:3", Some(id(3))).unwrap();

    let packet = MeshPacket::new(PacketType::Paid, id(1), id(3), b"unpaid".to_vec());
    let err = manager.handle_incoming_packet(&packet, None).await.unwrap_err();
    assert!(matches!(
        err,
        blvm_mesh::error::MeshError::PaymentVerification(_)
    ));
}
