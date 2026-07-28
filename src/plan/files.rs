use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::matching::{SourceCoverageClass, TargetActionClass};
use crate::plan::models::{DecisionAction, MigrationPlan, PLAN_SCHEMA_VERSION};

const CSV_DELIMITER: u8 = b';';

#[derive(Debug)]
pub struct WrittenPlanFiles {
    pub json_path: PathBuf,
    pub csv_path: PathBuf,
}

#[derive(Debug)]
pub struct DecisionValidationSummary {
    pub total_rows: usize,
    pub apply_all_rows: usize,
    pub skip_rows: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CsvRowType {
    TargetOperation,
    SourceIssue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CsvDecisionRow {
    row_id: String,
    row_type: CsvRowType,

    source_finding_id: u64,
    source_title: String,

    target_product_id: u64,
    target_finding_id: Option<u64>,

    class: String,
    candidate_ids: String,
    not_found_reasons: String,

    action: DecisionAction,
    selected_target_id: Option<u64>,
}

pub fn write_plan_files(
    plan: &MigrationPlan,
    output_directory: &Path,
    output_name: &str,
) -> Result<WrittenPlanFiles, AppError> {
    validate_output_name(output_name)?;

    fs::create_dir_all(output_directory).map_err(|source| AppError::CreateOutputDirectory {
        path: output_directory.to_path_buf(),
        source,
    })?;

    let json_path = output_directory.join(format!("{output_name}.json"));

    let csv_path = output_directory.join(format!("{output_name}.csv"));

    write_json(plan, &json_path)?;

    let rows = build_csv_rows(plan);
    write_csv(&rows, &csv_path)?;

    Ok(WrittenPlanFiles {
        json_path,
        csv_path,
    })
}

pub fn validate_decision_file(csv_path: &Path) -> Result<DecisionValidationSummary, AppError> {
    let json_path = csv_path.with_extension("json");
    let plan = read_plan(&json_path)?;

    if plan.schema_version != PLAN_SCHEMA_VERSION {
        return Err(AppError::InvalidDecisionFile(format!(
            "unsupported plan schema version {}; expected {}",
            plan.schema_version, PLAN_SCHEMA_VERSION
        )));
    }

    let expected_rows = build_csv_rows(&plan)
        .into_iter()
        .map(|row| (row.row_id.clone(), row))
        .collect::<BTreeMap<_, _>>();

    let mut reader = csv::ReaderBuilder::new()
        .delimiter(CSV_DELIMITER)
        .from_path(csv_path)
        .map_err(|source| AppError::Csv {
            path: csv_path.to_path_buf(),
            source,
        })?;

    let mut seen_row_ids = HashSet::new();
    let mut total_rows = 0;
    let mut apply_all_rows = 0;
    let mut skip_rows = 0;

    for row in reader.deserialize::<CsvDecisionRow>() {
        let row = row.map_err(|source| AppError::Csv {
            path: csv_path.to_path_buf(),
            source,
        })?;

        if !seen_row_ids.insert(row.row_id.clone()) {
            return Err(AppError::InvalidDecisionFile(format!(
                "CSV contains duplicate row_id '{}'",
                row.row_id
            )));
        }

        let expected = expected_rows.get(&row.row_id).ok_or_else(|| {
            AppError::InvalidDecisionFile(format!("CSV contains unknown row_id '{}'", row.row_id))
        })?;

        validate_immutable_fields(&row, expected)?;
        validate_decision(&row)?;

        match row.action {
            DecisionAction::ApplyAll => {
                apply_all_rows += 1;
            }
            DecisionAction::Skip => {
                skip_rows += 1;
            }
        }

        total_rows += 1;
    }

    if total_rows != expected_rows.len() {
        let missing = expected_rows
            .keys()
            .filter(|row_id| !seen_row_ids.contains(*row_id))
            .take(10)
            .cloned()
            .collect::<Vec<_>>();

        return Err(AppError::InvalidDecisionFile(format!(
            "CSV contains {total_rows} rows, but plan expects {}; \
             missing rows include: {}",
            expected_rows.len(),
            missing.join(", ")
        )));
    }

    Ok(DecisionValidationSummary {
        total_rows,
        apply_all_rows,
        skip_rows,
    })
}

fn build_csv_rows(plan: &MigrationPlan) -> Vec<CsvDecisionRow> {
    let mut rows = Vec::new();

    for operation in &plan.operations {
        rows.push(CsvDecisionRow {
            row_id: format!(
                "target-{}-{}-{}",
                operation.source_finding_id,
                operation.target_product_id,
                operation.target_finding_id
            ),
            row_type: CsvRowType::TargetOperation,
            source_finding_id: operation.source_finding_id,
            source_title: operation.source_title.clone(),
            target_product_id: operation.target_product_id,
            target_finding_id: Some(operation.target_finding_id),
            class: operation.class.label().to_owned(),
            candidate_ids: String::new(),
            not_found_reasons: String::new(),
            action: operation.default_action,
            selected_target_id: None,
        });
    }

    for correlation in &plan.coverage {
        if correlation.class == SourceCoverageClass::ExactMatch {
            continue;
        }

        rows.push(CsvDecisionRow {
            row_id: format!(
                "source-{}-{}-{}",
                correlation.source_finding_id,
                correlation.target_product_id,
                correlation.class.label()
            ),
            row_type: CsvRowType::SourceIssue,
            source_finding_id: correlation.source_finding_id,
            source_title: correlation.source_title.clone(),
            target_product_id: correlation.target_product_id,
            target_finding_id: None,
            class: correlation.class.label().to_owned(),
            candidate_ids: join_ids(&correlation.candidate_ids),
            not_found_reasons: correlation
                .not_found_reasons
                .iter()
                .map(|reason| reason.label())
                .collect::<Vec<_>>()
                .join("|"),
            action: DecisionAction::Skip,
            selected_target_id: None,
        });
    }

    rows.sort_by(|left, right| left.row_id.cmp(&right.row_id));

    rows
}

fn write_json(plan: &MigrationPlan, path: &Path) -> Result<(), AppError> {
    let file = File::create(path).map_err(|source| AppError::CreateOutputFile {
        path: path.to_path_buf(),
        source,
    })?;

    let writer = BufWriter::new(file);

    serde_json::to_writer_pretty(writer, plan).map_err(|source| AppError::WriteJson {
        path: path.to_path_buf(),
        source,
    })
}

fn write_csv(rows: &[CsvDecisionRow], path: &Path) -> Result<(), AppError> {
    let file = File::create(path).map_err(|source| AppError::CreateOutputFile {
        path: path.to_path_buf(),
        source,
    })?;

    let mut buffered = BufWriter::new(file);

    // buffered
    //     .write_all(b"\xEF\xBB\xBF")
    //     .map_err(|source| AppError::WriteOutputFile {
    //         path: path.to_path_buf(),
    //         source,
    //     })?;

    let mut writer = csv::WriterBuilder::new()
        .delimiter(CSV_DELIMITER)
        .from_writer(buffered);

    for row in rows {
        writer.serialize(row).map_err(|source| AppError::Csv {
            path: path.to_path_buf(),
            source,
        })?;
    }

    writer.flush().map_err(|source| AppError::FlushOutputFile {
        path: path.to_path_buf(),
        source,
    })
}

fn read_plan(path: &Path) -> Result<MigrationPlan, AppError> {
    let file = File::open(path).map_err(|source| AppError::ReadPlan {
        path: path.to_path_buf(),
        source,
    })?;

    serde_json::from_reader(BufReader::new(file)).map_err(|source| AppError::DecodePlan {
        path: path.to_path_buf(),
        source,
    })
}

fn validate_immutable_fields(
    actual: &CsvDecisionRow,
    expected: &CsvDecisionRow,
) -> Result<(), AppError> {
    let immutable_fields_match = actual.row_type == expected.row_type
        && actual.source_finding_id == expected.source_finding_id
        && actual.source_title == expected.source_title
        && actual.target_product_id == expected.target_product_id
        && actual.target_finding_id == expected.target_finding_id
        && actual.class == expected.class
        && actual.candidate_ids == expected.candidate_ids
        && actual.not_found_reasons == expected.not_found_reasons;

    if immutable_fields_match {
        Ok(())
    } else {
        Err(AppError::InvalidDecisionFile(format!(
            "protected fields were changed in row '{}'",
            actual.row_id
        )))
    }
}

fn validate_decision(row: &CsvDecisionRow) -> Result<(), AppError> {
    match row.row_type {
        CsvRowType::TargetOperation => validate_target_operation(row),

        CsvRowType::SourceIssue => validate_source_issue(row),
    }
}

fn validate_target_operation(row: &CsvDecisionRow) -> Result<(), AppError> {
    if row.selected_target_id.is_some() {
        return Err(AppError::InvalidDecisionFile(format!(
            "row '{}' already has a target finding; \
             selected_target_id must be empty",
            row.row_id
        )));
    }

    if row.class == TargetActionClass::AlreadyUpToDate.label() && row.action != DecisionAction::Skip
    {
        return Err(AppError::InvalidDecisionFile(format!(
            "AlreadyUpToDate row '{}' must remain skip",
            row.row_id
        )));
    }

    Ok(())
}

fn validate_source_issue(row: &CsvDecisionRow) -> Result<(), AppError> {
    let selectable = matches!(
        row.class.as_str(),
        "PossibleMatch" | "AmbiguousPossibleMatch"
    );

    if !selectable {
        if row.action != DecisionAction::Skip {
            return Err(AppError::InvalidDecisionFile(format!(
                "row '{}' has class '{}' and cannot be applied",
                row.row_id, row.class
            )));
        }

        if row.selected_target_id.is_some() {
            return Err(AppError::InvalidDecisionFile(format!(
                "row '{}' cannot contain selected_target_id",
                row.row_id
            )));
        }

        return Ok(());
    }

    match row.action {
        DecisionAction::Skip => {
            if row.selected_target_id.is_some() {
                return Err(AppError::InvalidDecisionFile(format!(
                    "skipped row '{}' must not contain \
                         selected_target_id",
                    row.row_id
                )));
            }
        }

        DecisionAction::ApplyAll => {
            let selected = row.selected_target_id.ok_or_else(|| {
                AppError::InvalidDecisionFile(format!(
                    "row '{}' requires selected_target_id",
                    row.row_id
                ))
            })?;

            let candidates = parse_candidate_ids(&row.candidate_ids)?;

            if !candidates.contains(&selected) {
                return Err(AppError::InvalidDecisionFile(format!(
                    "selected target #{selected} is not an allowed \
                     candidate for row '{}'",
                    row.row_id
                )));
            }
        }
    }

    Ok(())
}

fn parse_candidate_ids(value: &str) -> Result<Vec<u64>, AppError> {
    if value.is_empty() {
        return Ok(Vec::new());
    }

    value
        .split('|')
        .map(|item| {
            item.parse::<u64>().map_err(|_| {
                AppError::InvalidDecisionFile(format!("invalid candidate ID '{item}'"))
            })
        })
        .collect()
}

fn join_ids(ids: &[u64]) -> String {
    ids.iter().map(u64::to_string).collect::<Vec<_>>().join("|")
}

fn validate_output_name(output_name: &str) -> Result<(), AppError> {
    let valid = !output_name.is_empty()
        && output_name.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        });

    if valid {
        Ok(())
    } else {
        Err(AppError::InvalidConfig(
            "  - output name may contain only ASCII letters, \
             digits, '-' and '_'"
                .to_owned(),
        ))
    }
}
