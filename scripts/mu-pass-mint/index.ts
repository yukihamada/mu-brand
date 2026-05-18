/**
 * MU Pass on-chain mint tool — self-contained CLI.
 *
 * Mints compressed NFTs (Bubblegum) for each MU Pass edition to the
 * Treasury wallet (we hold them custodially; holders can request transfer
 * to their own wallet later).
 *
 * Costs: ~0.005 SOL one-time tree creation + ~0.000005 SOL per mint.
 *
 * Usage:
 *   MU_ADMIN_TOKEN=… bun run scripts/mu-pass-mint/index.ts test 1
 *   MU_ADMIN_TOKEN=… bun run scripts/mu-pass-mint/index.ts batch 1-20
 *   MU_ADMIN_TOKEN=… bun run scripts/mu-pass-mint/index.ts tree-info
 */
import {
  createUmi,
} from "@metaplex-foundation/umi-bundle-defaults";
import {
  generateSigner,
  keypairIdentity,
  publicKey,
  some,
  none,
} from "@metaplex-foundation/umi";
import {
  mplBubblegum,
  createTree,
  mintV1,
  TokenStandard,
  fetchMerkleTree,
} from "@metaplex-foundation/mpl-bubblegum";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { base58 } from "@metaplex-foundation/umi/serializers";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const b58 = (sig: Uint8Array) => base58.deserialize(sig)[0];

// ── config ──────────────────────────────────────────────────────────
const RPC_URL = process.env.SOLANA_RPC || "https://api.mainnet-beta.solana.com";
const KEYPAIR_PATH =
  process.env.KEYPAIR_PATH || `${process.env.HOME}/.config/solana/enai-treasury.json`;
const PASS_HOST = process.env.PASS_HOST || "https://wearmu.com";
const ADMIN_TOKEN = process.env.MU_ADMIN_TOKEN || "";
const TREE_FILE = path.join(__dirname, ".tree.json");
// Bubblegum supports only specific (maxDepth, maxBufferSize) combos.
// (5, 8) is the smallest valid one above the demo (3, 8) — gives 32 leaves
// for ~0.0028 SOL. Enough for the 20-pass genesis lot.
const TREE_DEPTH = 5;
const TREE_BUFFER = 8;

// ── helpers ─────────────────────────────────────────────────────────
function loadUmi() {
  const umi = createUmi(RPC_URL).use(mplBubblegum());
  const raw = fs.readFileSync(KEYPAIR_PATH, "utf8");
  const secret = new Uint8Array(JSON.parse(raw));
  const kp = umi.eddsa.createKeypairFromSecretKey(secret);
  umi.use(keypairIdentity(kp));
  console.log(`◯ payer = ${kp.publicKey}`);
  return umi;
}

async function ensureTree(umi: ReturnType<typeof loadUmi>) {
  if (fs.existsSync(TREE_FILE)) {
    const j = JSON.parse(fs.readFileSync(TREE_FILE, "utf8"));
    console.log(`◯ tree (reused) = ${j.publicKey}`);
    return publicKey(j.publicKey);
  }
  console.log("◯ creating new Merkle tree…");
  const merkleTreeSigner = generateSigner(umi);
  const builder = await createTree(umi, {
    merkleTree: merkleTreeSigner,
    maxDepth: TREE_DEPTH,
    maxBufferSize: TREE_BUFFER,
  });
  const sig = await builder.sendAndConfirm(umi);
  const txSig = b58(sig.signature);
  console.log(`✓ tree created: ${merkleTreeSigner.publicKey}`);
  console.log(`  tx: ${txSig}`);
  fs.writeFileSync(
    TREE_FILE,
    JSON.stringify({ publicKey: merkleTreeSigner.publicKey.toString(), createdTx: txSig }, null, 2),
  );
  return merkleTreeSigner.publicKey;
}

