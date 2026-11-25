//! Reversible configuration engine, ownership ledger, and durable execution journal.
#![forbid(unsafe_code)]

pub mod action;
pub mod authorization;
pub mod cpu_planner;
pub mod executor;
pub mod gpu_planner;
pub mod journal;
pub mod ledger;
pub mod memory_planner;
pub mod power_planner;
pub mod storage_planner;
pub mod whole_system_planner;

pub use action::{
    ActionClassification, AuthorizationScope, ConfigSubsystem, ConfigurationAction,
    PersistenceRequirement, ReversalProcedure, TypedMutation, VerificationProcedure,
    CONFIG_ACTION_SCHEMA_VERSION,
};
pub use authorization::{
    RuntimeAuthorizationContract, SetupAuthorization, RUNTIME_AUTH_SCHEMA_VERSION,
};
pub use cpu_planner::{CpuPlanner, CpuProfile};
pub use executor::{ActionResult, ConfigExecutor, ExecutionSummary, RollbackSummary};
pub use gpu_planner::{GpuPlanner, GpuProfile};
pub use journal::{ConfigJournal, ConfigJournalItem, ConfigTxPhase, CONFIG_JOURNAL_SCHEMA_VERSION};
pub use ledger::{IntegrityStatus, ManagedEntry, OwnershipLedger, LEDGER_SCHEMA_VERSION};
pub use memory_planner::{MemoryPlanner, MemoryProfile};
pub use power_planner::PowerPlanner;
pub use storage_planner::StoragePlanner;
pub use whole_system_planner::{WholeSystemPlanSummary, WholeSystemPlanner};
