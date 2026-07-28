use std::collections::HashMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::api::DefectDojoClient;
use crate::api::models::{CreateFindingNoteRequest, Finding, FindingPatchRequest, Note};
use crate::dry_run::{DryRunOutcome, DryRunReport, PreparedFindingPatch, prepare_patch};
use crate::error::AppError;
use crate::plan::MigrationPlan;
use crate::progress::ItemProgress;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionOutcome {
    Applied,
    PartiallyApplied,
    Failed,
    Skipped,
}

impl ExecutionOutcome {
    pub const ALL: [Self; 4] = [
        Self::Applied,
        Self::PartiallyApplied,
        Self::Failed,
        Self::Skipped,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Applied => "Applied",
            Self::PartiallyApplied => "PartiallyApplied",
            Self::Failed => "Failed",
            Self::Skipped => "Skipped",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ExecutionItem {
    pub row_id: String,
    pub source_finding_id: u64,
    pub target_product_id: u64,
    pub target_finding_id: u64,

    pub outcome: ExecutionOutcome,

    pub patch_applied: bool,
    pub notes_planned: usize,
    pub notes_created: usize,

    pub errors: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ExecutionReport {
    pub started_at_unix_seconds: u64,
    pub finished_at_unix_seconds: u64,
    pub items: Vec<ExecutionItem>,
}

impl ExecutionReport {
    pub fn failure_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| {
                matches!(
                    item.outcome,
                    ExecutionOutcome::PartiallyApplied | ExecutionOutcome::Failed
                )
            })
            .count()
    }
}

pub async fn execute_migration(
    client: &DefectDojoClient,
    plan: &MigrationPlan,
    dry_run: &DryRunReport,
    current_sources: &HashMap<u64, Finding>,
    current_source_notes: &HashMap<u64, Vec<Note>>,
    current_targets: &mut HashMap<u64, Finding>,
    current_target_notes: &mut HashMap<u64, Vec<Note>>,
) -> ExecutionReport {
    let started_at_unix_seconds = unix_timestamp();
    let progress = ItemProgress::new(dry_run.items.len(), "Applying migration operations");

    let mut items = Vec::with_capacity(dry_run.items.len());

    for dry_run_item in &dry_run.items {
        let execution_item = if dry_run_item.outcome != DryRunOutcome::Ready {
            ExecutionItem {
                row_id: dry_run_item.row_id.clone(),
                source_finding_id: dry_run_item.source_finding_id,
                target_product_id: dry_run_item.target_product_id,
                target_finding_id: dry_run_item.target_finding_id,
                outcome: ExecutionOutcome::Skipped,
                patch_applied: false,
                notes_planned: 0,
                notes_created: 0,
                errors: vec![format!(
                    "preflight outcome was {}",
                    dry_run_item.outcome.label()
                )],
            }
        } else {
            execute_item(
                client,
                plan,
                dry_run_item,
                current_sources,
                current_source_notes,
                current_targets,
                current_target_notes,
            )
            .await
        };

        items.push(execution_item);
        progress.increment();
    }

    progress.finish();

    ExecutionReport {
        started_at_unix_seconds,
        finished_at_unix_seconds: unix_timestamp(),
        items,
    }
}

async fn execute_item(
    client: &DefectDojoClient,
    plan: &MigrationPlan,
    dry_run_item: &crate::dry_run::DryRunItem,
    current_sources: &HashMap<u64, Finding>,
    current_source_notes: &HashMap<u64, Vec<Note>>,
    current_targets: &mut HashMap<u64, Finding>,
    current_target_notes: &mut HashMap<u64, Vec<Note>>,
) -> ExecutionItem {
    let mut result = ExecutionItem {
        row_id: dry_run_item.row_id.clone(),
        source_finding_id: dry_run_item.source_finding_id,
        target_product_id: dry_run_item.target_product_id,
        target_finding_id: dry_run_item.target_finding_id,
        outcome: ExecutionOutcome::Failed,
        patch_applied: false,
        notes_planned: 0,
        notes_created: 0,
        errors: Vec::new(),
    };

    let Some(source) = current_sources
        .get(&dry_run_item.source_finding_id)
        .cloned()
    else {
        result.errors.push(format!(
            "source finding #{} disappeared from execution state",
            dry_run_item.source_finding_id
        ));

        return result;
    };

    let Some(target) = current_targets
        .get(&dry_run_item.target_finding_id)
        .cloned()
    else {
        result.errors.push(format!(
            "target finding #{} disappeared from execution state",
            dry_run_item.target_finding_id
        ));

        return result;
    };

    let source_notes = current_source_notes
        .get(&dry_run_item.source_finding_id)
        .cloned()
        .unwrap_or_default();

    let target_notes = current_target_notes
        .get(&dry_run_item.target_finding_id)
        .cloned()
        .unwrap_or_default();

    /*
     * Важно: patch готовится по текущему локальному состоянию target.
     * Если предыдущая операция уже обновила этот же target, её текст
     * попадёт в target и не будет затёрт следующей операцией.
     */
    let prepared = prepare_patch(plan, &source, &target, &source_notes, &target_notes);

    result.notes_planned = prepared.notes.len();

    let request = finding_patch_request(&prepared);

    match client
        .patch_finding(dry_run_item.target_finding_id, &request)
        .await
    {
        Ok(updated_target) => {
            current_targets.insert(dry_run_item.target_finding_id, updated_target);

            result.patch_applied = true;
        }

        Err(error) => {
            result.errors.push(format!("finding PATCH failed: {error}"));

            return result;
        }
    }

    for prepared_note in prepared.notes {
        let source_note_id = prepared_note.source_note_id;

        let request = CreateFindingNoteRequest {
            entry: prepared_note.entry,
            private: false,
        };

        match client
            .create_finding_note(dry_run_item.target_finding_id, &request)
            .await
        {
            Ok(updated_notes) => {
                current_target_notes.insert(dry_run_item.target_finding_id, updated_notes.notes);

                result.notes_created += 1;
            }

            Err(error) => {
                result.errors.push(format!(
                    "source note #{source_note_id} POST failed: {error}"
                ));
            }
        }
    }

    result.outcome = if result.errors.is_empty() {
        ExecutionOutcome::Applied
    } else {
        ExecutionOutcome::PartiallyApplied
    };

    result
}

fn finding_patch_request(prepared: &PreparedFindingPatch) -> FindingPatchRequest {
    FindingPatchRequest {
        active: prepared.active,
        verified: prepared.verified,
        fixed: prepared.fixed,
        unfixed: prepared.unfixed,
        false_p: prepared.false_p,
        out_of_scope: prepared.out_of_scope,
        is_mitigated: prepared.is_mitigated,

        description: prepared.description.clone(),
        mitigation: prepared.mitigation.clone(),
        impact: prepared.impact.clone(),
    }
}

pub fn write_execution_report(
    report: &ExecutionReport,
    decisions_path: &Path,
) -> Result<PathBuf, AppError> {
    let parent = decisions_path.parent().unwrap_or_else(|| Path::new("."));

    let stem = decisions_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("migration");

    let path = parent.join(format!(
        "{stem}-execution-{}.json",
        report.finished_at_unix_seconds
    ));

    let file = File::create(&path).map_err(|source| AppError::CreateOutputFile {
        path: path.clone(),
        source,
    })?;

    serde_json::to_writer_pretty(BufWriter::new(file), report).map_err(|source| {
        AppError::WriteJson {
            path: path.clone(),
            source,
        }
    })?;

    Ok(path)
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
