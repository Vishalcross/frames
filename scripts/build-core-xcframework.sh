#!/usr/bin/env bash
# Build frames-core for the Mac app (frames_swift_spec.md section 5.1): the
# Rust static library, its generated Swift bindings, and an XCFramework.
# Everything lands in apps/mac/Generated/, which is never committed. Xcode
# runs this before each build of the app; files that didn't change are left
# alone, so the app relinks only after a real change.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET=aarch64-apple-darwin
PROFILE=ffi
LIB_DIR="$ROOT/target/$TARGET/$PROFILE"
STAGE="$ROOT/target/frames-ffi-stage"
OUT="$ROOT/apps/mac/Generated"
# The app's minimum macOS (spec decision 1). Without it, the C code in the
# core (SQLite, libwebp, ring) is built for whatever macOS this Mac runs.
export MACOSX_DEPLOYMENT_TARGET=26.0
# Xcode runs build scripts with a minimal PATH.
export PATH="$HOME/.cargo/bin:$PATH"

cd "$ROOT"
cargo build -p frames-ffi --lib --profile "$PROFILE" --target "$TARGET"

rm -rf "$STAGE"
mkdir -p "$STAGE/FramesCore" "$STAGE/headers"
cargo run -q -p frames-ffi --bin uniffi-bindgen -- generate \
  --library "$LIB_DIR/libframes_ffi.dylib" --language swift --out-dir "$STAGE/bindings"

mv "$STAGE/bindings/FramesCore.swift" "$STAGE/FramesCore/"
mv "$STAGE/bindings/FramesCoreFFI.h" "$STAGE/headers/"
mv "$STAGE/bindings/FramesCoreFFI.modulemap" "$STAGE/headers/module.modulemap"
xcodebuild -create-xcframework \
  -library "$LIB_DIR/libframes_ffi.a" -headers "$STAGE/headers" \
  -output "$STAGE/FramesCoreFFI.xcframework" >/dev/null
rm -rf "$STAGE/headers" "$STAGE/bindings"

mkdir -p "$OUT"
rsync -a --checksum --delete "$STAGE/" "$OUT/"
echo "Updated $OUT"
