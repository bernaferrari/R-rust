#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR/rstudio-mobile"

./gradlew :webApp:wasmJsDevelopmentExecutableCompileSync --console=plain --no-daemon
echo "Kotlin/Wasm web target check passed."
