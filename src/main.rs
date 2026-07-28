mod api;
mod cli;
mod config;
mod dry_run;
mod error;
mod matching;
mod plan;
mod progress;

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::time::{Duration, Instant};

use clap::Parser;

use crate::api::DefectDojoClient;
use crate::api::models::Finding;
use crate::cli::{Cli, Command, ExecutionMode};
use crate::config::{AppConfig, RuntimeEnvironment};
use crate::dry_run::{DryRunOutcome, DryRunReport, build_dry_run_report};
use crate::error::AppError;
use crate::matching::{
    CorrelationReport, NotFoundReason, SourceCorrelation, SourceCoverageClass, TargetActionClass,
    TargetOperation, correlate_findings,
};
use crate::plan::{
    ApprovedDecisionSet, MigrationPlanBuilder, load_approved_decisions, validate_decision_file,
    write_plan_files,
};
use crate::progress::{DownloadProgress, ItemProgress};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), AppError> {
    let cli = Cli::parse();
    let config = AppConfig::load(&cli.config)?;

    match cli.command {
        Command::Plan(arguments) => {
            let environment = RuntimeEnvironment::load()?;

            run_plan(&config, &environment, &arguments.output_name).await
        }

        Command::Validate(arguments) => {
            ensure_input_file(&arguments.decisions)?;

            let summary = validate_decision_file(&arguments.decisions)?;

            println!("Decision file is valid.");
            println!("Rows: {}", summary.total_rows);
            println!("apply_all: {}", summary.apply_all_rows);
            println!("skip: {}", summary.skip_rows);

            Ok(())
        }

        Command::Apply(arguments) => {
            ensure_input_file(&arguments.decisions)?;

            match arguments.execution_mode() {
                ExecutionMode::DryRun => {
                    let environment = RuntimeEnvironment::load()?;

                    run_apply_dry_run(&config, &environment, &arguments.decisions).await
                }

                ExecutionMode::Execute => Err(AppError::StageNotImplemented(
                    "write operations are disabled in stage 5".to_owned(),
                )),
            }
        }
    }
}

async fn run_plan(
    config: &AppConfig,
    environment: &RuntimeEnvironment,
    output_name: &str,
) -> Result<(), AppError> {
    if output_name.trim().is_empty() {
        return Err(AppError::InvalidConfig(
            "  - plan output name cannot be empty".to_owned(),
        ));
    }

    let client = DefectDojoClient::new(environment, &config.http)?;

    println!("Read-only DefectDojo correlation");
    println!("DefectDojo URL: {}", environment.base_url);
    println!();

    let source_product = client.get_product(config.source.product_id).await?;

    println!(
        "Source product: [{}] {}",
        source_product.id, source_product.name
    );

    let (source_findings, source_elapsed) = load_findings_with_progress(
        &client,
        config.source.product_id,
        &config.source.filters,
        format!(
            "Loading source product [{}] {}",
            source_product.id, source_product.name
        ),
    )
    .await?;

    println!(
        "Source findings after filters: {} ({source_elapsed:?})",
        source_findings.len()
    );

    if let Some(sample) = source_findings.first() {
        let notes = client.get_finding_notes(sample.id).await?;

        println!(
            "Sample source finding: #{} — {}",
            sample.id,
            sanitize_title(&sample.title)
        );
        println!("Sample found_by values: {}", format_ids(&sample.found_by));
        println!("Sample notes: {}", notes.notes.len());
    }

    let mut plan_builder = MigrationPlanBuilder::new(
        environment.base_url.to_string(),
        source_product,
        config.source.filters.clone(),
        source_findings.clone(),
    );

    println!();
    println!("Destination correlation:");

    for product_id in &config.destination.product_ids {
        let product = client.get_product(*product_id).await?;

        let (destination_findings, loading_elapsed) = load_findings_with_progress(
            &client,
            *product_id,
            &Default::default(),
            format!(
                "Loading destination product [{}] {}",
                product.id, product.name
            ),
        )
        .await?;

        let correlation_started = Instant::now();

        let report = correlate_findings(&source_findings, &destination_findings, *product_id);

        let correlation_elapsed = correlation_started.elapsed();

        println!();
        println!("[{}] {}", product.id, product.name);
        println!(
            "  Destination findings: {} ({loading_elapsed:?})",
            destination_findings.len()
        );
        println!("  Correlation time: {correlation_elapsed:?}");

        print_report(&report);

        plan_builder.add_destination(product, &destination_findings, report)?;
    }

    let plan = plan_builder.build();

    let written = write_plan_files(&plan, &config.output.directory, output_name)?;

    println!();
    println!("Migration plan created.");
    println!("JSON: {}", written.json_path.display());
    println!("CSV:  {}", written.csv_path.display());
    println!();
    println!("No changes were made to DefectDojo.");
    println!("Only GET requests were sent to DefectDojo.");

    println!();
    println!("Output directory: {}", config.output.directory.display());

    Ok(())
}

