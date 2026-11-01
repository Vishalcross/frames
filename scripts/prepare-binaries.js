import fs from 'node:fs';
import path from 'node:path';
import https from 'node:https';
import zlib from 'node:zlib';
import { fileURLToPath } from 'node:url';
import { execSync } from 'node:child_process';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const projectRoot = path.resolve(__dirname, '..');
const binariesDir = path.resolve(projectRoot, 'frames-shell/src-tauri/binaries');

if (!fs.existsSync(binariesDir)) {
  fs.mkdirSync(binariesDir, { recursive: true });
}

const BASE_URL = 'https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1';

const BINARIES = [
  {
    remoteGz: 'ffmpeg-darwin-arm64.gz',
    target: 'ffmpeg-aarch64-apple-darwin',
    executable: true
  },
  {
    remoteGz: 'ffprobe-darwin-arm64.gz',
    target: 'ffprobe-aarch64-apple-darwin',
    executable: true
  },
  {
    remoteGz: 'ffmpeg-darwin-x64.gz',
    target: 'ffmpeg-x86_64-apple-darwin',
    executable: true
  },
  {
    remoteGz: 'ffprobe-darwin-x64.gz',
    target: 'ffprobe-x86_64-apple-darwin',
    executable: true
  },
  {
    remoteGz: 'ffmpeg-win32-x64.gz',
    target: 'ffmpeg-x86_64-pc-windows-msvc.exe',
    executable: false
  },
  {
    remoteGz: 'ffprobe-win32-x64.gz',
    target: 'ffprobe-x86_64-pc-windows-msvc.exe',
    executable: false
  }
];

function downloadAndGunzip(url, destPath, isExecutable) {
  return new Promise((resolve, reject) => {
    if (fs.existsSync(destPath) && fs.statSync(destPath).size > 1000000) {
      console.log(`[skip] Already exists: ${path.basename(destPath)} (${fs.statSync(destPath).size} bytes)`);
      if (isExecutable) {
        try { fs.chmodSync(destPath, 0o755); } catch {}
      }
      return resolve();
    }

    console.log(`[downloading] ${url} -> ${path.basename(destPath)}...`);
    const fileStream = fs.createWriteStream(destPath);
    const gunzip = zlib.createGunzip();

    function followRedirect(curUrl) {
      https.get(curUrl, (res) => {
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          followRedirect(res.headers.location);
          return;
        }
        if (res.statusCode !== 200) {
          reject(new Error(`Failed to download ${curUrl}: status code ${res.statusCode}`));
          return;
        }

        res.pipe(gunzip).pipe(fileStream);

        fileStream.on('finish', () => {
          fileStream.close(() => {
            if (isExecutable) {
              try { fs.chmodSync(destPath, 0o755); } catch {}
            }
            console.log(`[done] Saved ${path.basename(destPath)} (${fs.statSync(destPath).size} bytes)`);
            resolve();
          });
        });

        gunzip.on('error', (err) => {
          fs.unlink(destPath, () => {});
          reject(err);
        });

        fileStream.on('error', (err) => {
          fs.unlink(destPath, () => {});
          reject(err);
        });
      }).on('error', (err) => {
        fs.unlink(destPath, () => {});
        reject(err);
      });
    }

    followRedirect(url);
  });
}

