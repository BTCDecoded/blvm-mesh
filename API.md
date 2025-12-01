# bllvm-mesh API Documentation

## Overview

The `bllvm-mesh` module provides payment-gated routing and network state management for bllvm-node.

## Core Components

### `manager`

Main mesh manager coordinating routing, payment verification, and packet forwarding.

#### `MeshManager`

Central coordinator for mesh networking.

**Methods:**

- `new(ctx: &ModuleContext, node_api: Arc<dyn NodeAPI>) -> Result<Self, MeshError>`
  - Creates a new mesh manager
  - Initializes routing policy, payment verifier, replay prevention, and routing table

- `handle_packet(packet: MeshPacket) -> Result<(), MeshError>`
  - Handles incoming mesh packets:
    - Protocol detection
    - Routing policy determination
    - Payment verification (if required)
    - Replay prevention
    - Packet forwarding

- `send_mesh_packet(peer_address: String, packet_data: Vec<u8>) -> Result<(), MeshError>`
  - Sends a mesh packet to a peer via NodeAPI

### `verifier`

Payment verification for mesh routing.

#### `PaymentVerifier`

Verifies Lightning and CTV payment proofs.

**Methods:**

- `new(node_api: Arc<dyn NodeAPI>) -> Self`
  - Creates a new payment verifier

- `verify(proof: &PaymentProof) -> Result<VerificationResult, MeshError>`
  - Verifies a payment proof:
    - Checks expiry
    - Verifies Lightning or CTV payment
    - Returns verification result with amount and validity

**Payment Proof Types:**
- `Lightning` - BOLT11 invoice + preimage + amount + timestamps
- `InstantSettlement` (CTV) - Covenant proof + output index

### `routing`

Routing table management.

#### `RoutingTable`

Manages direct peers and multi-hop routes.

**Methods:**

- `add_direct_peer(node_id: NodeId, address: String)`
  - Adds a direct peer to the routing table

- `find_route(destination: &NodeId) -> Option<Vec<NodeId>>`
  - Finds a route to a destination node

- `calculate_fee(route: &[NodeId], amount_sats: u64) -> FeeDistribution`
  - Calculates fee distribution (60/30/10 split)

### `routing_policy`

Protocol detection and routing policy determination.

#### `RoutingPolicyEngine`

Determines routing policy based on protocol and mode.

**Methods:**

- `detect_protocol(data: &[u8]) -> DetectedProtocol`
  - Detects protocol from message bytes:
    - BitcoinP2P (magic bytes)
    - CommonsGovernance
    - StratumV2
    - MeshPacket
    - Unknown

- `determine_policy(protocol: DetectedProtocol) -> RoutingPolicy`
  - Determines if payment is required:
    - `Free` - Bitcoin P2P, governance, Stratum V2
    - `PaymentRequired` - Unknown protocols, mesh packets (in payment-gated mode)

**Mesh Modes:**
- `BitcoinOnly` - Only Bitcoin P2P allowed
- `PaymentGated` - Free protocols + paid mesh
- `Open` - All traffic free

### `replay`

Replay prevention for payment proofs.

#### `ReplayPrevention`

Prevents reuse of payment proofs.

**Methods:**

- `check_replay(proof: &PaymentProof, source: &NodeId, sequence: u64) -> Result<(), MeshError>`
  - Checks if a payment proof is a replay
  - Uses hash-based tracking and sequence numbers

- `cleanup_expired() -> usize`
  - Removes expired payment proof hashes

## Events

### Subscribed Events
- `PeerConnected` - New peer connection
- `PeerDisconnected` - Peer disconnected
- `MessageReceived` - Message from peer
- `PaymentVerified` - Payment verification result

### Published Events
- `RouteDiscovered` - Route found to destination
- `RouteFailed` - Route discovery failed
- `PaymentVerified` - Payment verified for mesh routing

## Configuration

```toml
[mesh]
enabled = true
mode = "payment_gated"  # "bitcoin_only", "payment_gated", "open"
listen_addr = "0.0.0.0:8334"
```

## Error Handling

All methods return `Result<T, MeshError>` where `MeshError` can be:
- `NetworkError(String)` - Network operation failed
- `PaymentVerificationFailed(String)` - Payment verification failed
- `ReplayDetected(String)` - Payment proof replay detected
- `RoutingError(String)` - Routing operation failed

## Examples

### Sending a Mesh Packet

```rust
let manager = MeshManager::new(&ctx, node_api).await?;
manager.send_mesh_packet("127.0.0.1:8334".to_string(), packet_data).await?;
```

### Verifying a Payment

```rust
let verifier = PaymentVerifier::new(node_api);
let proof = PaymentProof::Lightning { /* ... */ };
let result = verifier.verify(&proof).await?;
if result.verified {
    // Payment verified, proceed with routing
}
```

