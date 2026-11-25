//! Reversible configuration engine, ownership ledger, and durable execution journal.
#![forbid(unsafe_code)]

pub mod action;
pub mod authorization;
pub mod executor;
pub mod journal;
pub mod ledger;
pub mod memory_planner;

pub use action::{
    ActionClassification, AuthorizationScope, ConfigSubsystem, ConfigurationAction,
    PersistenceRequirement, ReversalProcedure, TypedMutation, VerificationProcedure,
    CONFIG_ACTION_SCHEMA_VERSION,
};
pub use authorization::SetupAuthorization;
pub use executor::{ActionResult, ConfigExecutor, ExecutionSummary, RollbackSummary};
pub use journal::{ConfigJournal, ConfigJournalItem, ConfigTxPhase, CONFIG_JOURNAL_SCHEMA_VERSION};
pub use ledger::{IntegrityStatus, ManagedEntry, OwnershipLedger, LEDGER_SCHEMA_VERSION};
pub use memory_planner::{MemoryPlanner, MemoryProfile};
