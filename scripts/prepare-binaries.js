import fs from 'node:fs';
import path from 'node:path';
import https from 'node:https';
import zlib from 'node:zlib';
import { fileURLToPath } from 'node:url';
import { execSync } from 'node:child_process';
import crypto from 'node:crypto';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const projectRoot = path.resolve(__dirname, '..');
const binariesDir = path.resolve(projectRoot, 'frames-shell/src-tauri/binaries');

if (!fs.existsSync(binariesDir)) {
  fs.mkdirSync(binariesDir, { recursive: true });
}

const BASE_URL = 'https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1';

// Each sidecar is pinned to the SHA-256 of its unpacked binary, so a changed
// or tampered download (or a stray file in /tmp) is never bundled.
const BINARIES = [
  {
    remoteGz: 'ffmpeg-darwin-arm64.gz',
    target: 'ffmpeg-aarch64-apple-darwin',
    sha256: 'a90e3db6a3fd35f6074b013f948b1aa45b31c6375489d39e572bea3f18336584',
    executable: true
  },
  {
    remoteGz: 'ffprobe-darwin-arm64.gz',
    target: 'ffprobe-aarch64-apple-darwin',
    sha256: 'bb2db6f5d8cef919da12fbf592119a987202a8c060a886f3cab091f9cab90b64',
    executable: true
  },
  {
    remoteGz: 'ffmpeg-darwin-x64.gz',
    target: 'ffmpeg-x86_64-apple-darwin',
    sha256: 'ebdddc936f61e14049a2d4b549a412b8a40deeff6540e58a9f2a2da9e6b18894',
    executable: true
  },
  {
    remoteGz: 'ffprobe-darwin-x64.gz',
    target: 'ffprobe-x86_64-apple-darwin',
    sha256: 'fa3add0ce901f7241abe0dfc0155d958fc834aca3f8ce61f87cc712ae669c1e0',
    executable: true
  },
  {
    remoteGz: 'ffmpeg-win32-x64.gz',
    target: 'ffmpeg-x86_64-pc-windows-msvc.exe',
    sha256: '04e1307997530f9cf2fe35cba2ca7e8875ca91da02f89d6c7243df819c94ad00',
    executable: false
  },
  {
    remoteGz: 'ffprobe-win32-x64.gz',
    target: 'ffprobe-x86_64-pc-windows-msvc.exe',
    sha256: '3a7e2dc003dc2cd1472827e4c7c4f056ae1ae0ae7c5bbc580c99b49827351ba4',
    executable: false
  }
];

function sha256Of(file) {
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
}

function downloadAndGunzip(url, destPath, isExecutable, expectedSha256) {
  return new Promise((resolve, reject) => {
    if (fs.existsSync(destPath) && sha256Of(destPath) !== expectedSha256) {
      console.warn(`[re-download] ${path.basename(destPath)} does not match its pinned SHA-256`);
      fs.unlinkSync(destPath);
    }
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
            const actual = sha256Of(destPath);
            if (actual !== expectedSha256) {
              fs.unlinkSync(destPath);
              reject(new Error(`SHA-256 mismatch for ${path.basename(destPath)}: got ${actual}`));
              return;
            }
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
      await downloadAndGunzip(url, destPath, bin.executable, bin.sha256);
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
