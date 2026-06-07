//! Relay fee split tests.

use blvm_mesh::routing::{NodeId, RoutingTable};

fn id(byte: u8) -> NodeId {
    [byte; 32]
}

#[test]
fn three_hop_route_splits_intermediate_fee() {
    let table = RoutingTable::new(3600);
    let route = vec![id(1), id(2), id(3), id(4)];
    let fee = table.calculate_routing_fee(&route, 1000);

    assert_eq!(fee.total, 1000);
    assert_eq!(fee.destination, 600);
    assert_eq!(fee.source, 100);
    assert_eq!(fee.intermediate, 150);
    assert_eq!(fee.hop_count, 4);
}

#[test]
fn direct_route_has_no_intermediate_fee() {
    let table = RoutingTable::new(3600);
    let route = vec![id(1), id(2)];
    let fee = table.calculate_routing_fee(&route, 500);

    assert_eq!(fee.intermediate, 0);
    assert_eq!(fee.destination, 300);
    assert_eq!(fee.source, 50);
}
