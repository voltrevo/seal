#!/usr/bin/env node
// Publish an app to the SealRegistry contract.
// Usage: node publish.mjs <app-dir> --private-key <key> [--rpc-url <url>] [--dry-run]
//
// Builds the app, computes the deterministic bundle hash, and calls
// SealRegistry.publish() on-chain.
//
// Requires: cast (foundry) for the on-chain transaction.

import { execSync } from 'child_process';
import { readFileSync, readdirSync, statSync } from 'fs';
import { join, resolve, basename } from 'path';

const REGISTRY = '0x0377Ef2b30CA1E93D54de6576CFb8E133663AD9E';
const DEFAULT_RPC = 'https://ethereum-rpc.publicnode.com';

// --- Parse args ---

const args = process.argv.slice(2);
let appDir = null;
let privateKey = null;
let rpcUrl = DEFAULT_RPC;
let dryRun = false;

for (let i = 0; i < args.length; i++) {
  if (args[i] === '--private-key') { privateKey = args[++i]; continue; }
  if (args[i] === '--rpc-url') { rpcUrl = args[++i]; continue; }
  if (args[i] === '--dry-run') { dryRun = true; continue; }
  if (!appDir) { appDir = args[i]; continue; }
}

if (!appDir || (!privateKey && !dryRun)) {
  console.error('Usage: node publish.mjs <app-dir> --private-key <key> [--rpc-url <url>] [--dry-run]');
  process.exit(1);
}

// --- Read manifest to get seal_url and bundleSources ---

const appName = basename(resolve(appDir));
const prettyName = appName.charAt(0).toUpperCase() + appName.slice(1);

// --- Build the app ---

console.log(`Building ${appDir}...`);
execSync('npm run build', { cwd: resolve(appDir), stdio: 'inherit' });

// --- Read manifest ---

const distDir = join(resolve(appDir), 'dist');
const manifestPath = join(distDir, '.seal', 'manifest.json');

let manifest;
try {
  manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
} catch {
  console.error(`Missing or invalid manifest at ${manifestPath}`);
  process.exit(1);
}

const { seal_url, owner } = manifest;
const bundleSources = manifest.bundleSources || [];

// --- Build deterministic zip and compute full keccak256 ---

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

function createStoreZip(files) {
  const dosTime = 0x0000;
  const dosDate = 0x0021;
  const localHeaders = [];
  const centralHeaders = [];
  let offset = 0;

  for (const { path, data } of files) {
    const nameBytes = Buffer.from(path, 'utf8');
    const crc = crc32(data);

    const local = Buffer.alloc(30 + nameBytes.length + data.length);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(20, 4);
    local.writeUInt16LE(0, 6);
    local.writeUInt16LE(0, 8);
    local.writeUInt16LE(dosTime, 10);
    local.writeUInt16LE(dosDate, 12);
    local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(data.length, 18);
    local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(nameBytes.length, 26);
    local.writeUInt16LE(0, 28);
    nameBytes.copy(local, 30);
    data.copy(local, 30 + nameBytes.length);
    localHeaders.push(local);

    const central = Buffer.alloc(46 + nameBytes.length);
    central.writeUInt32LE(0x02014b50, 0);
    central.writeUInt16LE(20, 4);
    central.writeUInt16LE(20, 6);
    central.writeUInt16LE(0, 8);
    central.writeUInt16LE(0, 10);
    central.writeUInt16LE(dosTime, 12);
    central.writeUInt16LE(dosDate, 14);
    central.writeUInt32LE(crc, 16);
    central.writeUInt32LE(data.length, 20);
    central.writeUInt32LE(data.length, 24);
    central.writeUInt16LE(nameBytes.length, 28);
    central.writeUInt16LE(0, 30);
    central.writeUInt16LE(0, 32);
    central.writeUInt16LE(0, 34);
    central.writeUInt16LE(0, 36);
    central.writeUInt32LE(0, 38);
    central.writeUInt32LE(offset, 42);
    nameBytes.copy(central, 46);
    centralHeaders.push(central);

    offset += local.length;
  }

  const centralSize = centralHeaders.reduce((s, h) => s + h.length, 0);
  const eocd = Buffer.alloc(22);
  eocd.writeUInt32LE(0x06054b50, 0);
  eocd.writeUInt16LE(0, 4);
  eocd.writeUInt16LE(0, 6);
  eocd.writeUInt16LE(files.length, 8);
  eocd.writeUInt16LE(files.length, 10);
  eocd.writeUInt32LE(centralSize, 12);
  eocd.writeUInt32LE(offset, 16);
  eocd.writeUInt16LE(0, 20);

  return Buffer.concat([...localHeaders, ...centralHeaders, eocd]);
}

