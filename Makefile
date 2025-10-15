# Makefile for Acropolis workspace

.DEFAULT_GOAL := build

SHELL := bash
CARGO := cargo
PYTHON := python3
PROCESS_PKG := acropolis_process_omnibus

# Test snapshots
SNAPSHOT_SMALL ?= tests/fixtures/snapshot-small.cbor
MANIFEST_SMALL ?= tests/fixtures/test-manifest.json

# Real Cardano Haskell node snapshot (Conway era, epoch 507)
SNAPSHOT ?= tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.cbor
MANIFEST ?= tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.json

SECTIONS_ALL := --params --governance --pools --accounts --utxo

.PHONY: help all build test run fmt clippy
.PHONY: snapshot-summary snapshot-sections-all snapshot-bootstrap
.PHONY: snap-info snap-inspect snap-metadata snap-boot-data snap-tip snap-utxos snap-count-utxos
.PHONY: snap-summary snap-sections snap-bootstrap snap-test-streaming

help:
	@echo "Acropolis Makefile Targets:"
	@echo ""
	@echo "Build & Test:"
	@echo "  all                      Format, lint, and test"
	@echo "  build                    Build the omnibus process"
	@echo "  test                     Run all tests"
	@echo "  fmt                      Run cargo fmt"
	@echo "  clippy                   Run cargo clippy -D warnings"
	@echo ""
	@echo "Legacy Snapshot Commands:"
	@echo "  snapshot-summary         Run summary on SNAPSHOT"
	@echo "  snapshot-sections-all    Run sections (all) on SNAPSHOT"
	@echo "  snapshot-bootstrap       Run bootstrap on SNAPSHOT"
	@echo ""
	@echo "Snapshot Commands (snap- prefix):"
	@echo "  snap-summary             Display snapshot summary (epoch, treasury, reserves, counts)"
	@echo "  snap-sections            Display specific sections (use SECTIONS to customize)"
	@echo "  snap-bootstrap           Bootstrap node state from snapshot"
	@echo "  snap-info                Show snapshot format detection and diagnostic info"
	@echo "  snap-inspect             Inspect CBOR structure of snapshot"
	@echo "  snap-metadata            Extract metadata (epoch, file size, UTXO count)"
	@echo "  snap-boot-data           Extract boot data (epoch, treasury, reserves, counts)"
	@echo "  snap-tip                 Extract tip info from snapshot filename"
	@echo "  snap-utxos               Parse sample UTXOs from snapshot (use LIMIT to control count)"
	@echo "  snap-count-utxos         Count ALL UTXOs in snapshot (slow for large files)"
	@echo "  snap-test-streaming      Test streaming parser with large snapshot (2.4GB)"
	@echo ""
	@echo "Variables:"
	@echo "  SNAPSHOT=<path>          Path to snapshot file (default: Conway epoch 507)"
	@echo "  MANIFEST=<path>          Path to manifest file"
	@echo "  SNAPSHOT_SMALL=<path>    Path to small test snapshot (default: snapshot-small.cbor)"
	@echo "  LIMIT=<n>                UTXO limit for snap-utxos (default: 10)"
	@echo "  SECTIONS=<flags>         Section flags for snap-sections (default: all)"
	@echo ""
	@echo "Examples:"
	@echo "  make snap-summary"
	@echo "  make snap-utxos LIMIT=100"
	@echo "  make snap-sections SECTIONS='--pools --governance'"

all: fmt clippy test

build:
	$(CARGO) build -p $(PROCESS_PKG)

test:
	$(CARGO) test

run:
	$(CARGO) run -p $(PROCESS_PKG)

fmt:
	$(CARGO) fmt --all

clippy:
	$(CARGO) clippy --workspace -- -D warnings

# Legacy snapshot commands (compatibility)
snapshot-summary:
	@test -n "$(SNAPSHOT)" || (echo "SNAPSHOT is required"; exit 1)
	ACROPOLIS_SNAPSHOT_ARGS="summary $(SNAPSHOT)" $(CARGO) run -p $(PROCESS_PKG)

snapshot-sections-all:
	@test -n "$(SNAPSHOT)" || (echo "SNAPSHOT is required"; exit 1)
	ACROPOLIS_SNAPSHOT_ARGS="sections $(SNAPSHOT) $(SECTIONS_ALL)" $(CARGO) run -p $(PROCESS_PKG)

snapshot-bootstrap:
	@test -n "$(SNAPSHOT)" || (echo "SNAPSHOT is required"; exit 1)
	ACROPOLIS_SNAPSHOT_ARGS="bootstrap $(SNAPSHOT)" $(CARGO) run -p $(PROCESS_PKG)

# New snap- prefixed commands
snap-summary:
	@echo "Snapshot Summary: $(SNAPSHOT)"
	@echo ""
	@test -f "$(SNAPSHOT)" || (echo "Error: Snapshot file not found: $(SNAPSHOT)"; exit 1)
	@ACROPOLIS_SNAPSHOT_ARGS="summary $(SNAPSHOT)" $(CARGO) run --release -p $(PROCESS_PKG)

snap-sections:
	@echo "Snapshot Sections: $(SNAPSHOT)"
	@echo "Sections: $${SECTIONS:-$(SECTIONS_ALL)}"
	@echo ""
	@test -f "$(SNAPSHOT)" || (echo "Error: Snapshot file not found: $(SNAPSHOT)"; exit 1)
	@ACROPOLIS_SNAPSHOT_ARGS="sections $(SNAPSHOT) $${SECTIONS:-$(SECTIONS_ALL)}" $(CARGO) run --release -p $(PROCESS_PKG)