async fn load_findings_with_progress(
    client: &DefectDojoClient,
    product_id: u64,
    filters: &std::collections::BTreeMap<String, toml::Value>,
    message: String,
) -> Result<(Vec<Finding>, Duration), AppError> {
    let progress = DownloadProgress::new(message);
    let started = Instant::now();

    let result = client
        .list_findings(product_id, filters, Some(&progress))
        .await;

    progress.finish();

    result.map(|findings| (findings, started.elapsed()))
}

fn print_report(report: &CorrelationReport) {
    println!("  Source coverage:");

    for class in SourceCoverageClass::ALL {
        let count = report
            .sources
            .iter()
            .filter(|result| result.class == class)
            .count();

        println!("    {:<24} {}", class.label(), count);
    }

    println!("  Target actions:");

    for class in TargetActionClass::ALL {
        let count = report
            .target_operations
            .iter()
            .filter(|operation| operation.class == class)
            .count();

        println!("    {:<24} {}", class.label(), count);
    }

    print_not_found_diagnostics(&report.sources);
    print_source_examples(&report.sources);
    print_target_examples(&report.target_operations);
}

fn print_not_found_diagnostics(sources: &[SourceCorrelation]) {
    println!("  NotFound diagnostic signals:");

    for reason in NotFoundReason::ALL {
        let count = sources
            .iter()
            .filter(|source| {
                source.class == SourceCoverageClass::NotFound
                    && source.not_found_reasons.contains(&reason)
            })
            .count();

        println!("    {:<24} {}", reason.label(), count);
    }

    println!("    Note: one NotFound finding may have multiple signals");
}

fn print_source_examples(sources: &[SourceCorrelation]) {
    const EXAMPLES_PER_CLASS: usize = 3;

    for class in SourceCoverageClass::ALL {
        let examples = sources
            .iter()
            .filter(|source| source.class == class)
            .take(EXAMPLES_PER_CLASS)
            .collect::<Vec<_>>();

        if examples.is_empty() {
            continue;
        }

        println!("  {} examples:", class.label());

        for example in examples {
            println!(
                "    source #{} -> product {} -> \
                 candidates: {} -> reasons: {} — {}",
                example.source_finding_id,
                example.target_product_id,
                example.candidate_ids_display(),
                example.not_found_reasons_display(),
                sanitize_title(&example.source_title)
            );
        }
    }
}

fn print_target_examples(operations: &[TargetOperation]) {
    const EXAMPLES_PER_CLASS: usize = 3;

    for class in TargetActionClass::ALL {
        let examples = operations
            .iter()
            .filter(|operation| operation.class == class)
            .take(EXAMPLES_PER_CLASS)
            .collect::<Vec<_>>();

        if examples.is_empty() {
            continue;
        }

        println!("  {} target examples:", class.label());

        for operation in examples {
            println!(
                "    source #{} -> target #{} \
                 in product {} — {}",
                operation.source_finding_id,
                operation.target_finding_id,
                operation.target_product_id,
                sanitize_title(&operation.source_title)
            );
        }
    }
}

fn sanitize_title(title: &str) -> String {
    const MAX_CHARACTERS: usize = 120;

    let single_line = title
        .replace(['\r', '\n'], " ")
        .trim()
        .chars()
        .take(MAX_CHARACTERS)
        .collect::<String>();

    if title.trim().chars().count() > MAX_CHARACTERS {
        format!("{single_line}…")
    } else {
        single_line
    }
}

fn ensure_input_file(path: &Path) -> Result<(), AppError> {
    if !path.exists() {
        return Err(AppError::InputFileDoesNotExist {
            path: path.to_path_buf(),
        });
    }

    if !path.is_file() {
        return Err(AppError::InputPathIsNotFile {
            path: path.to_path_buf(),
        });
    }

    Ok(())
}