async function main() {
  console.log(`Preparing binaries in: ${binariesDir}`);

  // Also check if local /tmp downloads exist to reuse them
  if (fs.existsSync('/tmp/ffmpeg-darwin-arm64.gz')) {
    const dest = path.join(binariesDir, 'ffmpeg-aarch64-apple-darwin');
    if (!fs.existsSync(dest) || fs.statSync(dest).size < 1000000) {
      console.log('Unpacking from /tmp/ffmpeg-darwin-arm64.gz...');
      execSync(`gzip -d -c /tmp/ffmpeg-darwin-arm64.gz > "${dest}" && chmod +x "${dest}"`);
    }
  }
  if (fs.existsSync('/tmp/ffprobe-darwin-arm64.gz')) {
    const dest = path.join(binariesDir, 'ffprobe-aarch64-apple-darwin');
    if (!fs.existsSync(dest) || fs.statSync(dest).size < 1000000) {
      console.log('Unpacking from /tmp/ffprobe-darwin-arm64.gz...');
      execSync(`gzip -d -c /tmp/ffprobe-darwin-arm64.gz > "${dest}" && chmod +x "${dest}"`);
    }
  }

  for (const bin of BINARIES) {
    const destPath = path.join(binariesDir, bin.target);
    const url = `${BASE_URL}/${bin.remoteGz}`;
    try {
      await downloadAndGunzip(url, destPath, bin.executable);
    } catch (e) {
      console.error(`Error downloading ${bin.target}:`, e.message);
      process.exitCode = 1;
    }
  }

  // Create universal macOS binaries if lipo is available
  try {
    const ffmpegArm = path.join(binariesDir, 'ffmpeg-aarch64-apple-darwin');
    const ffmpegX64 = path.join(binariesDir, 'ffmpeg-x86_64-apple-darwin');
    const ffmpegUniversal = path.join(binariesDir, 'ffmpeg-universal-apple-darwin');

    if (fs.existsSync(ffmpegArm) && fs.existsSync(ffmpegX64)) {
      console.log('Creating universal binary for ffmpeg...');
      execSync(`lipo -create -output "${ffmpegUniversal}" "${ffmpegArm}" "${ffmpegX64}"`);
      fs.chmodSync(ffmpegUniversal, 0o755);
      console.log(`[done] Saved ffmpeg-universal-apple-darwin (${fs.statSync(ffmpegUniversal).size} bytes)`);
    }

    const ffprobeArm = path.join(binariesDir, 'ffprobe-aarch64-apple-darwin');
    const ffprobeX64 = path.join(binariesDir, 'ffprobe-x86_64-apple-darwin');
    const ffprobeUniversal = path.join(binariesDir, 'ffprobe-universal-apple-darwin');

    if (fs.existsSync(ffprobeArm) && fs.existsSync(ffprobeX64)) {
      console.log('Creating universal binary for ffprobe...');
      execSync(`lipo -create -output "${ffprobeUniversal}" "${ffprobeArm}" "${ffprobeX64}"`);
      fs.chmodSync(ffprobeUniversal, 0o755);
    }
  } catch (err) {
    console.warn('Universal binary lipo step skipped or failed:', err.message);
  }

  // Create Windows target variants (x86_64 GNU and ARM64 MSVC) from MSVC binary
  try {
    const ffmpegMsvc = path.join(binariesDir, 'ffmpeg-x86_64-pc-windows-msvc.exe');
    const ffmpegGnu = path.join(binariesDir, 'ffmpeg-x86_64-pc-windows-gnu.exe');
    const ffmpegArm64Win = path.join(binariesDir, 'ffmpeg-aarch64-pc-windows-msvc.exe');
    if (fs.existsSync(ffmpegMsvc)) {
      if (!fs.existsSync(ffmpegGnu)) fs.copyFileSync(ffmpegMsvc, ffmpegGnu);
      if (!fs.existsSync(ffmpegArm64Win)) fs.copyFileSync(ffmpegMsvc, ffmpegArm64Win);
    }

    const ffprobeMsvc = path.join(binariesDir, 'ffprobe-x86_64-pc-windows-msvc.exe');
    const ffprobeGnu = path.join(binariesDir, 'ffprobe-x86_64-pc-windows-gnu.exe');
    const ffprobeArm64Win = path.join(binariesDir, 'ffprobe-aarch64-pc-windows-msvc.exe');
    if (fs.existsSync(ffprobeMsvc)) {
      if (!fs.existsSync(ffprobeGnu)) fs.copyFileSync(ffprobeMsvc, ffprobeGnu);
      if (!fs.existsSync(ffprobeArm64Win)) fs.copyFileSync(ffprobeMsvc, ffprobeArm64Win);
    }
  } catch (err) {
    console.warn('Windows binary variant copying skipped or failed:', err.message);
  }

  console.log('All binaries prepared successfully:');
  for (const f of fs.readdirSync(binariesDir)) {
    const stat = fs.statSync(path.join(binariesDir, f));
    console.log(` - ${f} (${(stat.size / 1024 / 1024).toFixed(2)} MB)`);
  }
}

main().catch(err => {
  console.error(err);
  process.exit(1);
});
