//! Durable simulation evidence and a read-only restart diagnostic, never replay.
//! CRC32 detects accidental damage; it is not authentication or ownership evidence.
use crate::replay::Result;
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

pub const MAX_BYTES: usize = 256 * 1024;
const MAX_RECORDS: usize = crate::MAX_STEPS * 3 + 1;
const IDENTITY: &str = "simulation-only:owned-pages-v1";
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Captured,
    ApplyIntent,
    RestoredVerified,
    RunComplete,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub identity: String,
    pub step: usize,
    pub total_steps: usize,
    pub stage: Stage,
    pub original_bytes: usize,
    pub compress_requested: bool,
}
impl Entry {
    pub(crate) fn owned(
        step: usize,
        total_steps: usize,
        stage: Stage,
        compress_requested: bool,
    ) -> Self {
        Self {
            identity: IDENTITY.into(),
            step,
            total_steps,
            stage,
            original_bytes: crate::PAGES * crate::PAGE_SIZE,
            compress_requested,
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    version: u32,
    scope: String,
    sequence: usize,
    previous_crc32: u32,
    entry: Entry,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    payload: Payload,
    crc32: u32,
}
/// New regular file only; no overwrites, symlinks, FIFO blocking or deletion on error.
/// Each intent is flushed before private buffer work. Power-loss guarantees depend on
/// the filesystem/storage honoring sync; no physical cgroup intent is ever accepted.
pub struct Journal {
    file: File,
    sequence: usize,
    previous: u32,
    bytes: usize,
    poisoned: bool,
    last: Option<Entry>,
}
impl Journal {
    pub fn create(path: &Path) -> Result<Self> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = std::fs::canonicalize(parent)?;
        if ["/proc", "/sys", "/dev"]
            .iter()
            .any(|p| parent.starts_with(p))
        {
            return Err("journal must be outside system filesystems".into());
        }
        let name = path.file_name().ok_or("missing journal filename")?;
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(parent.join(name))?;
        if !file.metadata()?.is_file() {
            return Err("journal must be regular".into());
        }
        file.sync_all()?;
        File::open(parent)?.sync_all()?;
        Ok(Self {
            file,
            sequence: 0,
            previous: 0,
            bytes: 0,
            poisoned: false,
            last: None,
        })
    }
    pub fn append(&mut self, entry: Entry) -> Result<()> {
        if self.poisoned || self.sequence >= MAX_RECORDS {
            return Err("journal record bound".into());
        }
        if !ordered(self.last.as_ref(), &entry) {
            return Err("journal stage ordering conflict".into());
        }
        let (line, crc) = encode(self.sequence, self.previous, entry.clone())?;
        if self.bytes + line.len() > MAX_BYTES {
            return Err("journal byte bound".into());
        }
        // Advance only after sync. On error the caller stops and preserves this file.
        self.poisoned = true;
        self.file.write_all(&line)?;
        self.file.sync_all()?;
        self.sequence += 1;
        self.previous = crc;
        self.bytes += line.len();
        self.last = Some(entry);
        self.poisoned = false;
        Ok(())
    }
}
fn ordered(last: Option<&Entry>, e: &Entry) -> bool {
    if e.identity != IDENTITY
        || e.original_bytes != crate::PAGES * crate::PAGE_SIZE
        || !(1..=crate::MAX_STEPS).contains(&e.total_steps)
        || e.step >= e.total_steps
    {
        return false;
    }
    match last {
        None => e.step == 0 && e.stage == Stage::Captured,
        Some(last) if e.total_steps == last.total_steps => match last.stage {
            Stage::RestoredVerified => {
                (e.step == last.step + 1 && e.stage == Stage::Captured)
                    || (e.step == last.step
                        && e.step + 1 == e.total_steps
                        && e.stage == Stage::RunComplete
                        && e.compress_requested == last.compress_requested)
            }
            Stage::RunComplete => false,
            _ => {
                e.step == last.step
                    && e.compress_requested == last.compress_requested
                    && matches!(
                        (last.stage, e.stage),
                        (Stage::Captured, Stage::ApplyIntent)
                            | (Stage::ApplyIntent, Stage::RestoredVerified)
                    )
            }
        },
        Some(_) => false,
    }
}
fn encode(sequence: usize, previous_crc32: u32, entry: Entry) -> Result<(Vec<u8>, u32)> {
    let payload = Payload {
        version: 1,
        scope: "owned_buffer_simulation".into(),
        sequence,
        previous_crc32,
        entry,
    };
    let crc32 = crc32fast::hash(&serde_json::to_vec(&payload)?);
    let mut bytes = serde_json::to_vec(&Record { payload, crc32 })?;
    bytes.push(b'\n');
    Ok((bytes, crc32))
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    SimulationRestored,
    Unresolved,
    Corrupt,
    IdentityMismatch,
}
#[derive(Debug, Serialize)]
pub struct Diagnostic {
    pub schema_version: u32,
    pub status: Status,
    pub valid_prefix_records: usize,
    pub last: Option<Entry>,
    pub physical_mutation_available: bool,
    pub automatic_recovery_available: bool,
    pub manual_intervention_required: bool,
    pub explanation: String,
}
pub fn inspect_file(path: &Path) -> Result<Diagnostic> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err("recovery evidence must be a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    Ok(inspect(&bytes, None))
}
/// `expected_identity` is useful for fixture/restart comparisons only. It is never
/// fresh kernel evidence and cannot change the unconditional recovery write lock.
pub fn inspect(bytes: &[u8], expected_identity: Option<&str>) -> Diagnostic {
    let mut d = Diagnostic {
        schema_version: 1,
        status: Status::Unresolved,
        valid_prefix_records: 0,
        last: None,
        physical_mutation_available: false,
        automatic_recovery_available: false,
        manual_intervention_required: true,
        explanation:
            "No completed evidence; preserve records and inspect manually. No writes performed."
                .into(),
    };
    let result = (|| -> Result<()> {
        if bytes.len() > MAX_BYTES {
            return Err("journal byte bound".into());
        }
        let mut previous = 0;
        for line in bytes.split_inclusive(|b| *b == b'\n') {
            if d.valid_prefix_records >= MAX_RECORDS || !line.ends_with(b"\n") {
                return Err("truncated or excessive journal; valid prefix retained".into());
            }
            let r: Record = serde_json::from_slice(line)?;
            let p = &r.payload;
            if p.version != 1
                || p.scope != "owned_buffer_simulation"
                || p.sequence != d.valid_prefix_records
                || p.previous_crc32 != previous
                || r.crc32 != crc32fast::hash(&serde_json::to_vec(p)?)
                || p.entry.identity != IDENTITY
                || p.entry.step >= crate::MAX_STEPS
                || p.entry.original_bytes != crate::PAGES * crate::PAGE_SIZE
            {
                return Err("journal integrity, scope or bounds failure".into());
            }
            let e = &p.entry;
            if !ordered(d.last.as_ref(), e) {
                return Err("journal stage ordering conflict".into());
            }
            previous = r.crc32;
            d.last = Some(e.clone());
            d.valid_prefix_records += 1;
        }
        Ok(())
    })();
    if let Err(e) = result {
        d.status = Status::Corrupt;
        d.explanation = format!("{e}; preserve original evidence; no replay.");
    } else if expected_identity
        .is_some_and(|id| d.last.as_ref().is_some_and(|last| last.identity != id))
    {
        d.status = Status::IdentityMismatch;
        d.explanation =
            "Restart identity differs; no restoration or deletion is authorized.".into();
    } else if d
        .last
        .as_ref()
        .is_some_and(|e| e.stage == Stage::RunComplete)
    {
        d.status = Status::SimulationRestored;
        d.manual_intervention_required = false;
        d.explanation = "All declared owned-buffer steps verified and released; terminal record present. Simulation evidence only; no kernel recovery claim.".into();
    } else {
        d.explanation = "Incomplete owned-buffer evidence; effect is uncertain. Preserve file, no replay. Process-private buffers are reclaimed by the OS after process death; this says nothing about kernel controls.".into();
    }
    d
}
