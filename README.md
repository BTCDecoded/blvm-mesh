# blvm-mesh

Commons Mesh networking module for blvm-node.

## Overview

This module provides Commons Mesh networking capabilities for blvm-node, including:
- Payment-gated routing
- Traffic classification (free vs paid)
- Fee distribution
- Anti-monopoly protection
- Network state tracking

## Installation

```bash
# Install via cargo
cargo install blvm-mesh

# Or install via cargo-blvm-module
cargo install cargo-blvm-module
cargo blvm-module install blvm-mesh
```

## Configuration

Create a `config.toml` in the module directory:

```toml
[mesh]
enabled = true
mode = "payment_gated"  # "bitcoin_only", "payment_gated", "open"
listen_addr = "0.0.0.0:8334"
```

## Module Manifest

The module includes a `module.toml` manifest:

```toml
name = "blvm-mesh"
version = "0.1"
description = "Commons Mesh networking module"
author = "Bitcoin Commons Team"
entry_point = "blvm-mesh"

capabilities = [
    "read_blockchain",
    "subscribe_events",
]
```

## Events

### Subscribed Events (46+)
- Network events (PeerConnected, MessageReceived, etc.)
- Payment events (PaymentRequestCreated, PaymentVerified, etc.)
- Chain events (NewBlock, ChainTipUpdated, etc.)
- Mempool events (MempoolTransactionAdded, FeeRateChanged, etc.)

### Published Events
- RouteDiscovered
- RouteFailed
- PaymentVerified (for mesh routing payments)

## Mesh submodules (`modules/`)

`blvm-messaging`, `blvm-onion`, `blvm-bridge`, and `blvm-mining-pool` use the same **`run_module!` + `#[module]`** pattern as the core `blvm-mesh` binary: `ModuleBootstrap::init_module`, `ModuleDb`, async `setup` (mesh `MeshClient`, protocol registration), and **`#[on_event(...)]`** for `MeshPacketReceived` / network events. Set **`MESH_MODULE_ID`** if the mesh module is not named `blvm-mesh`. **`BRIDGE_MODE`** (`satellite` \| `radio` \| `internet` \| custom) selects bridge kind for `blvm-bridge`.

## External stacks (Meshtastic, Reticulum)

See **[docs/edge-adapters.md](docs/edge-adapters.md)** for how to add **edge adapters** (and why there are no Meshtastic/Reticulum subcrates in-tree yet).

BLVM mesh is **Bitcoin transport + payment policy** on top of the node’s P2P; it does not speak LoRa or Reticulum natively. To reach those networks, run a **small adapter** at the edge:

- **Meshtastic:** Typical integration is **MQTT** (protobuf `MeshPacket` / `ServiceEnvelope` on broker topics) or **serial** from a gateway radio; see [Meshtastic MQTT](https://meshtastic.org/docs/software/integrations/mqtt/) and protobuf definitions in the Meshtastic project. Your adapter converts between those frames and BLVM mesh packets (or the bridge module’s relay path).
- **Reticulum:** Use the **Python `RNS` API** (`Reticulum`, `Destination`, `Link`, `Resource`) with `rnsd` and interface config; see [Reticulum docs](https://reticulum.network/manual/). Bridge **application data** between an RNS destination and BLVM mesh **by design**—Reticulum is not a generic TCP/IP tunnel.

## Parity, registry, releases

Each publishable binary has a **`module.toml`** (root + `modules/*`) with **`[downloads]`** keys aligned to the **zmq** release matrix (`x86_64-linux`, `aarch64-linux`, `x86_64-windows`). CI/release should fill `url` / `sha256` per artifact. Bootstrap policy: [Module registry](../../blvm/README.md#module-registry) in the **`blvm`** crate README.

## License

MIT License - see LICENSE file for details.

