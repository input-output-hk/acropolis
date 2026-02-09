//! Plutus Phase 2 script validation.
//!
//! This module provides Phase 2 (script execution) validation for Plutus smart contracts
//! using the `uplc-turbo` crate from pragma-org/uplc.
//!
//! # Overview
//!
//! Phase 2 validation evaluates Plutus scripts after Phase 1 validation has passed.
//! It verifies that all scripts in a transaction execute successfully within their
//! allocated execution budgets.
//!
//! # Feature Flag
//!
//! Phase 2 validation is disabled by default. Enable it via configuration:
//! ```toml
//! [module.tx-unpacker]
//! phase2_enabled = true
//! ```

// TODO: T006 - Define ExBudget struct
// TODO: T007 - Define Phase2Error enum
// TODO: T008 - Define ScriptPurpose enum
// TODO: T012 - Implement evaluate_script()
// TODO: T026 - Implement validate_transaction_phase2()
