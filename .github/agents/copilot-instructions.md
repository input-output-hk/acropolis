# acropolis Development Guidelines

Auto-generated from all feature plans. Last updated: 2026-02-06

## Active Technologies
- Rust 2024 Edition + `uplc-turbo` (pragma-org/uplc), pallas, tokio
- Rust 2024 Edition + uplc-turbo (pragma-org/uplc, pinned commit), pallas, tokio, rayon, caryatid (568-datum-lifecycle)
- Fjall v3 (immutable UTxOs), DashMap/HashMap (volatile UTxOs) (568-datum-lifecycle)

## Project Structure

```text
src/
tests/
```

## Code Style & Safety
- Idiomatic Rust: Use clippy suggestions.
- Unsafe: Avoid if possible.
- Error Handling: No `unwrap()`. Use `Result` and `?` operator.
- No use of panic()


## Commands

cargo test [ONLY COMMANDS FOR ACTIVE TECHNOLOGIES][ONLY COMMANDS FOR ACTIVE TECHNOLOGIES]
cargo clippy --all-targets --all-features -- -D warnings

## Code Style

Rust 2024 Edition: Follow standard conventions

## Recent Changes
- 568-datum-lifecycle: Added Rust 2024 Edition + uplc-turbo (pragma-org/uplc, pinned commit), pallas, tokio, rayon, caryatid
- 568-plutus-phase2-validation: Added Rust 2024 Edition + `uplc-turbo` (pragma-org/uplc), pallas, tokio