snap-bootstrap:
	@echo "Snapshot Bootstrap: $(SNAPSHOT)"
	@echo ""
	@test -f "$(SNAPSHOT)" || (echo "Error: Snapshot file not found: $(SNAPSHOT)"; exit 1)
	@ACROPOLIS_SNAPSHOT_ARGS="bootstrap $(SNAPSHOT)" $(CARGO) run --release -p $(PROCESS_PKG)

snap-info:
	@echo "Testing snapshot format detection and diagnostic info..."
	@echo "Snapshot: $(SNAPSHOT)"
	@test -f "$(MANIFEST)" && echo "Manifest: $(MANIFEST)" || echo "Manifest: (not found)"
	@echo ""
	@test -f "$(SNAPSHOT)" || (echo "Error: Snapshot file not found: $(SNAPSHOT)"; exit 1)
	@ACROPOLIS_SNAPSHOT_ARGS="summary $(SNAPSHOT)" $(CARGO) run --release -p $(PROCESS_PKG) || true

snap-inspect:
	@echo "Inspecting snapshot CBOR structure..."
	@echo "Snapshot: $(SNAPSHOT)"
	@echo ""
	@test -f "$(SNAPSHOT)" || (echo "Error: Snapshot file not found: $(SNAPSHOT)"; exit 1)
	@echo "Note: Detailed CBOR inspection functionality to be implemented"
	@ACROPOLIS_SNAPSHOT_ARGS="summary $(SNAPSHOT)" $(CARGO) run --release -p $(PROCESS_PKG)

snap-metadata:
	@echo "Extracting metadata from snapshot..."
	@echo "Snapshot: $(SNAPSHOT)"
	@echo ""
	@test -f "$(SNAPSHOT)" || (echo "Error: Snapshot file not found: $(SNAPSHOT)"; exit 1)
	@ACROPOLIS_SNAPSHOT_ARGS="summary $(SNAPSHOT)" $(CARGO) run --release -p $(PROCESS_PKG)

snap-boot-data:
	@echo "Extracting boot data from snapshot..."
	@echo "Snapshot: $(SNAPSHOT)"
	@echo "Output: epoch, treasury, reserves, stake pools, DReps, accounts, proposals"
	@echo ""
	@test -f "$(SNAPSHOT)" || (echo "Error: Snapshot file not found: $(SNAPSHOT)"; exit 1)
	@ACROPOLIS_SNAPSHOT_ARGS="summary $(SNAPSHOT)" $(CARGO) run --release -p $(PROCESS_PKG)

snap-tip:
	@echo "Extracting tip information from snapshot filename..."
	@echo "Snapshot: $(SNAPSHOT)"
	@echo ""
	@test -f "$(SNAPSHOT)" || (echo "Error: Snapshot file not found: $(SNAPSHOT)"; exit 1)
	@ACROPOLIS_SNAPSHOT_ARGS="tip $(SNAPSHOT)" $(CARGO) run --release -p $(PROCESS_PKG)

snap-utxos:
	@echo "Parsing UTXOs from snapshot..."
	@echo "Snapshot: $(SNAPSHOT)"
	@echo "Limit: $${LIMIT:-10}"
	@echo ""
	@test -f "$(SNAPSHOT)" || (echo "Error: Snapshot file not found: $(SNAPSHOT)"; exit 1)
	@ACROPOLIS_SNAPSHOT_ARGS="utxos $(SNAPSHOT) $${LIMIT:-10}" $(CARGO) run --release -p $(PROCESS_PKG)

snap-count-utxos:
	@echo "Counting ALL UTXOs in snapshot (this may take a while)..."
	@echo "Snapshot: $(SNAPSHOT)"
	@echo ""
	@test -f "$(SNAPSHOT)" || (echo "Error: Snapshot file not found: $(SNAPSHOT)"; exit 1)
	@echo "Note: For large snapshots (11M+ UTXOs), this operation will take several minutes"
	@ACROPOLIS_SNAPSHOT_ARGS="count-utxos $(SNAPSHOT)" $(CARGO) run --release -p $(PROCESS_PKG)

snap-test-streaming:
	@echo "Testing Streaming Snapshot Parser"
	@echo "=================================="
	@echo "Snapshot: $(SNAPSHOT)"
	@echo "Size: $$(du -h $(SNAPSHOT) | cut -f1)"
	@echo ""
	@test -f "$(SNAPSHOT)" || (echo "Error: Snapshot file not found: $(SNAPSHOT)"; exit 1)
	@echo "This will parse the entire snapshot and collect all data with callbacks..."
	@echo "Expected time: ~1-3 minutes for 2.4GB snapshot with 11M UTXOs"
	@echo ""
	@$(CARGO) run --release --example test_streaming_parser -- "$(SNAPSHOT)"

# Pattern rule: generate .json manifest from .cbor snapshot
# Usage: make tests/fixtures/my-snapshot.json
# Extracts header metadata from CBOR and computes SHA256 + file size
%.json: %.cbor
	@echo "Generating manifest for $< -> $@"
	@echo "Note: Manifest generation script not yet ported"
	@echo "TODO: Port scripts/generate_manifest.py from original project"
	@ERA_FLAG=$${ERA:+--era $$ERA}; \
	BH_FLAG=$${BLOCK_HASH:+--block-hash $$BLOCK_HASH}; \
	BHGT_FLAG=$${BLOCK_HEIGHT:+--block-height $$BLOCK_HEIGHT}; \
	if [ -f scripts/generate_manifest.py ]; then \
		$(PYTHON) scripts/generate_manifest.py $$ERA_FLAG $$BH_FLAG $$BHGT_FLAG $< > $@; \
	else \
		echo "Error: scripts/generate_manifest.py not found"; \
		exit 1; \
	fi
