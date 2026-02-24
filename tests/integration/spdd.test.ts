import "dotenv/config";
import axios from "axios";
import { Client } from "pg";
import readline from "readline";

const ACROPOLIS_URL = process.env.ACROPOLIS_REST_URL!;
const DBSYNC_URL = process.env.MAINNET_DBSYNC_URL!;
if (!ACROPOLIS_URL || !DBSYNC_URL) {
    throw new Error("Missing required environment variables ACROPOLIS_REST_URL or MAINNET_DBSYNC_URL");
}

const START_EPOCH = Number(process.env.SPDD_VALIDATION_START_EPOCH);
if (Number.isNaN(START_EPOCH)) {
    throw new Error("SPDD_VALIDATION_START_EPOCH must be a number");
}

async function pause() {
    const rl = readline.createInterface({ input: process.stdin, output: process.stdout });

    return new Promise<void>((resolve) => {
        rl.on("SIGINT", () => {
            rl.close();
            process.exit(0);
        });

        rl.question("Press Enter to continue or Ctrl+C to stop...", () => {
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
    const { data, status } = await axios.get(`${ACROPOLIS_URL}/spdd?epoch=${epoch}`, {
        validateStatus: () => true,
    });

    if (status !== 200) {
        throw new Error(`HTTP ${status}`);
    }

    if (typeof data !== "object" || Array.isArray(data) || !data)
        throw new Error("Invalid SPDD response");

    const pools: { pool_id: string; stake: bigint }[] = [];
    for (const [pool_id, info] of Object.entries(data)) {
        if (!info || typeof info !== "object" || !("active" in info)) continue;
        pools.push({ pool_id, stake: BigInt((info as any).active) });
    }
    return pools;
}

async function validateEpoch(db: Client, epoch: number) {
    const dbPools = await queryDbSync(db, epoch);
    if (!dbPools.length) {
        console.log(`No db-sync data found for epoch ${epoch}. Exiting.`);
        return false;
    }

    const spddPools = await queryAcropolis(epoch);
    const spddMap = new Map(spddPools.map((p) => [p.pool_id, p.stake]));
    const dbMap = new Map(dbPools.map((p) => [p.pool_id, BigInt(p.stake)]));

    const dbTotal = dbPools.reduce((acc, p) => acc + BigInt(p.stake), 0n);
    const apiTotal = spddPools.reduce((acc, p) => acc + p.stake, 0n);

    if (dbTotal !== apiTotal) {
        console.log(
            `❌ Total active stake mismatch for epoch ${epoch}:\n   DB: ${dbTotal}\n   SPDD: ${apiTotal}`
        );
    }

    let matched = 0;
    const missing: string[] = [];
    const extra: string[] = [];
    const mismatched: { id: string; db: bigint; spdd: bigint }[] = [];

    // Missing or mismatched pools in Acropolis SPDD
    for (const { pool_id, stake } of dbPools) {
        const expected = BigInt(stake);
        const found = spddMap.get(pool_id);
        if (found === undefined) missing.push(pool_id);
        else if (found !== expected) mismatched.push({ id: pool_id, db: expected, spdd: found });
        else matched++;
    }

    // Extra pools in Acropolis SPDD which do not exist in DB Sync
    for (const { pool_id } of spddPools) {
        if (!dbMap.has(pool_id)) extra.push(pool_id);
    }

    const total = matched + mismatched.length + missing.length;
    console.log(
        `Epoch ${epoch}: ✅ ${matched} match, ⚠️ ${mismatched.length} mismatch, ❌ ${missing.length} missing, 🌀 ${extra.length} extra (total ${total})`
    );

    if (missing.length || mismatched.length || extra.length) {
        if (missing.length) console.log(`   Missing pools: ${missing.join(", ")}`);
        if (extra.length) console.log(`   Extra pools (in SPDD only): ${extra.join(", ")}`);
        if (mismatched.length) {
            const smallest = mismatched.reduce((best, curr) => {
                const bestDiff = best.db >= best.spdd ? best.db - best.spdd : best.spdd - best.db;
                const currDiff = curr.db >= curr.spdd ? curr.db - curr.spdd : curr.spdd - curr.db;
                return currDiff < bestDiff ? curr : best;
            });

            const diff =
                smallest.db >= smallest.spdd
                    ? smallest.db - smallest.spdd
                    : smallest.spdd - smallest.db;

            console.log(`   Mismatched pool with smallest difference:`);
            console.log(
                `   - ${smallest.id} (db: ${smallest.db}, spdd: ${smallest.spdd}, diff: ${diff})`
            );

            const smallestTotal = mismatched.reduce((best, curr) => {
                const bestTotal = best.db + best.spdd;
                const currTotal = curr.db + curr.spdd;
                return currTotal < bestTotal ? curr : best;
            });

            const diff2 =
                smallestTotal.db >= smallestTotal.spdd
                    ? smallestTotal.db - smallestTotal.spdd
                    : smallestTotal.spdd - smallestTotal.db;

            console.log(`   Mismatched pool with smallest total stake:`);
            console.log(
                `   - ${smallestTotal.id} (db: ${smallestTotal.db}, spdd: ${smallestTotal.spdd}, diff: ${diff2})`
            );
        }
        if (process.env.CI === undefined) await pause();
    }

    return true;
}

async function run() {
    const db = new Client({ connectionString: DBSYNC_URL });
    await db.connect();

    console.log("Validating Acropolis SPDD vs DB Sync...\n");

    for (let epoch = START_EPOCH; ; epoch++) {
        try {
            const keepGoing = await validateEpoch(db, epoch);
            if (!keepGoing) break;
        } catch (err: any) {
            if (err.message.startsWith("HTTP")) {
                if (epoch == START_EPOCH) {
                    console.log(`store-spdd=false or sync has not reached epoch ${START_EPOCH}.`);
                } else {
                    console.log(`Reached end of available epochs (${err.message}).`);
                }
                break;
            }
            console.error(`Stopping at epoch ${epoch}: ${err.message}`);
            break;
        }
    }

    await db.end();
    console.log("\nFinished.");
}

run().catch((err) => {
    console.error(err.message || err);
    process.exit(1);
});