fn format_ids(ids: &[u64]) -> String {
    if ids.is_empty() {
        return "<none>".to_owned();
    }

    ids.iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

async fn run_apply_dry_run(
    config: &AppConfig,
    environment: &RuntimeEnvironment,
    decisions_path: &Path,
) -> Result<(), AppError> {
    let decisions = load_approved_decisions(decisions_path)?;

    validate_plan_context(config, environment, &decisions)?;

    let client = DefectDojoClient::new(environment, &config.http)?;

    println!("DefectDojo migration dry-run");
    println!("Approved operations: {}", decisions.operations.len());
    println!();

    let source_product = &decisions.plan.source_product;

    let (current_sources, source_elapsed) = load_findings_with_progress(
        &client,
        source_product.id,
        &decisions.plan.source_filters,
        format!(
            "Refreshing source product [{}] {}",
            source_product.id, source_product.name
        ),
    )
    .await?;

    println!(
        "Current source findings: {} ({source_elapsed:?})",
        current_sources.len()
    );

    let current_sources = current_sources
        .into_iter()
        .map(|finding| (finding.id, finding))
        .collect::<HashMap<_, _>>();

    let target_ids = decisions
        .operations
        .iter()
        .map(|operation| operation.target_finding_id)
        .collect::<BTreeSet<_>>();

    let target_progress =
        ItemProgress::new(target_ids.len(), "Refreshing selected target findings");

    let mut current_targets = HashMap::new();

    for target_id in target_ids {
        let finding = client.get_finding(target_id).await?;

        current_targets.insert(target_id, finding);
        target_progress.increment();
    }

    target_progress.finish();

    let report = build_dry_run_report(
        &decisions.plan,
        &decisions.operations,
        &current_sources,
        &current_targets,
    );

    print_dry_run_report(&report);

    println!();
    println!("No changes were made.");
    println!("No PATCH or POST requests were sent.");

    Ok(())
}

fn validate_plan_context(
    config: &AppConfig,
    environment: &RuntimeEnvironment,
    decisions: &ApprovedDecisionSet,
) -> Result<(), AppError> {
    if decisions.plan.dojo_base_url != environment.base_url.as_str() {
        return Err(AppError::InvalidDecisionFile(format!(
            "plan was created for '{}', but current DOJO_URL is '{}'",
            decisions.plan.dojo_base_url, environment.base_url
        )));
    }

    if decisions.plan.source_product.id != config.source.product_id {
        return Err(AppError::InvalidDecisionFile(format!(
            "plan source product is {}, but config source product is {}",
            decisions.plan.source_product.id, config.source.product_id
        )));
    }

    if decisions.plan.source_filters != config.source.filters {
        return Err(AppError::InvalidDecisionFile(
            "source filters in plan differ from current config".to_owned(),
        ));
    }

    let planned_targets = decisions
        .plan
        .destination_products
        .iter()
        .map(|product| product.id)
        .collect::<BTreeSet<_>>();

    let configured_targets = config
        .destination
        .product_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();

    if planned_targets != configured_targets {
        return Err(AppError::InvalidDecisionFile(
            "destination products in plan differ from current config".to_owned(),
        ));
    }

    Ok(())
}

fn print_dry_run_report(report: &DryRunReport) {
    println!();
    println!("Dry-run results:");

    for outcome in DryRunOutcome::ALL {
        let count = report
            .items
            .iter()
            .filter(|item| item.outcome == outcome)
            .count();

        println!("  {:<24} {}", outcome.label(), count);
    }

    let ready = report.items.iter().filter_map(|item| item.patch.as_ref());

    let mut description_updates = 0;
    let mut mitigation_updates = 0;
    let mut impact_updates = 0;

    for patch in ready {
        description_updates += usize::from(patch.description.is_some());

        mitigation_updates += usize::from(patch.mitigation.is_some());

        impact_updates += usize::from(patch.impact.is_some());
    }

    println!("Prepared text additions:");
    println!("  description: {description_updates}");
    println!("  mitigation:  {mitigation_updates}");
    println!("  impact:      {impact_updates}");

    print_dry_run_problem_examples(report);
}

fn print_dry_run_problem_examples(report: &DryRunReport) {
    const EXAMPLES: usize = 5;

    for outcome in [
        DryRunOutcome::SourceChanged,
        DryRunOutcome::SourceMissing,
        DryRunOutcome::TargetChanged,
        DryRunOutcome::TargetMissingFromPlan,
    ] {
        let examples = report
            .items
            .iter()
            .filter(|item| item.outcome == outcome)
            .take(EXAMPLES)
            .collect::<Vec<_>>();

        if examples.is_empty() {
            continue;
        }

        println!("{} examples:", outcome.label());

        for item in examples {
            println!(
                "  row {}: source #{} -> target #{} \
                 in product {}",
                item.row_id, item.source_finding_id, item.target_finding_id, item.target_product_id
            );
        }
    }
}
