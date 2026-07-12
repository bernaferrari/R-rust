#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR/rstudio-mobile"

if command -v node >/dev/null 2>&1; then export NODE_BINARY="$(command -v node)"; fi
if command -v yarn >/dev/null 2>&1; then export YARN_BINARY="$(command -v yarn)"; fi

./gradlew :webApp:wasmJsBrowserDevelopmentWebpack --console=plain --no-daemon
echo "Kotlin/Wasm web target check passed."
