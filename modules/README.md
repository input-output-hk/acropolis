# Microservice modules for Acropolis

This directory holds microservice modules for a Caryatid framework which
compose the Acropolis Architecture

* [Upstream Chain Fetcher](upstream_chain_fetcher) -
  implementation of the Node-to-Node (N2N) client-side (initiator)
  protocol, allowing chain synchronisation and block fetching
* [Mithril Snapshot Fetcher](mithril_snapshot_fetcher) -
  Fetches a chain snapshot from Mithril and replays all the blocks in it
* [Genesis Bootstrapper](genesis_bootstrapper) - reads the Genesis
  file for a chain and generates initial UTXOs
* [Block Unpacker](block_unpacker) - unpacks received blocks
  into individual transactions
* [Tx Unpacker](tx_unpacker) - parses transactions and generates UTXO
  changes
* [UTXO State](utxo_state) - watches UTXO changes and maintains a basic in-memory UTXO state
* [SPO State](spo_state) - matches SPO registrations and retirements
* [Epoch Activity Counter](epoch_activity_couinter) - counts fees and block production for rewards
* [Reward State](reward_state) - calculates block rewards

## How to add a new module

To add a new module you're probably best off copying an existing one to start with.

If you want to start from scratch, here's the boilerplate for an empty module:

```rust
// my_module.rs
use caryatid_sdk::{Context, Module, module, MessageBusExt};
use acropolis_common::messages::Message;
use std::sync::Arc;
use anyhow::Result;
use config::Config;

/// My module
#[module(
    message_type(Message),
    name = "my-module",
    description = "One-liner on what it does"
)]
pub struct MyModule;

impl MyModule
{
    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Use config to get topics to subscribe to
        // Use context.message_bus to subscribe to them
        // Keep all state in variables in this function, capture in subscription closures
        Ok(())
    }
}
```

The `Cargo.toml` will look like this:

```toml
[package]
name = "acropolis_module_my_module"
version = "0.1.0"
edition = "2021"
authors = ["Your name <you@example.com>"]
description = "Simple description"
license = "Apache-2.0"

[dependencies]
caryatid_sdk = "0.4.0"
acropolis_common = { path = "../../common" }
anyhow = "1.0"
async-trait = "0.1"
tokio = { version = "1", features = ["full"] }
config = "0.15.11"
tracing = "0.1.40"

[lib]
path = "src/my_module.rs"
```

### Registering the module

To register the module into a process - for example [Omnibus](../processes/omnibus) you need
to call `MyModule::register()` in the process `main()`:

```rust
use acropolis_module_my_module::MyModule;

// in main()...
    MyModule::register(&mut process);
```

You also need to mention the module in (e.g.) `omnibus.toml` to get it created, even if all
the configuration is defaulted:

```toml
[module.my-module]
# Other configuration can go here
```
