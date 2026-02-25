import "dotenv/config";
import axios from "axios";
import { Client } from "pg";
import readline from "readline";
import { bech32 } from "bech32";

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

type DRepKind = "key" | "script";

function drepRawToBech32(raw: Buffer, kind: DRepKind = "key"): string {
    const words = bech32.toWords(raw);
    const hrp = kind === "script" ? "drep_script" : "drep";
    return bech32.encode(hrp, words);
}

async function queryDbSync(client: Client, epoch: number) {
    const { rows } = await client.query(
        `
            SELECT
                dh.raw AS drep_raw,
                dh.has_script,
                dd.amount::bigint AS stake
            FROM drep_distr dd
            JOIN drep_hash dh ON dh.id = dd.hash_id
            WHERE dd.epoch_no = $1
              AND dd.amount IS NOT NULL
              AND dd.amount > 0
              AND dh.raw IS NOT NULL
              AND octet_length(dh.raw) = 28
            ORDER BY dd.amount DESC;
        `,
        [epoch]
    );

    return rows;
}

async function queryAcropolis(epoch: number) {
    const { data, status } = await axios.get(
        `${ACROPOLIS_URL}/drdd?epoch=${epoch}`,
        { validateStatus: () => true }
    );

    if (status !== 200) {
        throw new Error(`HTTP ${status}`);
    }

    if (
        typeof data !== "object" ||
        !data ||
        typeof (data as any).dreps !== "object"
    ) {
        throw new Error("Invalid DRDD response");
    }

    const dreps: { drep_id: string; stake: bigint }[] = [];

    for (const [drep_id, amount] of Object.entries((data as any).dreps)) {
        if (typeof amount !== "number" && typeof amount !== "string" && typeof amount !== "bigint") {
            throw new Error(`Invalid DRDD amount for ${drep_id}`);
        }

        dreps.push({
            drep_id,
            stake: BigInt(amount),
        });
    }


    return dreps;
}


async function validateEpoch(db: Client, epoch: number) {
    const dbDReps = (await queryDbSync(db, epoch)).map((r) => ({
        drep_id: drepRawToBech32(
            r.drep_raw,
            r.has_script ? "script" : "key"
        ),
        stake: BigInt(r.stake),
    }));
    if (!dbDReps.length) {
        console.log(`No db-sync data found for epoch ${epoch}. Exiting.`);
        return false;
    }

    const drddDReps = await queryAcropolis(epoch);
    const drddMap = new Map(drddDReps.map((p) => [p.drep_id, p.stake]));
    const dbMap = new Map(dbDReps.map((p) => [p.drep_id, BigInt(p.stake)]));

    const dbTotal = dbDReps.reduce((acc, p) => acc + BigInt(p.stake), 0n);
    const apiTotal = drddDReps.reduce((acc, p) => acc + p.stake, 0n);

    if (dbTotal !== apiTotal) {
        console.log(
            `❌ Total active stake mismatch for epoch ${epoch}:\n   DB: ${dbTotal}\n   SPDD: ${apiTotal}`
        );
    }

    let matched = 0;
    const missing: string[] = [];
    const extra: string[] = [];
    const mismatched: { id: string; db: bigint; spdd: bigint }[] = [];

    // Missing or mismatched DReps in Acropolis DRDD
    for (const { drep_id, stake } of dbDReps) {
        const expected = BigInt(stake);
        const found = drddMap.get(drep_id);
        if (found === undefined) missing.push(drep_id);
        else if (found !== expected) mismatched.push({ id: drep_id, db: expected, spdd: found });
        else matched++;
    }

    // Extra DReps in Acropolis DRDD which do not exist in DB Sync
    for (const { drep_id } of drddDReps) {
        if (!dbMap.has(drep_id)) extra.push(drep_id);
    }

    const total = matched + mismatched.length + missing.length;
    console.log(
        `Epoch ${epoch}: ✅ ${matched} match, ⚠️ ${mismatched.length} mismatch, ❌ ${missing.length} missing, 🌀 ${extra.length} extra (total ${total})`
    );

    if (missing.length || mismatched.length || extra.length) {
        if (missing.length) {
            console.log(`   Missing DReps:`);
            for (const id of missing) {
                const dbStake = dbMap.get(id);
                console.log(`   - ${id} (db: ${dbStake})`);
            }
        }
        if (extra.length) {
            console.log(`   Extra DReps (in DRDD only):`);
            for (const id of extra) {
                const drddStake = drddMap.get(id);
                console.log(`   - ${id} (drdd: ${drddStake})`);
            }
        }
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

            console.log(`   Mismatched DRep with smallest difference:`);
            console.log(
                `   - ${smallest.id} (db: ${smallest.db}, drdd: ${smallest.spdd}, diff: ${diff})`
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

            console.log(`   Mismatched DRep with smallest total stake:`);
            console.log(
                `   - ${smallestTotal.id} (db: ${smallestTotal.db}, drdd: ${smallestTotal.spdd}, diff: ${diff2})`
            );
        }
        if (process.env.CI === undefined) await pause();
    }

    return true;
}

async function run() {
    const db = new Client({ connectionString: DBSYNC_URL });
    await db.connect();

    console.log("Validating Acropolis DRDD vs DB Sync...\n");

    for (let epoch = START_EPOCH; ; epoch++) {
        try {
            const keepGoing = await validateEpoch(db, epoch);
            if (!keepGoing) break;
        } catch (err: any) {
            if (err.message.startsWith("HTTP")) {
                if (epoch == START_EPOCH) {
                    console.log(`store-drdd=false or sync has not reached epoch ${START_EPOCH}.`);
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
