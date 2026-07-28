mod api;
mod cli;
mod config;
mod error;
mod matching;
mod progress;

use std::path::Path;
use std::time::{Duration, Instant};

use clap::Parser;

use crate::api::DefectDojoClient;
use crate::api::models::Finding;
use crate::cli::{Cli, Command, ExecutionMode};
use crate::config::{AppConfig, RuntimeEnvironment};
use crate::error::AppError;
use crate::matching::{
    CorrelationReport, NotFoundReason, SourceCorrelation, SourceCoverageClass, TargetActionClass,
    TargetOperation, correlate_findings,
};
use crate::progress::DownloadProgress;

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
    let environment = RuntimeEnvironment::load()?;

    match cli.command {
        Command::Plan(arguments) => {
            run_read_only_correlation(&config, &environment, &arguments.output_name).await
        }

        Command::Validate(arguments) => {
            ensure_input_file(&arguments.decisions)?;

            Err(AppError::StageNotImplemented(format!(
                "configuration and environment are valid, but \
                 validation of '{}' will be implemented at stage 4",
                arguments.decisions.display()
            )))
        }

        Command::Apply(arguments) => {
            ensure_input_file(&arguments.decisions)?;

            match arguments.execution_mode() {
                ExecutionMode::DryRun => Err(AppError::StageNotImplemented(format!(
                    "configuration and environment are valid, but \
                         dry-run application of '{}' will be implemented \
                         at stage 7",
                    arguments.decisions.display()
                ))),

                ExecutionMode::Execute => Err(AppError::StageNotImplemented(
                    "write operations are disabled in stage 3.1".to_owned(),
                )),
            }
        }
    }
}

async fn run_read_only_correlation(
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
    }

    println!();
    println!("Output directory: {}", config.output.directory.display());
    println!("Future plan name: {output_name}");
    println!();
    println!("No changes were made.");
    println!("Only GET requests were sent to DefectDojo.");

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
