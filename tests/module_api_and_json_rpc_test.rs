//! ModuleAPI and JSON-RPC roundtrips for local mesh delivery.

use std::sync::Arc;

use blvm_mesh::api::{MeshModuleAPI, SendPacketRequest, SendPacketResponse};
use blvm_mesh::json_rpc::{meshpollreceived, meshsendpacket};
use blvm_mesh::manager::MeshManager;
use blvm_mesh::packet::{MeshPacket, PacketMetadata, PacketType};
use blvm_mesh::routing::NodeId;
use blvm_mesh::routing_policy::MeshMode;
use blvm_mesh::test_support::TestNodeAPI;
use blvm_node::module::inter_module::api::ModuleAPI;
use serde_json::json;

const APP_PROTOCOL: &str = "app-ukm-v1";

fn id(n: u8) -> NodeId {
    let mut out = [0u8; 32];
    out[31] = n;
    out
}

async fn manager_with_queued_delivery() -> (Arc<MeshManager>, NodeId, Vec<u8>) {
    let local = id(42);
    let source = id(1);
    let payload = b"hello-mesh-app".to_vec();
    let manager = Arc::new(
        MeshManager::new_for_test(
            true,
            MeshMode::Open,
            local,
            Arc::new(TestNodeAPI::default()),
        )
        .await,
    );
    let mut packet = MeshPacket::new(PacketType::Paid, source, local, payload.clone());
    packet.metadata = Some(PacketMetadata {
        protocol: Some(APP_PROTOCOL.into()),
        fields: Default::default(),
    });
    manager.handle_incoming_packet(&packet, None).await.unwrap();
    (manager, source, payload)
}

#[tokio::test]
async fn module_api_poll_local_deliveries_roundtrip() {
    let (manager, source, payload) = manager_with_queued_delivery().await;
    let api = MeshModuleAPI::new(manager);

    #[derive(serde::Serialize)]
    struct PollRequest {
        protocol_id: Option<String>,
        max_packets: Option<usize>,
    }
    let poll_bytes = bincode::serialize(&PollRequest {
        protocol_id: Some(APP_PROTOCOL.into()),
        max_packets: Some(8),
    })
    .unwrap();

    let poll_resp = api
        .handle_request("poll_local_deliveries", &poll_bytes, "test")
        .await
        .unwrap();

    let deliveries: Vec<blvm_mesh::manager::LocalDelivery> =
        bincode::deserialize(&poll_resp).unwrap();
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].payload, payload);
    assert_eq!(deliveries[0].protocol_id, APP_PROTOCOL);
    assert_eq!(deliveries[0].source, source);
}

#[tokio::test(flavor = "multi_thread")]
async fn json_rpc_meshpollreceived_roundtrip() {
    let (manager, source, payload) = manager_with_queued_delivery().await;

    let result = meshpollreceived(
        &manager,
        &json!({
            "protocol_id": APP_PROTOCOL,
            "max_packets": 8
        }),
    )
    .unwrap();

    let packets = result["packets"].as_array().unwrap();
    assert_eq!(packets.len(), 1);
    assert_eq!(packets[0]["payload_hex"], hex::encode(&payload));
    assert_eq!(packets[0]["protocol_id"], APP_PROTOCOL);
    assert_eq!(packets[0]["source_hex"], hex::encode(source));
}

#[tokio::test(flavor = "multi_thread")]
async fn module_api_send_packet_and_json_rpc_meshsendpacket_roundtrip() {
    let local = id(42);
    let manager = Arc::new(
        MeshManager::new_for_test(
            true,
            MeshMode::Open,
            local,
            Arc::new(TestNodeAPI::default()),
        )
        .await,
    );
    let api = MeshModuleAPI::new(Arc::clone(&manager));

    let req = SendPacketRequest {
        destination: id(99),
        payload: b"hello-mesh-app".to_vec(),
        payment_proof: None,
        protocol_id: Some(APP_PROTOCOL.into()),
        ttl: Some(3600),
    };
    let send_bytes = bincode::serialize(&req).unwrap();

    let send_resp = api
        .handle_request("send_packet", &send_bytes, "test")
        .await
        .unwrap();
    let send: SendPacketResponse = bincode::deserialize(&send_resp).unwrap();
    assert!(send.success || send.error.is_some());

    let json_resp = meshsendpacket(
        &manager,
        &json!({ "request_hex": hex::encode(&send_bytes) }),
    )
    .unwrap();
    assert_eq!(json_resp["success"], send.success);
    if send.success {
        assert_eq!(json_resp["packet_id"], hex::encode(send.packet_id));
    }
}
