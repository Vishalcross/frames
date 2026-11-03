# Frames Release and Packaging Guide

This guide describes how to build, package, and release Frames for macOS and Windows with bundled ffmpeg and ffprobe sidecars.

---

## 1. Prerequisites and Sidecar Naming Rules

Frames relies on bundled `ffmpeg` and `ffprobe` static binaries to decode video frames, generate thumbnails, probe metadata, and transcode HEIC/video formats out of the box without requiring system-level installations.

### Sidecar Location
All sidecar binaries live in:
```text
frames-shell/src-tauri/binaries/
```

### Sidecar Naming Convention
Tauri requires external binaries (`externalBin`) to be named with the exact Rust target triple:

- **Apple Silicon Mac (ARM64)**:
  - `ffmpeg-aarch64-apple-darwin`
  - `ffprobe-aarch64-apple-darwin`
- **Intel Mac (x86_64)**:
  - `ffmpeg-x86_64-apple-darwin`
  - `ffprobe-x86_64-apple-darwin`
- **Universal macOS (both ARM64 and x86_64 via lipo)**:
  - `ffmpeg-universal-apple-darwin`
  - `ffprobe-universal-apple-darwin`
- **Windows 64-bit MSVC (default)**:
  - `ffmpeg-x86_64-pc-windows-msvc.exe`
  - `ffprobe-x86_64-pc-windows-msvc.exe`
- **Windows 64-bit GNU (MinGW)**:
  - `ffmpeg-x86_64-pc-windows-gnu.exe`
  - `ffprobe-x86_64-pc-windows-gnu.exe`
- **Windows ARM64 (Snapdragon / Surface)**:
  - `ffmpeg-aarch64-pc-windows-msvc.exe`
  - `ffprobe-aarch64-pc-windows-msvc.exe`

### Automatic Binary Preparation
A helper script is available at `scripts/prepare-binaries.js`. It downloads the static binaries if missing, extracts them, marks them executable, creates universal lipo binaries on macOS, and generates Windows toolchain aliases:
```bash
node scripts/prepare-binaries.js
```
This script runs automatically during `npm run tauri build`, `npm run bundle`, and `npm run build`.

---

## 2. Building for macOS

### Commands
On macOS (MacBook / Mac Studio):
```bash
cd frames-shell
npm install
# Builds macOS .app bundle directly:
npm run bundle:mac
# Or standard tauri build (configured for .app on macOS and .nsis on Windows):
npm run bundle
```

### Output Paths
- **Application Bundle**:
  `target/release/bundle/macos/Frames.app`
- **DMG Installer**:
  `target/release/bundle/dmg/Frames_0.1.0_aarch64.dmg` (or `_x64.dmg` / `_universal.dmg`)
- **Direct Executable**:
  `target/release/frames-shell`

### Bundled Sidecar Verification inside .app
Inside `Frames.app/Contents/MacOS/`:
- `frames-shell` (Main binary)
- `ffmpeg` (Bundled static transcoder)
- `ffprobe` (Bundled static media inspector)

### Signing, requirements and checks
- The bundle is ad-hoc signed (`bundle.macOS.signingIdentity: "-"`), so its
  signature seals the whole app. Check it with
  `codesign --verify --deep --strict target/release/bundle/macos/Frames.app`.
  Without this, a copied or downloaded app is reported as "damaged".
- macOS 11 or later: thumbnails are WebP, which the macOS web view shows from 11.
- The default build is for the Mac it runs on (Apple silicon here). For Intel
  Macs too, `rustup target add x86_64-apple-darwin` and build with
  `npm run tauri build -- --target universal-apple-darwin`.
- HEIC and preview-less RAW (phone DNG) thumbnails use macOS's own decoder,
  because the bundled ffmpeg cannot assemble HEIC tile grids. Import on the
  Mac: Windows has no such decoder here and would store placeholders.
- `scripts/prepare-binaries.js` pins each sidecar's SHA-256 and refuses
  anything else.

### Gatekeeper / macOS Unidentified Developer Tip
Ad-hoc signing is not notarisation. On another Mac, macOS warns that the app is from an unidentified developer:
1. Right-click `Frames.app` in Finder, choose **Open**, and click **Open** in the confirmation dialog.
2. Alternatively, remove the quarantine attribute via terminal:
   ```bash
   xattr -cr /Applications/Frames.app
   ```

---

## 3. Building for Windows

### Prerequisites on Windows
1. Visual Studio 2022 Build Tools (with "Desktop development with C++" workload).
2. Rust (`rustup-init.exe`, default target `x86_64-pc-windows-msvc`).
3. Node.js LTS (v20+ or v22+).
4. (Optional) Inno Setup 6 (for standalone Inno Setup packaging via `installer/frames.iss`).

### Commands (NSIS Installer)
From PowerShell or CMD:
```powershell
cd frames-shell
npm install
npm run bundle:win
# Or standard build:
npm run bundle
```

### Output Paths (NSIS)
- **NSIS Setup Installer**:
  `target\release\bundle\nsis\Frames_0.1.0_x64-setup.exe`
- **Standalone Executable**:
  `target\release\frames-shell.exe`
- **Bundled Binaries**:
  `ffmpeg.exe` and `ffprobe.exe` installed alongside `Frames.exe` in `{autopf}\Frames\`.

### Alternative Packaging: Inno Setup 6
An Inno Setup script is provided at `installer/frames.iss`:
```powershell
iscc installer\frames.iss
```
This generates:
- `target\release\bundle\inno\Frames_0.1.0_x64-setup.exe`

---

## 4. Testing & Verification

Run the test suite across both crates to verify dynamic path resolution and sidecar availability:
```bash
cargo test
cargo check
npm --prefix frames-shell run check
```
