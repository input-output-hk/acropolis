#!/usr/bin/env node

/* How to run
    1. configure DB sync URL and start/end epochs
    2. `npm install`
    3. `node spdd_reference_from_dbsync.js`
*/

const fs = require("fs");
const path = require("path");
const { Client } = require("pg");

// ---------- config ----------
const DBSYNC_URL = "postgresql://username:password@hostname:5432/database_name";
const START_EPOCH = 515;
const END_EPOCH = 600;

if ((!DBSYNC_URL) || (DBSYNC_URL == "postgresql://username:password@hostname:5432/database_name")) {
  throw new Error("Missing MAINNET_DBSYNC_URL");
}
if (Number.isNaN(START_EPOCH) || Number.isNaN(END_EPOCH)) {
  throw new Error("SPDD_START_EPOCH and SPDD_END_EPOCH must be numbers");
}
if (START_EPOCH > END_EPOCH) {
  throw new Error("START_EPOCH must be <= END_EPOCH");
}

const QUERY = `
    SELECT
    encode(ph.hash_raw, 'hex') AS pool_id,
    SUM(es.amount)::bigint     AS amount
    FROM epoch_stake es
    JOIN pool_hash ph ON es.pool_id = ph.id
    WHERE es.epoch_no = $1 + 2
    GROUP BY ph.hash_raw
    HAVING SUM(es.amount) > 0
    ORDER BY ph.hash_raw
`;

async function run() {
  const db = new Client({ connectionString: DBSYNC_URL });
  await db.connect();

  try {
    for (let epoch = START_EPOCH; epoch <= END_EPOCH; epoch++) {
      const { rows } = await db.query(QUERY, [epoch]);

      const file = path.join(__dirname, `spdd.mainnet.${epoch}.csv`);
      const out = fs.createWriteStream(file, { flags: "w" });

      out.write("pool_id,amount\n");
      for (const row of rows) {
        out.write(`${row.pool_id},${row.amount}\n`);
      }
      out.end();

      console.log(
        `Wrote ${file} (epoch ${epoch}, ${rows.length} pools)`
      );
    }
  } finally {
    await db.end();
  }

  console.log("\nFinished.");
}

run().catch((err) => {
  console.error(err);
  process.exit(1);
});
