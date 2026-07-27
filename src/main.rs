mod cli;
mod config;
mod error;

use std::path::Path;

use clap::Parser;

use crate::cli::{Cli, Command, ExecutionMode};
use crate::config::{AppConfig, RuntimeEnvironment};
use crate::error::AppError;

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), AppError> {
    let cli = Cli::parse();

    let config = AppConfig::load(&cli.config)?;
    let environment = RuntimeEnvironment::load()?;

    match cli.command {
        Command::Plan(arguments) => run_plan(&config, &environment, &arguments.output_name),

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
                    "configuration and environment are valid, but dry-run \
                         application of '{}' will be implemented at stage 7",
                    arguments.decisions.display()
                ))),

                ExecutionMode::Execute => Err(AppError::StageNotImplemented(
                    "write operations are disabled in stage 1".to_owned(),
                )),
            }
        }
    }
}

fn run_plan(
    config: &AppConfig,
    environment: &RuntimeEnvironment,
    output_name: &str,
) -> Result<(), AppError> {
    if output_name.trim().is_empty() {
        return Err(AppError::InvalidConfig(
            "  - plan output name cannot be empty".to_owned(),
        ));
    }

    // Reading the token length ensures that the secret was loaded, while the
    // token value itself is never printed.
    let _token_is_available = !environment.api_token.expose().is_empty();

    println!("Configuration is valid.");
    println!("DefectDojo URL: {}", environment.base_url);
    println!("Source product: {}", config.source.product_id);
    println!(
        "Destination products: {}",
        format_product_ids(&config.destination.product_ids)
    );
    println!("Source filters: {}", config.source.filters.len());
    println!("Output directory: {}", config.output.directory.display());
    println!("Output name: {output_name}");
    println!();
    println!("No API requests were made.");
    println!("The read-only planning operation will be implemented at stage 4.");

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
    product_ids
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}
