use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "dd-migrate-findings",
    version,
    about = "Transfers triage data between matching DefectDojo findings"
)]
pub struct Cli {
    /// Path to the TOML configuration file.
    #[arg(
        long,
        global = true,
        value_name = "FILE",
        default_value = "config.toml"
    )]
    pub config: PathBuf,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Build a migration plan without changing DefectDojo.
    Plan(PlanArgs),

    /// Validate a migration CSV and its associated JSON plan.
    Validate(ValidateArgs),

    /// Apply or preview a previously generated migration plan.
    Apply(ApplyArgs),
}

#[derive(Debug, Args)]
pub struct PlanArgs {
    /// Base name for migration.csv and migration.json.
    #[arg(long, value_name = "NAME", default_value = "migration")]
    pub output_name: String,
}

#[derive(Debug, Args)]
pub struct ValidateArgs {
    /// CSV file containing reviewed migration decisions.
    #[arg(value_name = "CSV")]
    pub decisions: PathBuf,
}

#[derive(Debug, Args)]
pub struct ApplyArgs {
    /// CSV file containing reviewed migration decisions.
    #[arg(value_name = "CSV")]
    pub decisions: PathBuf,

    /// Explicitly confirm that changes may be sent to DefectDojo.
    #[arg(long, conflicts_with = "dry_run")]
    pub execute: bool,

    /// Explicitly run in read-only preview mode.
    ///
    /// Preview mode is also used by default when neither flag is supplied.
    #[arg(long)]
    pub dry_run: bool,
}

impl ApplyArgs {
    pub fn execution_mode(&self) -> ExecutionMode {
        if self.execute {
            ExecutionMode::Execute
        } else {
            ExecutionMode::DryRun
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    DryRun,
    Execute,
}