const files = walkDir(distDir);
const zipData = createStoreZip(files);

// Full keccak256 via cast
const bundleHash = execSync(`cast keccak "0x${zipData.toString('hex')}"`, {
  encoding: 'utf8',
}).trim();

// Content hash (truncated base36) for display
const ALPHABET = '0123456789abcdefghijklmnopqrstuvwxyz';
function base36Encode(bytes) {
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

const truncated = Buffer.from(bundleHash.replace('0x', '').slice(0, 48), 'hex');
const contentHash = base36Encode(truncated);

// --- Check current on-chain state ---

let previousVersionKey = '0x0000000000000000000000000000000000000000000000000000000000000000';
const sealId = execSync(`cast keccak "$(cast --from-utf8 '${seal_url}')"`, { encoding: 'utf8' }).trim();

try {
  const result = execSync(
    `cast call ${REGISTRY} "getApp(address,string)(string,uint256,bytes32)" ${owner} "${seal_url}" --rpc-url ${rpcUrl}`,
    { encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'] }
  ).trim();
  // Parse the versionKey from the third return value
  const lines = result.split('\n');
  if (lines.length >= 3) {
    previousVersionKey = lines[2].trim();
    console.log(`Existing app found. Current versionKey: ${previousVersionKey}`);
  }
} catch {
  console.log('No existing app on-chain (first publish).');
}

// --- Summary ---

console.log('');
console.log('=== Publish Summary ===');
console.log(`  App name:     ${prettyName}`);
console.log(`  Seal URL:     ${seal_url}`);
console.log(`  Version:      1.0.0`);
console.log(`  Bundle hash:  ${bundleHash}`);
console.log(`  Content hash: ${contentHash}`);
console.log(`  Format:       1 (zip.br)`);
console.log(`  Sources:      ${JSON.stringify(bundleSources)}`);
console.log(`  Previous key: ${previousVersionKey}`);
console.log(`  Registry:     ${REGISTRY}`);
console.log(`  RPC:          ${rpcUrl}`);
console.log('');

// --- Estimate gas cost ---

const sourcesArg = JSON.stringify(bundleSources);
const castCallArgs =
  `"publish(string,string,string,bytes32,uint256,string[],bytes32)" ` +
  `"${seal_url}" "${prettyName}" "1.0.0" ` +
  `${bundleHash} 1 ` +
  `'${sourcesArg}' ` +
  `${previousVersionKey}`;

try {
  const gasEstimate = execSync(
    `cast estimate ${REGISTRY} ${castCallArgs} --from ${owner} --rpc-url ${rpcUrl}`,
    { encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'] }
  ).trim();
  const gasPrice = execSync(
    `cast gas-price --rpc-url ${rpcUrl}`,
    { encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'] }
  ).trim();
  const ethPrice = execSync(
    `cast to-unit ${BigInt(gasEstimate) * BigInt(gasPrice)} ether`,
    { encoding: 'utf8', stdio: ['pipe', 'pipe', 'pipe'] }
  ).trim();
  console.log(`  Estimated gas:  ${gasEstimate}`);
  console.log(`  Gas price:      ${(Number(gasPrice) / 1e9).toFixed(4)} gwei`);
  console.log(`  Estimated cost: ${ethPrice} ETH`);
  console.log('');
} catch (e) {
  console.log('  (could not estimate gas)');
  console.log('');
}

if (dryRun) {
  process.exit(0);
}

// --- Send transaction ---

console.log('Sending transaction...');

const output = execSync(
  `cast send ${REGISTRY} ${castCallArgs} ` +
  `--rpc-url ${rpcUrl} ` +
  `--private-key ${privateKey}`,
  { encoding: 'utf8' }
);

console.log(output);
console.log(`Published ${prettyName} v1.0.0`);
console.log(`  Bundle: ${contentHash}.zip.br`);
