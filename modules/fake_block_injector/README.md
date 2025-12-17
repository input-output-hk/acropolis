# Fake block injector module

The fake block injector is designed to replace the
(Peer Network Interface)[../peer_network_interface] and inject fake blocks into the
system for testing.

It waits for a snapshot complete event indicating the
last block fetched, sent by the (Mithril Snapshot Fetcher)[../mithril_snapshot_fetcher] or
the (Snapshot Bootstrapper)[../snapshot_bootstrapper).  It then reads one or more CBOR-encoded
block files from disk and sends them as if it was from the network.

## Configuration

The following is the default configuration - if the defaults are OK,
everything except the section header can be left out.

```toml
[module.fake-block-injector]

# File pattern to read (in sorted order)
block-files = "fake-blocks/block.*.csv"

# Message topics
completion-topic = "cardano.shapshot.complete"

```

## Messages

The injector waits for a `cardano.snapshot.complete` message before starting.

For each block read from disk, the block body as a BlockBodyMessage on topic
`cardano.block.body`.

All blocks will be tagged as volatile.
