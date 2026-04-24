# Testnet P2P Topology Apply Report

Applied topology from `testnet-peers.md` to all `pools/*/configs/topology.json` files and normalized P2P flags in all `pools/*/configs/config.json` files.

## Summary

- Pools processed: 30
- Topology files rewritten: 30
- Config files normalized: 30 (JSON re-serialized)
- `PeerSharing` value changes: 27 pools (`pool4`..`pool30`)

## Topology Applied

- Access point host: `host.docker.internal`
- Access point ports: `3000 + peer_id`
- 4 access points per pool from `testnet-peers.md`
- `advertise = true`
- `valency = 2`
- `publicRoots = []`
- `useLedgerAfterSlot = 0`

## Config Applied

- `EnableP2P = true` for all pools
- `PeerSharing = true` for all pools
- `PeerSharing` already `true` in pools: `pool1`, `pool2`, `pool3`

## Applied Peer Map

- pool1: [2, 30, 6, 11]
- pool2: [3, 1, 7, 12]
- pool3: [4, 2, 8, 13]
- pool4: [5, 3, 9, 14]
- pool5: [6, 4, 10, 15]
- pool6: [7, 5, 11, 16]
- pool7: [8, 6, 12, 17]
- pool8: [9, 7, 13, 18]
- pool9: [10, 8, 14, 19]
- pool10: [11, 9, 15, 20]
- pool11: [12, 10, 16, 21]
- pool12: [13, 11, 17, 22]
- pool13: [14, 12, 18, 23]
- pool14: [15, 13, 19, 24]
- pool15: [16, 14, 20, 25]
- pool16: [17, 15, 21, 26]
- pool17: [18, 16, 22, 27]
- pool18: [19, 17, 23, 28]
- pool19: [20, 18, 24, 29]
- pool20: [21, 19, 25, 30]
- pool21: [22, 20, 26, 1]
- pool22: [23, 21, 27, 2]
- pool23: [24, 22, 28, 3]
- pool24: [25, 23, 29, 4]
- pool25: [26, 24, 30, 5]
- pool26: [27, 25, 1, 6]
- pool27: [28, 26, 2, 7]
- pool28: [29, 27, 3, 8]
- pool29: [30, 28, 4, 9]
- pool30: [1, 29, 5, 10]
