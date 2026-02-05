#!/usr/bin/env node

/* How to run
    1. configure DB sync URL and start/end epochs
    2. `npm install`
    3. `node rewards_reference_from_dbsync.js`
*/

const fs = require("fs");
const path = require("path");
const { Client } = require("pg");

// ---------- config ----------
const DBSYNC_URL = "postgresql://username:password@hostname:5432/database_name";
const START_EPOCH = 525;
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
      encode(sa.hash_raw, 'hex') AS address,
      r.type, r.amount 
    FROM reward r
    JOIN pool_hash ph ON ph.id = r.pool_id
    JOIN stake_address sa ON sa.id = r.addr_id
    WHERE r.earned_epoch = $1
    ORDER BY sa.hash_raw;
`;

async function run() {
  const db = new Client({ connectionString: DBSYNC_URL });
  await db.connect();

  try {
    for (let epoch = START_EPOCH; epoch <= END_EPOCH; epoch++) {
      const { rows } = await db.query(QUERY, [epoch]);

      const file = path.join(__dirname, `rewards.mainnet.${epoch}.csv`);
      const out = fs.createWriteStream(file, { flags: "w" });

      out.write("spo,address,type,amount\n");
      for (const row of rows) {
        out.write(`${row.pool_id},${row.address},${row.type},${row.amount}\n`);
      }
      out.end();

      console.log(
        `Wrote ${file} (epoch ${epoch}, ${rows.length} reward entries)`
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
