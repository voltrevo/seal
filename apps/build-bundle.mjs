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
//
// Uses a deterministic store-only zip encoder (no compression, zeroed
// timestamps, sorted entries) so the output is identical across platforms.
// Brotli handles the actual compression.

import { readdirSync, readFileSync, statSync, writeFileSync, mkdirSync } from 'fs';
import { join, resolve } from 'path';
import { brotliCompressSync, constants } from 'zlib';

const [appDir, outputDir] = process.argv.slice(2);
if (!appDir || !outputDir) {
  console.error('Usage: node build-bundle.mjs <app-dir> <output-dir>');
  process.exit(1);
}

const distDir = join(resolve(appDir), 'dist');

// Collect all files recursively, sorted for determinism
function walkDir(dir, base = '') {
  const entries = [];
  for (const name of readdirSync(dir).sort()) {
    const full = join(dir, name);
    const rel = base ? `${base}/${name}` : name;
    const st = statSync(full);
    if (st.isDirectory()) {
      entries.push(...walkDir(full, rel));
    } else {
      entries.push({ path: rel, data: readFileSync(full) });
    }
  }
  return entries;
}

const files = walkDir(distDir);
const zipData = createStoreZip(files);

// Keccak256 (pure JS)
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

console.log(contentHash);

// --- Store-only ZIP encoder (deterministic) ---

function createStoreZip(files) {
  // All timestamps zeroed for reproducibility (DOS date: 0x0021 = 1980-01-01, time: 0x0000)
  const dosTime = 0x0000;
  const dosDate = 0x0021;

  const localHeaders = [];
  const centralHeaders = [];
  let offset = 0;

  for (const { path, data } of files) {
    const nameBytes = Buffer.from(path, 'utf8');
    const crc = crc32(data);

    // Local file header (30 bytes + name + data)
    const local = Buffer.alloc(30 + nameBytes.length + data.length);
    local.writeUInt32LE(0x04034b50, 0);   // signature
    local.writeUInt16LE(20, 4);            // version needed (2.0)
    local.writeUInt16LE(0, 6);             // flags
    local.writeUInt16LE(0, 8);             // compression: store
    local.writeUInt16LE(dosTime, 10);      // mod time
    local.writeUInt16LE(dosDate, 12);      // mod date
    local.writeUInt32LE(crc, 14);          // crc32
    local.writeUInt32LE(data.length, 18);  // compressed size
    local.writeUInt32LE(data.length, 22);  // uncompressed size
    local.writeUInt16LE(nameBytes.length, 26); // name length
    local.writeUInt16LE(0, 28);            // extra field length
    nameBytes.copy(local, 30);
    data.copy(local, 30 + nameBytes.length);
    localHeaders.push(local);

    // Central directory header (46 bytes + name)
    const central = Buffer.alloc(46 + nameBytes.length);
    central.writeUInt32LE(0x02014b50, 0);  // signature
    central.writeUInt16LE(20, 4);           // version made by
    central.writeUInt16LE(20, 6);           // version needed
    central.writeUInt16LE(0, 8);            // flags
    central.writeUInt16LE(0, 10);           // compression: store
    central.writeUInt16LE(dosTime, 12);     // mod time
    central.writeUInt16LE(dosDate, 14);     // mod date
    central.writeUInt32LE(crc, 16);         // crc32
    central.writeUInt32LE(data.length, 20); // compressed size
    central.writeUInt32LE(data.length, 24); // uncompressed size
    central.writeUInt16LE(nameBytes.length, 28); // name length
    central.writeUInt16LE(0, 30);           // extra field length
    central.writeUInt16LE(0, 32);           // comment length
    central.writeUInt16LE(0, 34);           // disk number start
    central.writeUInt16LE(0, 36);           // internal attrs
    central.writeUInt32LE(0, 38);           // external attrs
    central.writeUInt32LE(offset, 42);      // local header offset
    nameBytes.copy(central, 46);
    centralHeaders.push(central);

    offset += local.length;
  }

  const centralOffset = offset;
  const centralSize = centralHeaders.reduce((s, h) => s + h.length, 0);

  // End of central directory (22 bytes)
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);       // signature
  eocd.writeUInt16LE(0, 4);                // disk number
  eocd.writeUInt16LE(0, 6);                // central dir disk
  eocd.writeUInt16LE(files.length, 8);     // entries on disk
  eocd.writeUInt16LE(files.length, 10);    // total entries
  eocd.writeUInt32LE(centralSize, 12);     // central dir size
  eocd.writeUInt32LE(centralOffset, 16);   // central dir offset
  eocd.writeUInt16LE(0, 20);               // comment length

  return Buffer.concat([...localHeaders, ...centralHeaders, eocd]);
}

// CRC32 (used by ZIP format)
function crc32(buf) {
  let crc = 0xffffffff;
  for (let i = 0; i < buf.length; i++) {
    crc ^= buf[i];
    for (let j = 0; j < 8; j++) {
      crc = (crc >>> 1) ^ (crc & 1 ? 0xedb88320 : 0);
    }
  }
  return (crc ^ 0xffffffff) >>> 0;
}

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

  const state = Array.from({length: 5}, () => Array(5).fill(0n));

  const rate = 136;
  const buf = Buffer.alloc(Math.ceil((data.length + 1) / rate) * rate);
  data.copy ? data.copy(buf) : Buffer.from(data).copy(buf);
  buf[data.length] |= 0x01;
  buf[buf.length - 1] |= 0x80;

  for (let offset = 0; offset < buf.length; offset += rate) {
    for (let i = 0; i < rate / 8; i++) {
      const x = i % 5, y = Math.floor(i / 5);
      let lane = 0n;
      for (let b = 0; b < 8; b++) lane |= BigInt(buf[offset + i * 8 + b]) << BigInt(b * 8);
      state[x][y] ^= lane;
    }
    for (let round = 0; round < ROUNDS; round++) {
      const C = Array(5);
      for (let x = 0; x < 5; x++) C[x] = state[x][0] ^ state[x][1] ^ state[x][2] ^ state[x][3] ^ state[x][4];
      const D = Array(5);
      for (let x = 0; x < 5; x++) D[x] = C[(x + 4) % 5] ^ (((C[(x + 1) % 5] << 1n) | (C[(x + 1) % 5] >> 63n)) & mask64);
      for (let x = 0; x < 5; x++) for (let y = 0; y < 5; y++) state[x][y] = (state[x][y] ^ D[x]) & mask64;
      const B = Array.from({length: 5}, () => Array(5).fill(0n));
      for (let x = 0; x < 5; x++) for (let y = 0; y < 5; y++) {
        const r = ROTATIONS[x][y];
        const rot = r === 0 ? state[x][y] : (((state[x][y] << BigInt(r)) | (state[x][y] >> BigInt(64 - r))) & mask64);
        B[y][(2 * x + 3 * y) % 5] = rot;
      }
      for (let x = 0; x < 5; x++) for (let y = 0; y < 5; y++)
        state[x][y] = (B[x][y] ^ ((~B[(x + 1) % 5][y] & mask64) & B[(x + 2) % 5][y])) & mask64;
      state[0][0] = (state[0][0] ^ RC[round]) & mask64;
    }
  }

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

  const num = [...bytes];
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
