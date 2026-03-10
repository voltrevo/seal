#!/usr/bin/env node
// Build a zip.br bundle from a Vite app's dist/ directory.
// Usage: node build-bundle.mjs <app-dir> <output-dir>
//
// Outputs:
//   <output-dir>/<contentHash>.zip.br
//   Prints the content hash to stdout.
//
// Content hash: keccak256 truncated to 192 bits, base36 encoded (~38 chars).
// Same encoding used for <hash>--keccak.seal local app URLs.

import { execSync } from 'child_process';
import { readFileSync, writeFileSync, mkdirSync } from 'fs';
import { join, resolve } from 'path';
import { brotliCompressSync, constants } from 'zlib';

const [appDir, outputDir] = process.argv.slice(2);
if (!appDir || !outputDir) {
  console.error('Usage: node build-bundle.mjs <app-dir> <output-dir>');
  process.exit(1);
}

const distDir = join(resolve(appDir), 'dist');
const zipPath = join(resolve(appDir), 'bundle.zip');

// Create zip of dist contents (files at root of zip, not nested under dist/)
execSync(`cd "${distDir}" && zip -r "${zipPath}" .`, { stdio: 'pipe' });

const zipData = readFileSync(zipPath);

// Keccak256 (pure JS — no external dependencies)
const hexHash = keccak256(zipData);

// Truncate to 192 bits (24 bytes = 48 hex chars) and base36 encode
const truncated = Buffer.from(hexHash.slice(0, 48), 'hex');
const contentHash = base36Encode(truncated);

// Brotli compress
const compressed = brotliCompressSync(zipData, {
  params: {
    [constants.BROTLI_PARAM_QUALITY]: 11,
  },
});

mkdirSync(resolve(outputDir), { recursive: true });
const outPath = join(resolve(outputDir), `${contentHash}.zip.br`);
writeFileSync(outPath, compressed);

// Clean up temp zip
execSync(`rm "${zipPath}"`);

console.log(contentHash);

// --- keccak256 (FIPS 202 / Ethereum variant) ---

function keccak256(data) {
  const ROUNDS = 24;
  const RC = [
    0x0000000000000001n, 0x0000000000008082n, 0x800000000000808an, 0x8000000080008000n,
    0x000000000000808bn, 0x0000000080000001n, 0x8000000080008081n, 0x8000000000008009n,
    0x000000000000008an, 0x0000000000000088n, 0x0000000080008009n, 0x000000008000000an,
    0x000000008000808bn, 0x800000000000008bn, 0x8000000000008089n, 0x8000000000008003n,
    0x8000000000008002n, 0x8000000000000080n, 0x000000000000800an, 0x800000008000000an,
    0x8000000080008081n, 0x8000000000008080n, 0x0000000080000001n, 0x8000000080008008n,
  ];
  const ROTATIONS = [
    [0,36,3,41,18],[1,44,10,45,2],[62,6,43,15,61],[28,55,25,21,56],[27,20,39,8,14]
  ];
  const mask64 = 0xffffffffffffffffn;

  // State: 5x5 array of 64-bit lanes
  const state = Array.from({length: 5}, () => Array(5).fill(0n));

  // Padding: keccak uses 0x01 suffix (not SHA3's 0x06)
  const rate = 136; // bytes (1088 bits for keccak256)
  const buf = Buffer.alloc(Math.ceil((data.length + 1) / rate) * rate);
  data.copy ? data.copy(buf) : Buffer.from(data).copy(buf);
  buf[data.length] |= 0x01;
  buf[buf.length - 1] |= 0x80;

  // Absorb
  for (let offset = 0; offset < buf.length; offset += rate) {
    for (let i = 0; i < rate / 8; i++) {
      const x = i % 5, y = Math.floor(i / 5);
      let lane = 0n;
      for (let b = 0; b < 8; b++) lane |= BigInt(buf[offset + i * 8 + b]) << BigInt(b * 8);
      state[x][y] ^= lane;
    }
    // Keccak-f[1600]
    for (let round = 0; round < ROUNDS; round++) {
      // θ
      const C = Array(5);
      for (let x = 0; x < 5; x++) C[x] = state[x][0] ^ state[x][1] ^ state[x][2] ^ state[x][3] ^ state[x][4];
      const D = Array(5);
      for (let x = 0; x < 5; x++) D[x] = C[(x + 4) % 5] ^ (((C[(x + 1) % 5] << 1n) | (C[(x + 1) % 5] >> 63n)) & mask64);
      for (let x = 0; x < 5; x++) for (let y = 0; y < 5; y++) state[x][y] = (state[x][y] ^ D[x]) & mask64;
      // ρ and π
      const B = Array.from({length: 5}, () => Array(5).fill(0n));
      for (let x = 0; x < 5; x++) for (let y = 0; y < 5; y++) {
        const r = ROTATIONS[x][y];
        const rot = r === 0 ? state[x][y] : (((state[x][y] << BigInt(r)) | (state[x][y] >> BigInt(64 - r))) & mask64);
        B[y][(2 * x + 3 * y) % 5] = rot;
      }
      // χ
      for (let x = 0; x < 5; x++) for (let y = 0; y < 5; y++)
        state[x][y] = (B[x][y] ^ ((~B[(x + 1) % 5][y] & mask64) & B[(x + 2) % 5][y])) & mask64;
      // ι
      state[0][0] = (state[0][0] ^ RC[round]) & mask64;
    }
  }

  // Squeeze 256 bits
  let hex = '';
  for (let i = 0; i < 4; i++) {
    const x = i % 5, y = Math.floor(i / 5);
    for (let b = 0; b < 8; b++) hex += Number((state[x][y] >> BigInt(b * 8)) & 0xffn).toString(16).padStart(2, '0');
  }
  return hex;
}

// Base36 encode a byte array (0-9, a-z). Same algorithm as Seal daemon's base36_encode.
function base36Encode(bytes) {
  const ALPHABET = '0123456789abcdefghijklmnopqrstuvwxyz';
  if (bytes.every(b => b === 0)) return '0';

  const num = [...bytes]; // mutable copy
  const digits = [];
  while (num.some(b => b !== 0)) {
    let remainder = 0;
    for (let i = 0; i < num.length; i++) {
      const val = (remainder << 8) | num[i];
      num[i] = Math.floor(val / 36);
      remainder = val % 36;
    }
    digits.push(ALPHABET[remainder]);
  }
  digits.reverse();
  return digits.join('');
}
