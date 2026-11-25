//! Configuration ownership ledger for tracking HyperCore-managed settings and drop-ins.
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read, Write},
    path::Path,
};

pub const LEDGER_SCHEMA_VERSION: u32 = 1;

/// Integrity status of a managed configuration entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrityStatus {
    /// File or parameter matches the recorded applied state.
    Clean,
    /// Content was modified externally by administrator or another tool.
    ModifiedExternally,
    /// Configuration file has been removed from disk.
    Missing,
    /// Target path is not readable or inaccessible.
    Inaccessible,
}

/// An auditable record of a single setting or drop-in managed by HyperCore.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedEntry {
    pub key: String,
    pub target_file: Option<String>,
    pub original_value: Option<String>,
    pub applied_value: String,
    pub applied_checksum: Option<String>,
    pub transaction_id: String,
    pub created_at_unix_ms: u64,
    pub last_verified_at_unix_ms: u64,
}

/// Durable ledger storing ownership metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnershipLedger {
    pub schema_version: u32,
    pub updated_at_unix_ms: u64,
    pub entries: BTreeMap<String, ManagedEntry>,
}

impl Default for OwnershipLedger {
    fn default() -> Self {
        Self {
            schema_version: LEDGER_SCHEMA_VERSION,
            updated_at_unix_ms: 0,
            entries: BTreeMap::new(),
        }
    }
}

impl OwnershipLedger {
    /// Compute a cryptographic SHA-256 checksum string for file contents and plan fingerprints.
    pub fn compute_checksum(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let hash = hasher.finalize();
        let mut s = String::with_capacity(64);
        for byte in hash {
            use std::fmt::Write;
            let _ = write!(s, "{:02x}", byte);
        }
        s
    }

    /// Fast non-cryptographic CRC-32 checksum for lightweight operational checks.
    pub fn compute_crc32(bytes: &[u8]) -> String {
        let mut hasher = crc32fast::Hasher::new();
        hasher.update(bytes);
        format!("{:08x}", hasher.finalize())
    }

    /// Load the ledger from the specified path. If missing, returns a default empty ledger.
    pub fn load_from(path: &Path) -> io::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let mut file = File::open(path)?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        serde_json::from_str(&buf)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
    }

    /// Save the ledger atomically using a temporary file and sync.
    pub fn save_to(&mut self, path: &Path, now_ms: u64) -> io::Result<()> {
        if path.is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Refusing to save ledger through a symlink",
            ));
        }
        self.updated_at_unix_ms = now_ms;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp_path = path.with_extension("tmp");
        let payload = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

        {
            let mut file = File::create(&temp_path)?;
            file.write_all(payload.as_bytes())?;
            file.sync_all()?;
        }

        fs::rename(&temp_path, path)?;
        Ok(())
    }

    /// Check the integrity of a specific managed entry on the filesystem.
    pub fn check_entry_integrity(
        &self,
        entry: &ManagedEntry,
        root_prefix: &Path,
    ) -> IntegrityStatus {
        if let Some(target_file) = &entry.target_file {
            let rel_path = target_file
                .strip_prefix("/etc/")
                .or_else(|| target_file.strip_prefix("etc/"))
                .or_else(|| target_file.strip_prefix('/'))
                .unwrap_or(target_file);
            let full_path = root_prefix.join(rel_path);

            if !full_path.exists() {
                return IntegrityStatus::Missing;
            }

            match fs::read(&full_path) {
                Ok(bytes) => {
                    let current_hash = Self::compute_checksum(&bytes);
                    if let Some(expected) = &entry.applied_checksum {
                        if current_hash == *expected {
                            IntegrityStatus::Clean
                        } else {
                            IntegrityStatus::ModifiedExternally
                        }
                    } else {
                        IntegrityStatus::Clean
                    }
                }
                Err(_) => IntegrityStatus::Inaccessible,
            }
        } else {
            IntegrityStatus::Clean
        }
    }

    /// Register or update a managed entry in the ledger.
    #[allow(clippy::too_many_arguments)]
    pub fn record_applied(
        &mut self,
        key: String,
        target_file: Option<String>,
        original_value: Option<String>,
        applied_value: String,
        file_bytes: Option<&[u8]>,
        tx_id: String,
        now_ms: u64,
    ) {
        let applied_checksum = file_bytes.map(Self::compute_checksum);
        let entry = ManagedEntry {
            key: key.clone(),
            target_file,
            original_value,
            applied_value,
            applied_checksum,
            transaction_id: tx_id,
            created_at_unix_ms: now_ms,
            last_verified_at_unix_ms: now_ms,
        };
        self.entries.insert(key, entry);
    }

    /// Remove a managed entry from the ledger upon verified rollback.
    pub fn remove_entry(&mut self, key: &str) -> Option<ManagedEntry> {
        self.entries.remove(key)
    }

    /// Determine if HyperCore currently holds ownership of the specified parameter.
    pub fn is_owned(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }
}
