mod api;
mod cli;
mod config;
mod error;
mod matching;

use std::path::Path;
use std::time::Instant;

use clap::Parser;

use crate::api::DefectDojoClient;
use crate::cli::{Cli, Command, ExecutionMode};
use crate::config::{AppConfig, RuntimeEnvironment};
use crate::error::AppError;
use crate::matching::{CorrelationClass, CorrelationResult, correlate_findings};

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
                "configuration and environment are valid, but validation \
                 of '{}' will be implemented at stage 4",
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
                    "write operations are disabled in stage 3".to_owned(),
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

    let source_started = Instant::now();

    let source_findings = client
        .list_findings(config.source.product_id, &config.source.filters)
        .await?;

    println!(
        "Source findings after filters: {} ({:?})",
        source_findings.len(),
        source_started.elapsed()
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

        let destination_started = Instant::now();

        let destination_findings = client
            .list_findings(*product_id, &Default::default())
            .await?;

        let loading_elapsed = destination_started.elapsed();

        let correlation_started = Instant::now();

        let results = correlate_findings(&source_findings, &destination_findings, *product_id);

        let correlation_elapsed = correlation_started.elapsed();

        println!();
        println!("[{}] {}", product.id, product.name);
        println!(
            "  Destination findings: {} ({loading_elapsed:?})",
            destination_findings.len()
        );
        println!("  Correlation time: {correlation_elapsed:?}");

        print_correlation_summary(&results);
        print_diagnostic_examples(&results);
    }

    println!();
    println!("Output directory: {}", config.output.directory.display());
    println!("Future plan name: {output_name}");
    println!();
    println!("No changes were made.");
    println!("Only GET requests were sent to DefectDojo.");

    Ok(())
}

fn print_correlation_summary(results: &[CorrelationResult]) {
    println!("  Results:");

    for class in CorrelationClass::ALL {
        let count = results
            .iter()
            .filter(|result| result.class == class)
            .count();

        println!("    {:<20} {}", class.label(), count);
    }
}

fn print_diagnostic_examples(results: &[CorrelationResult]) {
    const EXAMPLES_PER_CLASS: usize = 3;

    for class in CorrelationClass::ALL {
        let examples = results
            .iter()
            .filter(|result| result.class == class)
            .take(EXAMPLES_PER_CLASS)
            .collect::<Vec<_>>();

        if examples.is_empty() {
            continue;
        }

        println!("  {} examples:", class.label());

        for example in examples {
            println!(
                "    source #{} -> product {} -> [{}] candidates: {} — {}",
                example.source_finding_id,
                example.target_product_id,
                example.class.label(),
                example.candidate_ids_display(),
                sanitize_title(&example.source_title)
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
