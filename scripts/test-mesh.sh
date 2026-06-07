#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
echo "== blvm-mesh unit + integration (default features) =="
cargo test -p blvm-mesh
echo "== blvm-mesh with CTV feature =="
cargo test -p blvm-mesh --features ctv
echo "ok"
