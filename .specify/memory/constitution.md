# Project Constitution

## 1. Technical Stack
- Language: Rust 2024 Edition
- Async Runtime: Tokio
- Error Handling: thiserror/anyhow
- Serialization: Serde and CBOR

## 2. Architectural Principles
- Modular
- Strict separation of public API (`lib.rs`) and internal implementation.
- All database interactions must use Fjall v3

## 3. Code Style & Safety
- Idiomatic Rust: Use clippy suggestions.
- Unsafe: Avoid if possible.
- Error Handling: No `unwrap()`. Use `Result` and `?` operator.
- No use of panic()

## 4. Documentation
- All public types/functions must have doc comments.

## 5. Testing
- Prefer to follow a Test Driven Development (TDD) workflow
- Produce integration tests that can be run in CI/CD to identify regressions in nightly builds
