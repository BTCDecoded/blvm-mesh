# bllvm-mesh

Commons Mesh networking module for bllvm-node.

## Overview

This module provides Commons Mesh networking capabilities for bllvm-node, including:
- Payment-gated routing
- Traffic classification (free vs paid)
- Fee distribution
- Anti-monopoly protection
- Network state tracking

## Installation

```bash
# Install via cargo
cargo install bllvm-mesh

# Or install via cargo-bllvm-module
cargo install cargo-bllvm-module
cargo bllvm-module install bllvm-mesh
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
name = "bllvm-mesh"
version = "0.1.0"
description = "Commons Mesh networking module"
author = "Bitcoin Commons Team"
entry_point = "bllvm-mesh"

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

## License

MIT License - see LICENSE file for details.

