# Makefile for Acropolis workspace

.DEFAULT_GOAL := build

SHELL := bash
CARGO := cargo
PYTHON := python3
PROCESS_PKG := acropolis_process_omnibus
LOG_LEVEL ?= info


# Test snapshots
SNAPSHOT_SMALL ?= tests/fixtures/snapshot-small.cbor
MANIFEST_SMALL ?= tests/fixtures/test-manifest.json

# Real Cardano Haskell node snapshot (Conway era, epoch 507)
SNAPSHOT ?= tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.cbor
MANIFEST ?= tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.json
SNAP_URL ?= "https://pub-b844360df4774bb092a2bb2043b888e5.r2.dev/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.cbor.gz"

SECTIONS_ALL := --params --governance --pools --accounts --utxo

.PHONY: help all build test run fmt clippy
.PHONY: snapshot-summary snapshot-sections-all snapshot-bootstrap
.PHONY: snap-test-streaming

help:
	@echo "Acropolis Makefile Targets:"
	@echo ""
	@echo "Build & Test:"
	@echo "  all                      Format, lint, and test"
	@echo "  build                    Build the omnibus process"
	@echo "  run 					  Run the omnibus"
	@echo "  test                     Run all tests"
	@echo "  fmt                      Run cargo fmt"
	@echo "  clippy                   Run cargo clippy -D warnings"
	@echo ""
	@echo "Snapshot Commands:"
	@echo "  snap-test-streaming      Test streaming parser with large snapshot (2.4GB)"
	@echo ""
	@echo "Variables:"
	@echo "  SNAPSHOT=<path>          Path to snapshot file (default: Conway epoch 507)"
	@echo "  LOG_LEVEL=<level>        Set log level (default: info, options: error, warn, info, debug, trace)"
	@echo ""
	@echo "Examples:"
	@echo "  make snap-test-streaming"
	@echo "  make run LOG_LEVEL=debug"
	@echo "  make snap-test-streaming SNAPSHOT=path/to/snapshot.cbor"

all: fmt clippy test

build:
	$(CARGO) build -p $(PROCESS_PKG)

test:
	$(CARGO) test

run:
	cd processes/omnibus && RUST_LOG=$(LOG_LEVEL) $(CARGO) run --release --bin $(PROCESS_PKG)

fmt:
	$(CARGO) fmt --all

clippy:
	$(CARGO) clippy --workspace -- -D warnings

snapshot-download: $(SNAPSHOT)

$(SNAPSHOT):
	echo "Downloading snapshot file..."
	curl -L -f -o "$(SNAPSHOT).gz" "$(SNAP_URL)" || { echo "Download failed"; exit 1; }
	gunzip "$(SNAPSHOT).gz"

# Streaming snapshot parser test
snap-test-streaming: $(SNAPSHOT)
	@echo "Testing Streaming Snapshot Parser"
	@echo "=================================="
	@echo "Snapshot: $(SNAPSHOT)"
	@echo "Size: $$(du -h $(SNAPSHOT) | cut -f1)"
	@echo "Log Level: $(LOG_LEVEL)"
	@echo ""
	@test -f "$(SNAPSHOT)" || (echo "Error: Snapshot file not found: $(SNAPSHOT)"; exit 1)
	@echo "This will parse the entire snapshot and collect all data with callbacks..."
	@echo "Expected time: ~1-3 minutes for 2.4GB snapshot with 11M UTXOs"
	@echo ""
	RUST_LOG=$(LOG_LEVEL) $(CARGO) run --release --example test_streaming_parser -- "$(SNAPSHOT)"

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
