# blvm-mesh transport

Protocol-agnostic, payment-gated mesh overlay on blvm-node P2P.

## Wire format

- Magic: `MESH` (`0x4D 0x45 0x53 0x48`)
- Body: bincode-serialized `MeshPacket` (`src/packet.rs`)
- Max size: 256 KiB bincode payload (`MAX_BINCODE_PAYLOAD_SIZE`)

## Operating modes

| Mode | Behaviour |
|------|-----------|
| `open` | All mesh traffic free |
| `payment_gated` | `PacketType::Paid` requires valid `PaymentProof` |
| `bitcoin_only` | Mesh app traffic rejected |

Routing policy uses **`packet_type`** on mesh-originated packets (not payload sniffing).

## Node setup

```toml
# blvm-node config
[modules]
enabled_modules = ["blvm-mesh"]
modules_dir = "modules"   # or absolute path
```

`module.toml` must declare: `register_module_api`, `network_access`, `publish_events`, `read_payment`. Without these, spawned modules cannot register their API or reach P2P/payment state.

### Subprocess ModuleAPI

Spawned modules register method descriptors over IPC; blvm-node installs an `IpcForwardingModuleAPI` proxy in `ModuleApiRegistry`. RPC `meshsendpacket` / `meshpollreceived` call `call_module("send_packet")` and `call_module("poll_local_deliveries")` through this proxy.

Verify: `cargo test -p blvm-node --test mesh_ipc_proxy_test` and `cargo test -p blvm-node --test mesh_rpc_integration_test`.

```toml
# <data_dir>/modules/blvm-mesh/config.toml
[mesh]
enabled = true
mode = "open"
```

Install binary (dev):

```bash
cd blvm-mesh && cargo build
cp target/debug/blvm-mesh $NODE_DATA/modules/blvm-mesh/target/release/blvm-mesh
```

## Smoke test (two nodes)

1. Start node A and node B with `blvm-mesh` enabled, `mode = "open"`.
2. On B: `mesh status` — note **Node ID** (64-char hex).
3. On A: `mesh add-peer <B-addr> <B-node-id-hex>` (explicit id required until mesh hello).
4. On A: `mesh send_packet <B-node-id-hex> "hello" bitsov-ukm-v1` (optional third arg sets `metadata.protocol` for app delivery / poll filter).
5. On B: logs should show `Packet delivered to local node`.

Without explicit `node_id_hex` on `add-peer`, local delivery fails until mesh hello completes (Ed25519 pubkey exchange).

## Smoke test (three nodes)

1. Start nodes A, B, C with `blvm-mesh` enabled, `mode = "open"`.
2. Connect A↔B and B↔C at the P2P layer (Bitcoin peers or configured transports).
3. On each node, confirm mesh hello runs on `PeerConnected` (logs: `Mesh hello from …`).
4. On B: `mesh status` — note B and C **Node ID** (64-char hex, Ed25519 pubkey).
5. On A: verify B appears in routing with pubkey id (not address hash) after hello.
6. On A: `mesh send_packet <C-node-id-hex> "hello"` — B forwards; C logs `Packet delivered to local node`.

Automated equivalent: `cargo test -p blvm-mesh --test multihop_harness`.

For `payment_gated`, attach a valid `PaymentProof::Lightning` (or `OnChainSettlement` stub) before send.

## BitSov integration

| Direction | Path |
|-----------|------|
| Outbound | `HybridBlvmTransport` → `meshsendpacket` → mesh module → P2P |
| Inbound | `meshpollreceived` → UKM decode → payment gate |
| ACK/reject | Mesh control frames (`bitsov-ctrl-v1` wrapper) via hybrid `send_raw_frame` |

Wire compatibility: bincode `PaymentProof` variant order is `0 Lightning`, `1 OnChainSettlement`, `2 InstantSettlement`. BitSov uses protocol id `bitsov-ukm-v1`.

**Identity:** BitSov app `NodeId` and mesh `MeshIdentity` are separate Ed25519 keys until explicitly bound (configure receiver mesh id from `mesh status`, or peer registry `mesh_node_id` when wired).

## Payment paths (overview)

| Path | Mesh proof | Notes |
|------|------------|-------|
| Lightning | `PaymentProof::Lightning` | BOLT11 + preimage |
| BLVM + CTV | `InstantSettlement` (`ctv` feature) | Covenant proof |
| BLVM, no CTV | `OnChainSettlement` (planned) | Mempool + session prepay |

All paths settle on **Bitcoin L1** — not a sidechain.

## Application transport

Prefer `MeshAppTransport` / `MeshClient` over raw `call_module`. `register_protocol_handler` is deprecated — inbound app data arrives via `MeshPacketReceived`; deserialize to `MeshPacket` and filter on `metadata.protocol`.

Ingress hardening: monotonic outbound `packet.sequence`, per-source sequence checks on ingress, and `mesh.rate_limit_per_minute` (default 120, `0` = off).

## Tests

```bash
cargo test -p blvm-mesh
cargo test -p blvm-mesh --features ctv   # path 2
./scripts/test-mesh.sh
```
