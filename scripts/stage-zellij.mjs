import { createHash } from 'node:crypto';
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { basename, join, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

export const ZELLIJ_VERSION = '0.45.0';

export const ZELLIJ_ARTIFACTS = Object.freeze({
  'x86_64-pc-windows-msvc': Object.freeze({
    archive: 'zellij-no-web-x86_64-pc-windows-msvc.zip',
    executable: 'zellij.exe',
    sha256: 'be22d0bc16d02cef21ab20554cd5adf208dbdeae6fc09c624765b4baea5d381c',
  }),
  'x86_64-unknown-linux-musl': Object.freeze({
    archive: 'zellij-no-web-x86_64-unknown-linux-musl.tar.gz',
    executable: 'zellij',
    sha256: 'a9331a7ac3e62833e599e3bedd3bbad053437d66bcb447466f21c079c3d5c002',
  }),
  'aarch64-unknown-linux-musl': Object.freeze({
    archive: 'zellij-no-web-aarch64-unknown-linux-musl.tar.gz',
    executable: 'zellij',
    sha256: 'd2da64ca3bbd9f15b33ce91bf706b05d23e6d1865bdabc3b4aecab3391c683ab',
  }),
  'x86_64-apple-darwin': Object.freeze({
    archive: 'zellij-no-web-x86_64-apple-darwin.tar.gz',
    executable: 'zellij',
    sha256: '65b5514cd38ee75f464e981c3354d93921dd866a8deec02061bce3305db8222c',
  }),
  'aarch64-apple-darwin': Object.freeze({
    archive: 'zellij-no-web-aarch64-apple-darwin.tar.gz',
    executable: 'zellij',
    sha256: 'a6961dcbf401706198ef0ce6245fd4d0ece80aa95e1bfe1d73ff15e72d7635f0',
  }),
});

function repoRoot() {
  return resolve(fileURLToPath(new URL('..', import.meta.url)));
}

export function resolveZellijTarget({ platform, arch, rustTarget }) {
  if (rustTarget?.trim()) {
    const target = rustTarget.trim();
    if (ZELLIJ_ARTIFACTS[target]) return target;
    throw new Error(`Zellij ${ZELLIJ_VERSION} is not pinned for Rust target ${target}.`);
  }

  if (platform === 'win32' && arch === 'x64') return 'x86_64-pc-windows-msvc';
  if (platform === 'linux' && arch === 'x64') return 'x86_64-unknown-linux-musl';
  if (platform === 'linux' && arch === 'arm64') return 'aarch64-unknown-linux-musl';
  if (platform === 'darwin' && arch === 'x64') return 'x86_64-apple-darwin';
  if (platform === 'darwin' && arch === 'arm64') return 'aarch64-apple-darwin';

  throw new Error(`Zellij ${ZELLIJ_VERSION} is not pinned for ${platform}/${arch}.`);
}

export function sha256File(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

export function verifyZellijExecutable(path, artifact) {
  const actual = sha256File(path);
  if (actual !== artifact.sha256) {
    throw new Error(
      `Refusing to stage Zellij: SHA-256 mismatch for ${path}; expected ${artifact.sha256}, got ${actual}.`,
    );
  }
}

async function download(url, destination) {
  const response = await fetch(url, { redirect: 'follow' });
  if (!response.ok) {
    throw new Error(`Failed to download ${url}: HTTP ${response.status}.`);
  }
  writeFileSync(destination, Buffer.from(await response.arrayBuffer()));
}

function extractArchive(archive, destination) {
  mkdirSync(destination, { recursive: true });
  const result = spawnSync('tar', ['-xf', archive, '-C', destination], {
    stdio: 'inherit',
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`Failed to extract Zellij archive ${archive}.`);
  }
}

function resetExtractedRoot(cacheRoot, extractedRoot) {
  const resolvedCache = resolve(cacheRoot);
  const resolvedExtracted = resolve(extractedRoot);
  if (!resolvedExtracted.startsWith(`${resolvedCache}${sep}`)) {
    throw new Error(`Refusing to reset Zellij extraction outside ${resolvedCache}.`);
  }
  rmSync(resolvedExtracted, { recursive: true, force: true });
}

export async function stageZellij({
  root = repoRoot(),
  platform = process.platform,
  arch = process.arch,
  rustTarget = process.env.WARDIAN_CLI_TARGET,
} = {}) {
  const target = resolveZellijTarget({ platform, arch, rustTarget });
  const artifact = ZELLIJ_ARTIFACTS[target];
  const cacheRoot = join(root, '.tmp', 'zellij', ZELLIJ_VERSION, target);
  const extractedRoot = join(cacheRoot, 'extracted');
  const extractedExecutable = join(extractedRoot, artifact.executable);
  const destinationRoot = join(root, 'src-tauri', 'resources', 'bin');
  const destination = join(destinationRoot, artifact.executable);

  if (!existsSync(extractedExecutable)) {
    mkdirSync(cacheRoot, { recursive: true });
    const archivePath = join(cacheRoot, basename(artifact.archive));
    if (!existsSync(archivePath)) {
      const url = `https://github.com/zellij-org/zellij/releases/download/v${ZELLIJ_VERSION}/${artifact.archive}`;
      console.log(`Downloading pinned Zellij runtime ${artifact.archive}`);
      await download(url, archivePath);
    }
    resetExtractedRoot(cacheRoot, extractedRoot);
    extractArchive(archivePath, extractedRoot);
  }

  verifyZellijExecutable(extractedExecutable, artifact);
  mkdirSync(destinationRoot, { recursive: true });
  if (existsSync(destination)) {
    try {
      verifyZellijExecutable(destination, artifact);
      if (platform !== 'win32') chmodSync(destination, 0o755);
      return destination;
    } catch {
      // Replace a stale or corrupt destination with the verified extraction.
    }
  }
  copyFileSync(extractedExecutable, destination);
  if (platform !== 'win32') chmodSync(destination, 0o755);
  return destination;
}
