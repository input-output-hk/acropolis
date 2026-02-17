import "dotenv/config";
import { Client } from "pg";
import { bech32 } from "bech32";
import fs from "fs";
import path from "path";

const DBSYNC_URL = process.env.DBSYNC_URL!;
if (!DBSYNC_URL) {
    throw new Error("Missing DBSYNC_URL");
}

const TARGET_EPOCH = Number(process.env.OPCERTS_EPOCH);
if (Number.isNaN(TARGET_EPOCH)) {
    throw new Error("OPCERTS_EPOCH must be a number");
}

function toPoolBech32(hex: string): string {
    const words = bech32.toWords(Buffer.from(hex, "hex"));
    return bech32.encode("pool", words);
}

async function fetchOpCerts(db: Client, epoch: number) {
    const { rows } = await db.query(
        `
        SELECT DISTINCT ON (ph.hash_raw)
            encode(ph.hash_raw::bytea, 'hex') AS pool_key_hash_hex,
            b.op_cert_counter
        FROM block b
        JOIN slot_leader sl ON sl.id = b.slot_leader_id
        JOIN pool_hash ph ON ph.id = sl.pool_hash_id
        WHERE b.epoch_no <= $1
        ORDER BY ph.hash_raw, b.block_no DESC;
        `,
        [epoch]
    );

    return rows.map((r) => ({
        pool_id: toPoolBech32(r.pool_key_hash_hex),
        op_cert_counter: Number(r.op_cert_counter ?? 0),
    }));
}

async function run() {
    const db = new Client({ connectionString: DBSYNC_URL });
    await db.connect();

    console.log(
        `Fetching latest op certs at or before epoch ${TARGET_EPOCH}...`
    );

    const data = await fetchOpCerts(db, TARGET_EPOCH);

    // Sort lexicographically by pool_id
    data.sort((a, b) => a.pool_id.localeCompare(b.pool_id));

    const outputPath = path.join(
        process.cwd(),
        `op_cert_counters.csv`
    );

    const header = `"pool_id","latest_op_cert_counter"\n`;

    const rows = data
        .map((r) => `"${r.pool_id}","${r.op_cert_counter}"`)
        .join("\n");

    fs.writeFileSync(outputPath, header + rows + "\n");

    console.log(`Wrote ${data.length} pools to ${outputPath}`);

    await db.end();
}

run().catch((err) => {
    console.error(err.message || err);
    process.exit(1);
});
