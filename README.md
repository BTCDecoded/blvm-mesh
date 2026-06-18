# blvm-mesh

Commons Mesh networking module for blvm-node.

## Overview

Payment-gated mesh overlay for blvm-node:

- Payment-gated routing
- Traffic classification (free vs paid)
- Fee distribution
- Anti-monopoly protection
- Network state tracking

## Installation

Pin in node `blvm.toml`:

```toml
[modules]
blvm-mesh = "0.1.*"
```

Or build from source and place the binary + `module.toml` on the module search path. See [blvm-docs — Mesh module](https://github.com/BTCDecoded/blvm-docs/blob/main/src/modules/mesh.md).

## Configuration

Create `config.toml` in `<modules.data_dir>/blvm-mesh/` with **flat top-level keys** (no `[mesh]` wrapper for `MeshConfig` — a `[mesh]` table is **silently ignored** and `enabled` stays **`false`**):

```toml
enabled = true
mode = "payment_gated"  # open | payment_gated | bitcoin_only
max_peers = 50
rate_limit_per_minute = 120   # 0 = off
# peers = [{ address = "127.0.0.1:8333", node_id_hex = "..." }]
```

> **Note:** Some tooling writes `identity_seed_hex` under a `[mesh]` table for mesh identity; **`enabled`**, **`mode`**, and other **`MeshConfig`** fields belong at the **root** of `config.toml`.

Node overrides: `[modules.blvm-mesh]` with the same flat keys.

## Module Manifest

See `module.toml` in this repo and **`registry/modules.json`** — do not hardcode semver in operator docs.

```toml
name = "blvm-mesh"
description = "Commons Mesh networking module"
author = "Bitcoin Commons Team"
entry_point = "blvm-mesh"

capabilities = [
    "read_blockchain",
    "subscribe_events",
]
```

## Events

**Subscribed:** mesh/network, payment, chain, and mempool events (see module source). **Published:** `RouteDiscovered`, `RouteFailed`.

## Mesh submodules (`modules/`)

`blvm-messaging`, `blvm-onion`, `blvm-bridge`, and `blvm-mining-pool` use the same **`run_module!` + `#[module]`** pattern. Set **`MESH_MODULE_ID`** if the mesh module is not named `blvm-mesh`. **`BRIDGE_MODE`** (`satellite` | `radio` | `internet` | custom) selects bridge kind for `blvm-bridge`.

## External stacks (Meshtastic, Reticulum)

See **[docs/edge-adapters.md](docs/edge-adapters.md)** for edge adapters. BLVM mesh is Bitcoin transport + payment policy on the node P2P stack; it does not speak LoRa or Reticulum natively.

## Parity, registry, releases

Each publishable binary has a **`module.toml`** with **`[downloads]`** keys aligned to the release matrix. CI/release fills `url` / `sha256` per artifact. Bootstrap policy: [Module registry](https://github.com/BTCDecoded/blvm/blob/main/README.md#module-registry) in the **`blvm`** crate README.

## License

MIT License — see LICENSE file for details.
