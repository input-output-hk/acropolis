# Makefile for Acropolis workspace

SHELL := bash
CARGO := cargo
PROCESS_PKG := acropolis_process_omnibus

# Override with: make <target> SNAPSHOT=/path/to/snapshot.cbor
SNAPSHOT ?= tests/fixtures/your_snapshot.cbor

SECTIONS_ALL := --params --governance --pools --accounts --utxo

.PHONY: help build run snapshot-summary snapshot-sections-all snapshot-bootstrap fmt clippy

help:
	@echo "Targets:"
	@echo "  build                    Build the omnibus process"
	@echo "  run                      Run the omnibus process normally"
	@echo "  snapshot-summary         Run summary on SNAPSHOT via ACROPOLIS_SNAPSHOT_ARGS"
	@echo "  snapshot-sections-all    Run sections (all) on SNAPSHOT via ACROPOLIS_SNAPSHOT_ARGS"
	@echo "  snapshot-bootstrap       Run bootstrap on SNAPSHOT via ACROPOLIS_SNAPSHOT_ARGS"
	@echo "  fmt                      Run cargo fmt"
	@echo "  clippy                   Run cargo clippy -D warnings"
	@echo ""
	@echo "Variables:"
	@echo "  SNAPSHOT=<path>          Path to snapshot file (default: $(SNAPSHOT))"

build:
	$(CARGO) build -p $(PROCESS_PKG)

run:
	$(CARGO) run -p $(PROCESS_PKG)

snapshot-summary:
	@test -n "$(SNAPSHOT)" || (echo "SNAPSHOT is required"; exit 1)
	ACROPOLIS_SNAPSHOT_ARGS="summary $(SNAPSHOT)" $(CARGO) run -p $(PROCESS_PKG)

snapshot-sections-all:
	@test -n "$(SNAPSHOT)" || (echo "SNAPSHOT is required"; exit 1)
	ACROPOLIS_SNAPSHOT_ARGS="sections $(SNAPSHOT) $(SECTIONS_ALL)" $(CARGO) run -p $(PROCESS_PKG)

snapshot-bootstrap:
	@test -n "$(SNAPSHOT)" || (echo "SNAPSHOT is required"; exit 1)
	ACROPOLIS_SNAPSHOT_ARGS="bootstrap $(SNAPSHOT)" $(CARGO) run -p $(PROCESS_PKG)

fmt:
	$(CARGO) fmt --all

clippy:
	$(CARGO) clippy --workspace -D warnings
