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

Build and run preview:

```shell
docker compose up --build omnibus-preview
```

Build and run mainnet:

```shell
docker compose up --build omnibus-mainnet
```

Build and run preview bootstrap mode:

```shell
docker compose up --build omnibus-bootstrap-preview
```

Build and run mainnet bootstrap mode:

```shell
docker compose up --build omnibus-bootstrap-mainnet
```

Default host ports:
- `omnibus-preview`: `4340` (REST), `4341` (MCP)
- `omnibus-mainnet`: `5340` (REST), `5341` (MCP)
- `omnibus-bootstrap-preview`: `6340` (REST), `6341` (MCP)
- `omnibus-bootstrap-mainnet`: `7340` (REST), `7341` (MCP)

Override with:
- `OMNIBUS_PREVIEW_REST_PORT`, `OMNIBUS_PREVIEW_MCP_PORT`
- `OMNIBUS_MAINNET_REST_PORT`, `OMNIBUS_MAINNET_MCP_PORT`
- `OMNIBUS_BOOTSTRAP_PREVIEW_REST_PORT`, `OMNIBUS_BOOTSTRAP_PREVIEW_MCP_PORT`
- `OMNIBUS_BOOTSTRAP_MAINNET_REST_PORT`, `OMNIBUS_BOOTSTRAP_MAINNET_MCP_PORT`
