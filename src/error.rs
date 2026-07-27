use std::io;
use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("failed to read configuration file '{}': {source}", path.display())]
    ReadConfig {
        path: PathBuf,

        #[source]
        source: io::Error,
    },

    #[error("failed to parse configuration file '{}': {source}", path.display())]
    ParseConfig {
        path: PathBuf,

        #[source]
        source: toml::de::Error,
    },

    #[error("configuration is invalid:\n{0}")]
    InvalidConfig(String),

    #[error("required environment variable '{name}' is not set")]
    MissingEnvironmentVariable { name: &'static str },

    #[error("environment variable '{name}' is invalid: {reason}")]
    InvalidEnvironmentVariable { name: &'static str, reason: String },

    #[error("input file '{}' does not exist", path.display())]
    InputFileDoesNotExist { path: PathBuf },

    #[error("input path '{}' is not a file", path.display())]
    InputPathIsNotFile { path: PathBuf },

    #[error("{0}")]
    StageNotImplemented(String),
}