async function fetchPass(edition: number) {
  // We don't have a public per-edition endpoint, so use the admin-token
  // protected list (added in the same PR).
  const r = await fetch(
    `${PASS_HOST}/api/admin/pass/list?admin_token=${encodeURIComponent(ADMIN_TOKEN)}`,
  );
  if (!r.ok) throw new Error(`list failed: HTTP ${r.status}`);
  const data: any = await r.json();
  const row = data.passes?.find((p: any) => p.edition === edition);
  if (!row) throw new Error(`edition ${edition} not found`);
  return row;
}

async function recordMint(edition: number, mintAsset: string, mintTx: string) {
  const r = await fetch(`${PASS_HOST}/api/admin/pass/record_mint`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ admin_token: ADMIN_TOKEN, edition, mint_asset: mintAsset, mint_tx: mintTx }),
  });
  if (!r.ok) {
    console.error(`  ⚠ record_mint failed (HTTP ${r.status}) — mint is on-chain but DB not updated`);
  }
  return r.ok;
}

async function mintOne(umi: ReturnType<typeof loadUmi>, treeKey: any, edition: number) {
  const row = await fetchPass(edition);
  if (row.mint_status === "minted") {
    console.log(`  ─ #${String(edition).padStart(3, "0")} already minted (${row.mint_asset?.slice(0, 12)}…)`);
    return;
  }

  const edPad = String(edition).padStart(3, "0");
  const metadataUri = `${PASS_HOST}/api/pass/metadata/${edPad}`;

  console.log(`  ◯ minting #${edPad} (${row.email}) …`);
  // Mint cNFT to the Treasury wallet; we hold custodially.
  const treasury = umi.identity.publicKey;
  const builder = await mintV1(umi, {
    leafOwner: treasury,
    merkleTree: treeKey,
    metadata: {
      name: `MU Pass #${edPad}`,
      uri: metadataUri,
      sellerFeeBasisPoints: 0,
      collection: none(),
      creators: [{ address: treasury, verified: true, share: 100 }],
      tokenStandard: some(TokenStandard.NonFungible),
    },
  });
  const sig = await builder.sendAndConfirm(umi);
  const txSig = b58(sig.signature);
  // For cNFTs the "asset id" is derived from tree+leaf index; we'll write
  // the tx for now and let the DAS index pick up the asset id later.
  console.log(`  ✓ #${edPad} minted — tx: ${txSig}`);
  await recordMint(edition, "", txSig);
}

// ── main ────────────────────────────────────────────────────────────
async function main() {
  if (!ADMIN_TOKEN) {
    console.error("✗ MU_ADMIN_TOKEN env var required");
    process.exit(1);
  }
  const [, , cmd, arg] = process.argv;
  const umi = loadUmi();

  if (cmd === "tree-info") {
    if (!fs.existsSync(TREE_FILE)) {
      console.log("no tree yet — run a mint to create");
      return;
    }
    const j = JSON.parse(fs.readFileSync(TREE_FILE, "utf8"));
    console.log(JSON.stringify(j, null, 2));
    try {
      const tree = await fetchMerkleTree(umi, publicKey(j.publicKey));
      console.log(`leaves filled: ${tree.tree.activeIndex} / ${1 << TREE_DEPTH}`);
    } catch (e: any) {
      console.error(`fetch failed: ${e.message}`);
    }
    return;
  }

  const treeKey = await ensureTree(umi);

  if (cmd === "test") {
    const ed = parseInt(arg || "1", 10);
    await mintOne(umi, treeKey, ed);
    return;
  }

  if (cmd === "batch") {
    const [a, b] = (arg || "1-20").split("-").map((n) => parseInt(n, 10));
    for (let ed = a; ed <= b; ed++) {
      try { await mintOne(umi, treeKey, ed); }
      catch (e: any) { console.error(`  ✗ #${ed}: ${e.message}`); }
      await new Promise((r) => setTimeout(r, 500));
    }
    return;
  }

  console.error("usage: bun index.ts [test <edition> | batch <a-b> | tree-info]");
  process.exit(2);
}

main().catch((e) => { console.error(e); process.exit(1); });
