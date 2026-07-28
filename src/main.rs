mod api;
mod cli;
mod config;
mod error;

use std::path::Path;

use clap::Parser;

use crate::api::DefectDojoClient;
use crate::cli::{Cli, Command, ExecutionMode};
use crate::config::{AppConfig, RuntimeEnvironment};
use crate::error::AppError;

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
            run_read_only_inspection(&config, &environment, &arguments.output_name).await
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
                    "write operations are disabled in stage 2".to_owned(),
                )),
            }
        }
    }
}

async fn run_read_only_inspection(
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

    println!("Read-only DefectDojo inspection");
    println!("DefectDojo URL: {}", environment.base_url);
    println!();

    let source_product = client.get_product(config.source.product_id).await?;

    println!(
        "Source product: [{}] {}",
        source_product.id, source_product.name
    );

    let source_findings = client
        .list_findings(config.source.product_id, &config.source.filters)
        .await?;

    println!("Source findings after filters: {}", source_findings.len());

    if let Some(sample) = source_findings.first() {
        let notes = client.get_finding_notes(sample.id).await?;

        println!("Sample source finding: #{} — {}", sample.id, sample.title);
        println!(
            "Sample found_by values: {}",
            format_product_ids(&sample.found_by)
        );
        println!("Sample notes: {}", notes.notes.len());
    } else {
        println!("No source finding is available for notes inspection.");
    }

    println!();
    println!("Destination products:");

    for product_id in &config.destination.product_ids {
        let product = client.get_product(*product_id).await?;

        let findings = client
            .list_findings(*product_id, &Default::default())
            .await?;

        println!(
            "  [{}] {} — {} findings",
            product.id,
            product.name,
            findings.len()
        );
    }

    println!();
    println!("Output directory: {}", config.output.directory.display());
    println!("Future plan name: {output_name}");
    println!();
    println!("No changes were made.");
    println!("Only GET requests were sent to DefectDojo.");

    Ok(())
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

fn format_product_ids(product_ids: &[u64]) -> String {
    if product_ids.is_empty() {
        return "<none>".to_owned();
    }

    product_ids
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}
