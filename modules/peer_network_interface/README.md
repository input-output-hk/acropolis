# Peer network interface module

The peer network interface module uses the ChainSync and BlockFetch protocols to fetch blocks from one of several upstream sources. It chooses one peer to treat as the "preferred" chain to follow, but will gracefully switch which peer it follows during network issues.

It can either run independently, from the origin or current tip, or
be triggered by a Mithril snapshot event (the default) where it starts from
where the snapshot left off, and follows the chain from there.

Rollbacks are handled by signalling in the block data - it is downstream
subscribers' responsibility to deal with the effects of this.

## Configuration

See [./config.default.toml](./config.default.toml) for the available configuration options and their default values.

## Messages

This module publishes "raw block messages" to the configured `block-topic`. Each message includes the raw bytes composing the header and body of a block. The module follows the head of one chain at any given time, though that chain may switch during runtime. If that chain reports a rollback (or if this module switches to a different chain), the next message it emits will be the new head of the chain and have the status `RolledBack`.
