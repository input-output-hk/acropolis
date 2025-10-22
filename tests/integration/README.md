# Running the SPDD validation test
1. Install dependencies
```bash
npm install
```

2. Set environment variables
Create a `.env` file in the `tests/integration/` with the following values:
```env
MAINNET_DBSYNC_URL=postgres://user:password@host:port/dbname
ACROPOLIS_REST_URL=https://your-acropolis-endpoint
SPDD_VALIDATION_START_EPOCH=208
```
If you do not currently operate a mainnet DB Sync server, Demeter provides free access to an instance

3. Enable SPDD storage in `omnibus.toml`
```toml
[module.spdd-state]
store-spdd = true
```

4. Start Omnibus process and wait for sync
```bash
cd ~/acropolis/processes/omnibus
cargo run --release --bin acropolis_process_omnibus
```

5. Run the validator
```bash
npm run test:spdd
```

6. Observe output
This validator will:
* Compare Acropolis SPDD data with DB Sync epoch stake data
* Display total stake and per-pool differences 
* Pause for review when mismatches are detected
* Stop automatically when Acropolis stops returning data
