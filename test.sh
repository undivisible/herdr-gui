#!/usr/bin/env bash
set -euo pipefail

echo "=== crepuscularity + herdr-gui test ==="
echo ""

# 1. Test crepuscularity-core (parser, analysis, templates)
echo "→ Testing crepuscularity-core..."
cd /Users/undivisible/projects/crepuscularity
cargo test -p crepuscularity-core 2>&1 | tail -20
echo ""

# 2. Build crepuscularity crates (macros + gpui + runtime)
echo "→ Building crepuscularity-gpui..."
cargo check -p crepuscularity-gpui 2>&1 | tail -5
echo ""

# 3. Test herdr-gui
echo "→ Testing herdr-gui..."
cd /Users/undivisible/projects/herdr-gui
cargo fmt -- --check
echo "  fmt: ok"
cargo clippy --all-targets -- -D warnings 2>&1 | tail -3
echo "  clippy: ok"
cargo test 2>&1 | tail -15
echo ""

# 4. Build release binary
echo "→ Building release..."
cargo build --release 2>&1 | tail -5
echo ""

# 5. Smoke test — run the binary briefly (will fail if herdr socket missing, that's ok)
echo "→ Smoke test (expect herdr socket error if server not running)..."
timeout 3 ./target/release/herdr-gui 2>&1 || true
echo ""

echo "=== all checks passed ==="
