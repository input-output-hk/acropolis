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
        if (!info || typeof info !== "object" || !("live" in info)) continue;
        pools.push({ pool_id, stake: BigInt((info as any).live) });
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
            // Calculate differences with sign (positive = SPDD > DB, negative = DB > SPDD)
            const withDiffs = mismatched.map((m) => ({
                ...m,
                diff: m.spdd - m.db, // signed difference
                absDiff: m.spdd >= m.db ? m.spdd - m.db : m.db - m.spdd,
            }));

            // Sort by absolute difference
            const sortedByDiff = [...withDiffs].sort((a, b) =>
                a.absDiff > b.absDiff ? -1 : a.absDiff < b.absDiff ? 1 : 0
            );

            // Aggregate statistics
            const totalAbsDiff = withDiffs.reduce((sum, m) => sum + m.absDiff, 0n);
            const spddHigher = withDiffs.filter((m) => m.diff > 0n).length;
            const dbHigher = withDiffs.filter((m) => m.diff < 0n).length;
            const sumPositiveDiffs = withDiffs
                .filter((m) => m.diff > 0n)
                .reduce((sum, m) => sum + m.diff, 0n);
            const sumNegativeDiffs = withDiffs
                .filter((m) => m.diff < 0n)
                .reduce((sum, m) => sum + m.diff, 0n);

            // Total stake difference analysis
            const totalDiff = apiTotal - dbTotal;
            const diffPercent =
                dbTotal > 0n ? (Number(totalDiff) / Number(dbTotal)) * 100 : 0;

            console.log(`\n   📊 Mismatch Analysis:`);
            console.log(
                `   Total stake diff: ${totalDiff} (${diffPercent.toFixed(6)}% of DB total)`
            );
            console.log(
                `   Direction: ${spddHigher} pools SPDD>DB (+${sumPositiveDiffs}), ${dbHigher} pools DB>SPDD (${sumNegativeDiffs})`
            );
            console.log(`   Sum of absolute differences: ${totalAbsDiff}`);

            // Show top 5 largest differences
            console.log(`\n   🔝 Top 5 largest differences:`);
            for (const m of sortedByDiff.slice(0, 5)) {
                const sign = m.diff >= 0n ? "+" : "";
                const pct =
                    m.db > 0n ? ((Number(m.absDiff) / Number(m.db)) * 100).toFixed(4) : "N/A";
                console.log(
                    `   - ${m.id}: db=${m.db}, spdd=${m.spdd}, diff=${sign}${m.diff} (${pct}%)`
                );
            }

            // Show smallest difference (potential rounding issues)
            const smallest = sortedByDiff[sortedByDiff.length - 1];
            console.log(`\n   🔬 Smallest difference (potential rounding):`);
            console.log(
                `   - ${smallest.id}: db=${smallest.db}, spdd=${smallest.spdd}, diff=${smallest.diff}`
            );

            // Bucket analysis - group by difference magnitude
            const buckets = {
                tiny: withDiffs.filter((m) => m.absDiff <= 1000n).length, // <= 1000 lovelace
                small: withDiffs.filter((m) => m.absDiff > 1000n && m.absDiff <= 1000000n).length, // 1K-1M
                medium: withDiffs.filter((m) => m.absDiff > 1000000n && m.absDiff <= 1000000000n)
                    .length, // 1M-1B
                large: withDiffs.filter((m) => m.absDiff > 1000000000n).length, // > 1B (1000 ADA)
            };
            console.log(`\n   📈 Difference distribution:`);
            console.log(
                `   Tiny (≤1K): ${buckets.tiny}, Small (1K-1M): ${buckets.small}, Medium (1M-1B): ${buckets.medium}, Large (>1B): ${buckets.large}`
            );
        }
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
