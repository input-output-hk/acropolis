import axios from "axios";
import { Client } from "pg";
import "dotenv/config";
import readline from "readline";

const ACROPOLIS_URL = process.env.ACROPOLIS_REST_URL!;
const DBSYNC_URL = process.env.MAINNET_DBSYNC_URL!;

async function pause() {
    const rl = readline.createInterface({ input: process.stdin, output: process.stdout });
    await new Promise<void>((resolve) => {
        rl.question("⏸ Press Enter to continue, or Ctrl+C to stop...", () => {
            rl.close();
            resolve();
        });
    });
}

async function queryDbSync(client: Client, epoch: number) {
    const { rows } = await client.query(
        `
            SELECT ph.view AS pool_id, SUM(es.amount)::bigint AS stake
            FROM epoch_stake es
            JOIN pool_hash ph ON es.pool_id = ph.id
            WHERE es.epoch_no = ($1 + 2)
            GROUP BY ph.view
        `,
        [epoch]
    );
    return rows;
}

async function queryAcropolis(epoch: number) {
    const { data } = await axios.get(`${ACROPOLIS_URL}/spdd?epoch=${epoch}`);
    if (typeof data !== "object" || Array.isArray(data) || !data)
        throw new Error("Invalid SPDD response");

    const pools: { pool_id: string; stake: bigint }[] = [];
    for (const [pool_id, info] of Object.entries(data)) {
        if (!info || typeof info !== "object" || !("active" in info)) continue;
        pools.push({ pool_id, stake: BigInt((info as any).active) });
    }
    return pools;
}

async function run() {
    const db = new Client({ connectionString: DBSYNC_URL });
    await db.connect();

    let epoch = 208;
    console.log("Validating Acropolis SPDD vs DB Sync...\n");

    while (true) {
        try {
            const dbPools = await queryDbSync(db, epoch);
            if (!dbPools.length) {
                console.log(`No db-sync data found for epoch ${epoch}. Exiting.`);
                break;
            }

            const spddPools = await queryAcropolis(epoch);
            const spddMap = new Map(spddPools.map((p) => [p.pool_id, p.stake]));
            const dbMap = new Map(dbPools.map((p) => [p.pool_id, BigInt(p.stake)]));

            const dbTotal = dbPools.reduce((acc, p) => acc + BigInt(p.stake), 0n);
            const apiTotal = spddPools.reduce((acc, p) => acc + p.stake, 0n);

            if (dbTotal === apiTotal) {
                console.log(`Total active stake matches for epoch ${epoch}`);
            } else {
                console.log(
                    `Total active stake mismatch for epoch ${epoch}. (db: ${dbTotal}, spdd: ${apiTotal})`
                );
            }

            let matched = 0;
            let missing: string[] = [];
            let mismatched: { id: string; db: bigint; spdd: bigint }[] = [];
            let extra: string[] = [];

            // Pools in DB but missing or mismatched in SPDD
            for (const { pool_id, stake } of dbPools) {
                const expected = BigInt(stake);
                const found = spddMap.get(pool_id);
                if (found === undefined) missing.push(pool_id);
                else if (found !== expected)
                    mismatched.push({ id: pool_id, db: expected, spdd: found });
                else matched++;
            }

            // Pools in SPDD but missing in DB
            for (const { pool_id } of spddPools) {
                if (!dbMap.has(pool_id)) extra.push(pool_id);
            }

            const total = matched + mismatched.length + missing.length;
            console.log(
                `Epoch ${epoch}: ✅ ${matched} match, ⚠️ ${mismatched.length} mismatch, ❌ ${missing.length} missing, 🌀 ${extra.length} extra (total ${total})`
            );

            if (missing.length || mismatched.length || extra.length) {
                if (missing.length)
                    console.log(`   Missing pools: ${missing.join(", ")}`);
                if (extra.length)
                    console.log(`   Extra pools (in SPDD only): ${extra.join(", ")}`);
                if (mismatched.length) {
                    console.log(`   Mismatched pools:`);
                    for (const { id, db, spdd } of mismatched) {
                        console.log(`   - ${id} (db: ${db}, spdd: ${spdd})`);
                    }
                }
                await pause();
            }

            epoch++;
        } catch (err: any) {
            console.error(`💥 Stopping at epoch ${epoch}: ${err.message}`);
            break;
        }
    }

    await db.end();
    console.log("\n🧾 Finished.");
}


run().catch((err) => {
    console.error(err.message || err);
    process.exit(1);
});
