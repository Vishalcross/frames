#!/usr/bin/env bash
# Build frames-core for the Mac app (frames_swift_spec.md section 5.1): the
# Rust static library, its generated Swift bindings, and an XCFramework.
# Everything lands in apps/mac/Generated/, which is never committed.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET=aarch64-apple-darwin
PROFILE=ffi
LIB_DIR="$ROOT/target/$TARGET/$PROFILE"
OUT="$ROOT/apps/mac/Generated"
# The app's minimum macOS (spec decision 1). Without it, the C code in the
# core (SQLite, libwebp, ring) is built for whatever macOS this Mac runs.
export MACOSX_DEPLOYMENT_TARGET=26.0

cd "$ROOT"
cargo build -p frames-ffi --lib --profile "$PROFILE" --target "$TARGET"

rm -rf "$OUT"
mkdir -p "$OUT/FramesCore" "$OUT/headers"
cargo run -q -p frames-ffi --bin uniffi-bindgen -- generate \
  --library "$LIB_DIR/libframes_ffi.dylib" --language swift --out-dir "$OUT/bindings"

mv "$OUT/bindings/FramesCore.swift" "$OUT/FramesCore/"
mv "$OUT/bindings/FramesCoreFFI.h" "$OUT/headers/"
mv "$OUT/bindings/FramesCoreFFI.modulemap" "$OUT/headers/module.modulemap"
rm -rf "$OUT/bindings"

xcodebuild -create-xcframework \
  -library "$LIB_DIR/libframes_ffi.a" -headers "$OUT/headers" \
  -output "$OUT/FramesCoreFFI.xcframework" >/dev/null
rm -rf "$OUT/headers"
echo "Built $OUT/FramesCoreFFI.xcframework and $OUT/FramesCore/FramesCore.swift"
