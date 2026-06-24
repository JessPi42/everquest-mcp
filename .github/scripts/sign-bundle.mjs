import { createPrivateKey, sign } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { basename } from 'node:path';

const keyContent = process.env.TAURI_SIGNING_PRIVATE_KEY;
if (!keyContent) { console.error('TAURI_SIGNING_PRIVATE_KEY not set'); process.exit(1); }

// Parse minisign secret key: find the base64 payload line (skip comment lines)
const b64Line = keyContent.trim().split('\n').find(l => !l.startsWith('untrusted comment'));
if (!b64Line) { console.error('Cannot parse key — no base64 line found'); process.exit(1); }

const keyBytes = Buffer.from(b64Line.trim(), 'base64');
// Minisign secret key binary layout (158 bytes):
//   algo(2) + kdf(2) + cksum(2) + kdf_salt(32) + ops(8) + mem(8) = 54 bytes header
//   keynum_sk: key_id(8) + ed25519_sk(64) + blake2b_chk(32) = 104 bytes
if (keyBytes.length < 158) {
  console.error('Key too short:', keyBytes.length, '(expected 158)');
  process.exit(1);
}
const keyId = keyBytes.slice(54, 62);  // 8 bytes
const seed  = keyBytes.slice(62, 94);  // 32-byte Ed25519 seed (first half of libsodium sk)

// Build a PKCS8 DER-encoded Ed25519 private key from the raw seed.
// The 16-byte header is the fixed ASN.1 structure for Ed25519 PKCS8.
const pkcs8Header = Buffer.from('302e020100300506032b657004220420', 'hex');
const pkcs8Der = Buffer.concat([pkcs8Header, seed]);
const privKey = createPrivateKey({ key: pkcs8Der, format: 'der', type: 'pkcs8' });

const zipPath = process.argv[2];
if (!zipPath) { console.error('Usage: sign-bundle.mjs <path-to-zip>'); process.exit(1); }
const zipData = readFileSync(zipPath);

// Ed25519 signature of the raw zip bytes
const rawSig = sign(null, zipData, privKey);
console.log('rawSig length:', rawSig.length, '(expected 64)');

// minisign SigStruct: algo(2="Ed") + key_id(8) + rawSig(64) = 74 bytes total
const sigStruct = Buffer.concat([Buffer.from([0x45, 0x64]), keyId, rawSig]);
const sigB64 = sigStruct.toString('base64');

// Trusted comment: plain string, NOT the output-file line prefix
const ts = Math.floor(Date.now() / 1000);
const filename = basename(zipPath);
const trustedContent = 'timestamp:' + ts + '\tfile:' + filename;

// Global sig: signs (rawSig || trustedContent) to bind the comment to the signature
const globalInput = Buffer.concat([rawSig, Buffer.from(trustedContent)]);
const globalSig = sign(null, globalInput, privKey);

// Write standard minisign .sig format (4 lines + trailing newline)
const sigFile = [
  'untrusted comment: signature from minisign secret key',
  sigB64,
  'trusted comment: ' + trustedContent,
  globalSig.toString('base64'),
  '',
].join('\n');

writeFileSync(zipPath + '.sig', sigFile);
console.log('Signed successfully:', zipPath);
// Print the SigStruct b64 so the workflow can capture it for latest.json
process.stdout.write('::set-sig::' + sigB64 + '\n');
