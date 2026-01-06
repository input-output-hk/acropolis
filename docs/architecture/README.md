# The Architecture of Acropolis

These pages give a high-level overview of Acropolis, its philosophy and how the modules interact
to become a full Cardano node.

## Contents

- [Modularity](modularity.md) - Why and how Acropolis is split into loosely-coupled modules
- Building a node from the ground up:
  - [Simple Mithril UTXOs](system-simple-mithril-utxo.md) - The most basic UTXO follower from a Mithril snapshot
  - [Mithril and Sync UTXOs](system-simple-mithril-and-sync-utxo.md) - Adding a live network sync
  - [Basic ledger](system-bootstrap-and-sync-with-basic-ledger.md) - Adding basic (Shelley-era) ledger state
  - [Conway ledger](system-bootstrap-and-sync-with-conway.md) - Adding Conway / CIP-1694 governance
  - [BlockFrost API](system-ledger-with-api-and-history.md) - Adding a BlockFrost REST API and history storage
  - [Validation](system-ledger-validation.md) - Validating incoming blocks, Phase 1
