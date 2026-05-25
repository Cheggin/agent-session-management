#!/usr/bin/env node
// Downloads the prebuilt `asm` binary matching this platform from the matching
// GitHub Release and writes it to ./bin/asm. Run automatically by npm via the
// `postinstall` script. Safe to re-run; replaces the existing binary in place.

const fs = require('node:fs');
const path = require('node:path');
const https = require('node:https');
const zlib = require('node:zlib');
const { execSync } = require('node:child_process');

const REPO = 'Cheggin/agent-session-management';
const pkg = require('./package.json');
const VERSION = pkg.version;

const PLATFORM_MAP = {
  'darwin-arm64': 'aarch64-apple-darwin',
  'darwin-x64': 'x86_64-apple-darwin',
  'linux-x64': 'x86_64-unknown-linux-gnu',
};

function detectTarget() {
  const key = `${process.platform}-${process.arch}`;
  const target = PLATFORM_MAP[key];
  if (!target) {
    const supported = Object.keys(PLATFORM_MAP).join(', ');
    throw new Error(
      `@reaganhsu/asm has no prebuilt binary for ${key}. ` +
      `Supported: ${supported}. ` +
      `Install from source: https://github.com/${REPO}#install-from-source`
    );
  }
  return target;
}

function get(url) {
  return new Promise((resolve, reject) => {
    https
      .get(url, { headers: { 'user-agent': `@reaganhsu/asm v${VERSION}` } }, (res) => {
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          res.resume();
          resolve(get(res.headers.location));
          return;
        }
        if (res.statusCode !== 200) {
          res.resume();
          reject(new Error(`GET ${url} → HTTP ${res.statusCode}`));
          return;
        }
        const chunks = [];
        res.on('data', (chunk) => chunks.push(chunk));
        res.on('end', () => resolve(Buffer.concat(chunks)));
        res.on('error', reject);
      })
      .on('error', reject);
  });
}

// The release artifact is `asm-<target>.tar.gz` containing a single `asm`
// binary at the archive root. Decode gzip in Node, then shell out to `tar -x`
// to extract — avoids pulling a tar library dependency.
async function downloadAndExtract(target, destBin) {
  const tarGzUrl = `https://github.com/${REPO}/releases/download/v${VERSION}/asm-${target}.tar.gz`;
  process.stdout.write(`@reaganhsu/asm: fetching ${tarGzUrl}\n`);
  const gzipped = await get(tarGzUrl);

  const tmpDir = fs.mkdtempSync(path.join(require('node:os').tmpdir(), 'asm-install-'));
  const tarPath = path.join(tmpDir, 'asm.tar');
  fs.writeFileSync(tarPath, zlib.gunzipSync(gzipped));

  try {
    execSync(`tar -xf ${JSON.stringify(tarPath)} -C ${JSON.stringify(tmpDir)} asm`, {
      stdio: 'inherit',
    });
    const extractedBin = path.join(tmpDir, 'asm');
    fs.mkdirSync(path.dirname(destBin), { recursive: true });
    fs.copyFileSync(extractedBin, destBin);
    fs.chmodSync(destBin, 0o755);
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
}

function maybeInstallShellHook(asmBin) {
  if (process.env.ASM_SKIP_SHELL_HOOK === '1') {
    process.stdout.write('@reaganhsu/asm: ASM_SKIP_SHELL_HOOK=1, skipping shell hook setup\n');
    return;
  }
  const shell = process.env.SHELL || '';
  if (!shell.endsWith('/zsh') && shell !== 'zsh') {
    process.stdout.write(
      `@reaganhsu/asm: shell is ${shell || 'unknown'}, not zsh — skipping hook. ` +
      `Re-run 'asm install' manually if you switch to zsh.\n`
    );
    return;
  }
  try {
    execSync(`${JSON.stringify(asmBin)} install`, { stdio: 'inherit' });
  } catch (err) {
    process.stderr.write(
      `@reaganhsu/asm: 'asm install' failed (${err.message}); ` +
      `run it manually to enable claude --resume / codex resume proxying.\n`
    );
  }
}

async function main() {
  if (process.env.ASM_SKIP_DOWNLOAD === '1') {
    process.stdout.write('@reaganhsu/asm: ASM_SKIP_DOWNLOAD=1, skipping binary download\n');
    return;
  }
  const target = detectTarget();
  const destBin = path.join(__dirname, 'bin', 'asm');
  await downloadAndExtract(target, destBin);
  process.stdout.write(`@reaganhsu/asm: installed ${destBin}\n`);
  maybeInstallShellHook(destBin);
}

main().catch((err) => {
  process.stderr.write(`@reaganhsu/asm: install failed: ${err.message}\n`);
  process.exit(1);
});
