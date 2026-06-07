//! Three-node mesh harness for multihop smoke tests.

use blvm_mesh::manager::MeshManager;
use blvm_mesh::packet::{MeshPacket, PacketType};
use blvm_mesh::routing_policy::MeshMode;
use blvm_mesh::test_support::TestNodeAPI;
use std::sync::Arc;

#[tokio::test]
async fn three_node_a_to_b_to_c_delivery() {
    let api_a = Arc::new(TestNodeAPI::default());
    let api_b = Arc::new(TestNodeAPI::default());
    let api_c = Arc::new(TestNodeAPI::default());

    let a = MeshManager::new_for_test_with_seed(true, MeshMode::Open, 1, Arc::clone(&api_a) as Arc<_>).await;
    let b = MeshManager::new_for_test_with_seed(true, MeshMode::Open, 2, Arc::clone(&api_b) as Arc<_>).await;
    let c = MeshManager::new_for_test_with_seed(true, MeshMode::Open, 3, Arc::clone(&api_c) as Arc<_>).await;

    let a_id = a.node_id();
    let b_id = b.node_id();
    let c_id = c.node_id();

    a.add_peer_with_id("b:1", Some(b_id)).unwrap();
    b.add_peer_with_id("a:1", Some(a_id)).unwrap();
    b.add_peer_with_id("c:1", Some(c_id)).unwrap();
    c.add_peer_with_id("b:1", Some(b_id)).unwrap();

    a.install_route_for_test(c_id, vec![a_id, b_id, c_id]);

    let packet = MeshPacket::new(PacketType::Paid, a_id, c_id, b"multihop-smoke".to_vec());
    a.route_packet(&packet).await.unwrap();

    let hop1 = api_a.take_outbox();
    assert_eq!(hop1.len(), 1);
    assert_eq!(hop1[0].0, "b:1");
    b.handle_mesh_packet_received(&hop1[0].1, "a:1")
        .await
        .unwrap();

    let hop2 = api_b.take_outbox();
    assert_eq!(hop2.len(), 1);
    assert_eq!(hop2[0].0, "c:1");
    c.handle_mesh_packet_received(&hop2[0].1, "b:1")
        .await
        .unwrap();
}
