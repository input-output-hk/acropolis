# Acropolis 'omnibus' process

This process contains all the [modules](../../modules), communicating
over the in-memory message bus, which makes it very easy to test.  It
is not suggested this is the right way to package things for
production.

## How to run it

```shell
$ cd processes/omnibus
$ cargo run
```

## Known issues

### Too many open files when using modules using Fjall

There are two things that contribute towards this:
- The version of lsm-tree used by the Fjall package is known to leave file
  descriptors open for closed files.
- The default maximum open file limit per Fjall database is 512, so running
  multiple modules with Fjall stores can quickly reach the default limits of
  some systems

This issue can be worked around by increasing the open file limit. On Linux
systems this can be done with the ulimit command:
```
$ ulimit -n 4096
```

## Docker Compose

Build and run with preview config (default):

```shell
docker compose up --build
```

Run with mainnet config:

```shell
OMNIBUS_CONFIG=omnibus.toml docker compose up --build
```

Choose another omnibus config at runtime:

```shell
OMNIBUS_CONFIG=omnibus-preview.toml docker compose up --build
```

Notes:
- Relative paths in config resolve from `/app/processes/omnibus` in the container.
- Mithril downloads persist by default in the named volume `omnibus_downloads`.
- To persist Mithril downloads on the host instead, set `MITHRIL_DOWNLOADS_DIR` to a host path (for example `MITHRIL_DOWNLOADS_DIR=./modules/mithril_snapshot_fetcher/downloads`).
- Mithril snapshot downloads are network-namespaced by the module default path: `.../downloads/<network-name>`.
