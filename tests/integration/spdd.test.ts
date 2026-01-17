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

    if (missing.length || mismatched.length || extra.length || dbTotal !== apiTotal) {
        // Calculate aggregate statistics
        let totalDiff = 0n;
        let spddHigherCount = 0;
        let dbHigherCount = 0;
        let spddHigherTotal = 0n;
        let dbHigherTotal = 0n;

        for (const m of mismatched) {
            const diff = m.spdd - m.db;
            totalDiff += diff;
            if (diff > 0n) {
                spddHigherCount++;
                spddHigherTotal += diff;
            } else {
                dbHigherCount++;
                dbHigherTotal += -diff;
            }
        }

        console.log(`\n   === Diagnostic Summary ===`);
        console.log(`   Total pools in DB: ${dbPools.length}, in SPDD: ${spddPools.length}`);
        console.log(`   Total stake difference (SPDD - DB): ${apiTotal - dbTotal} lovelace`);

        if (mismatched.length) {
            console.log(`\n   Mismatch breakdown:`);
            console.log(`   - SPDD higher than DB: ${spddHigherCount} pools, total +${spddHigherTotal} lovelace`);
            console.log(`   - DB higher than SPDD: ${dbHigherCount} pools, total -${dbHigherTotal} lovelace`);
            console.log(`   - Net difference from mismatches: ${totalDiff} lovelace`);

            // Sort mismatched by absolute difference descending
            const sortedByDiff = [...mismatched].sort((a, b) => {
                const aDiff = a.spdd >= a.db ? a.spdd - a.db : a.db - a.spdd;
                const bDiff = b.spdd >= b.db ? b.spdd - b.db : b.db - b.spdd;
                return Number(bDiff - aDiff);
            });

            console.log(`\n   Top 5 pools with largest differences:`);
            for (const m of sortedByDiff.slice(0, 5)) {
                const diff = m.spdd - m.db;
                const sign = diff >= 0n ? "+" : "";
                console.log(`   - ${m.id}: DB=${m.db}, SPDD=${m.spdd}, diff=${sign}${diff}`);
            }

            console.log(`\n   Top 5 pools with smallest differences:`);
            for (const m of sortedByDiff.slice(-5).reverse()) {
                const diff = m.spdd - m.db;
                const sign = diff >= 0n ? "+" : "";
                console.log(`   - ${m.id}: DB=${m.db}, SPDD=${m.spdd}, diff=${sign}${diff}`);
            }

            // Check if all differences are in the same direction
            if (spddHigherCount === mismatched.length) {
                console.log(`\n   ⚠️  ALL mismatches have SPDD > DB (possible rounding UP issue)`);
            } else if (dbHigherCount === mismatched.length) {
                console.log(`\n   ⚠️  ALL mismatches have DB > SPDD (possible rounding DOWN issue)`);
            }
        }

        if (missing.length) {
            console.log(`\n   Missing pools (in DB but not SPDD): ${missing.slice(0, 10).join(", ")}${missing.length > 10 ? ` ... and ${missing.length - 10} more` : ""}`);
        }
        if (extra.length) {
            console.log(`\n   Extra pools (in SPDD but not DB): ${extra.slice(0, 10).join(", ")}${extra.length > 10 ? ` ... and ${extra.length - 10} more` : ""}`);
        }

        console.log(`\n   === End Diagnostic ===\n`);
        await pause();
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
