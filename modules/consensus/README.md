# Consensus module

The consensus module takes offered blocks from an upstream source (`peer-network-interface`, bootstrappers, etc) and
decides which chain (fork) to favour, passing on blocks on the favoured chain to other validation and storage modules
downstream.

## Configuration

The following is the default configuration, these are the default topics so they can be left out if they are OK. The
validators _must_ be configured, if empty, no validation is performed.

```toml
[module.consensus]

# Message topics
blocks-available-topic = "cardano.block.available"
blocks-proposed-topic = "cardano.block.proposed"

# Block flow mode is set globally in [global.startup]:
# block-flow-mode = "consensus"  # Options: "direct" | "consensus"

# Validation result topics
validators = [
           "cardano.validation.vrf",
           "cardano.validation.kes",
           "cardano.validation.utxo"
           ...
]

```

## Validation

The consensus module passes on blocks it receives from upstream (e.g. `peer-network-interface`)and sends them out as
'proposed' blocks for validation, storage and further processing. It then listens on all of the `validators` topics for
BlockValidation messages, which give a Go / NoGo for the block. Single NoGo is enough to mark the block as invalid.

In the `direct` mode, the consensus module simply passes on all blocks it receives and logs the validation failure.  
When in `consensus` mode, the module is reacting on the validation results and decides which chain to favour and emits
rollback messages if necessary. It is downstream subscribers' responsibility to deal with the effects of the rollbacks.

## Messages

The consensus module subscribes for `cardano.block.available`, `cardano.block.offered` and `cardano.block.rescinded`. It
sends out `cardano.block.wanted` and `cardano.block.rejected` messages to fetch or mark offered blocks as rejected. The
module uses the consensus rules (Ourobouros Praos `maxvalid` rule) to decide which of multiple chains (forks) to favour,
and sends candidate blocks on `cardano.block.proposed` to request validation and storage.

Both input and output are `RawBlockMessage`.
