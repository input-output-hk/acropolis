#!/bin/bash
# Use 'tarpaulin' to generate test coverage stats
set -e
cargo install cargo-tarpaulin

# Exclude all target files, examples and caryatid_macros
cargo tarpaulin --target-dir target/tarpaulin --skip-clean --workspace --lib --exclude caryatid_macros --exclude-files "target/**" --exclude-files "examples/**" "$@"
