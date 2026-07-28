use std::io;
use std::path::PathBuf;

use reqwest::StatusCode;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error(
        "failed to read configuration file '{}': {source}",
        path.display()
    )]
    ReadConfig {
        path: PathBuf,

        #[source]
        source: io::Error,
    },

    #[error(
        "failed to parse configuration file '{}': {source}",
        path.display()
    )]
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

    #[error("failed to create the Authorization header")]
    InvalidAuthorizationHeader(#[source] reqwest::header::InvalidHeaderValue),

    #[error("failed to build HTTP client: {0}")]
    BuildHttpClient(#[source] reqwest::Error),

    #[error("failed to construct API URL for '{path}': {source}")]
    BuildApiUrl {
        path: String,

        #[source]
        source: url::ParseError,
    },

    #[error("{method} {url} failed: {source}")]
    HttpRequest {
        method: &'static str,
        url: String,

        #[source]
        source: reqwest::Error,
    },

    #[error("failed to read response body from {method} {url}: {source}")]
    ReadResponseBody {
        method: &'static str,
        url: String,

        #[source]
        source: reqwest::Error,
    },

    #[error("{method} {url} returned HTTP {status}: {body}")]
    ApiStatus {
        method: &'static str,
        url: String,
        status: StatusCode,
        body: String,
    },

    #[error("failed to decode JSON response from {method} {url}: {source}")]
    DecodeResponse {
        method: &'static str,
        url: String,

        #[source]
        source: serde_json::Error,
    },

    #[error("pagination URL points to a different origin: {url}")]
    UnsafePaginationUrl { url: String },

    #[error("pagination entered a loop at URL: {url}")]
    PaginationLoop { url: String },

    #[error("pagination returned {actual} records, but API reported {expected}")]
    PaginationCountMismatch { expected: usize, actual: usize },

    #[error("pagination exceeded the configured maximum of {maximum} pages")]
    TooManyPages { maximum: usize },

    #[error("invalid next-page URL '{value}': {source}")]
    InvalidNextPageUrl {
        value: String,

        #[source]
        source: url::ParseError,
    },

    #[error("filter contains an unsupported TOML value")]
    UnsupportedFilterValue,

    #[error("{0}")]
    StageNotImplemented(String),

    #[error("migration plan invariant failed: {0}")]
    PlanInvariant(String),

    #[error(
    "failed to create output directory '{}': {source}",
    path.display()
    )]
    CreateOutputDirectory {
        path: PathBuf,

        #[source]
        source: io::Error,
    },

    #[error(
    "failed to create output file '{}': {source}",
    path.display()
    )]
    CreateOutputFile {
        path: PathBuf,

        #[source]
        source: io::Error,
    },

    #[error(
    "failed to write JSON file '{}': {source}",
    path.display()
    )]
    WriteJson {
        path: PathBuf,

        #[source]
        source: serde_json::Error,
    },

    #[error(
    "CSV operation failed for '{}': {source}",
    path.display()
    )]
    Csv {
        path: PathBuf,

        #[source]
        source: csv::Error,
    },

    #[error(
    "failed to flush output file '{}': {source}",
    path.display()
    )]
    FlushOutputFile {
        path: PathBuf,

        #[source]
        source: io::Error,
    },

    #[error(
    "failed to read migration plan '{}': {source}",
    path.display()
    )]
    ReadPlan {
        path: PathBuf,

        #[source]
        source: io::Error,
    },

    #[error(
    "failed to decode migration plan '{}': {source}",
    path.display()
    )]
    DecodePlan {
        path: PathBuf,

        #[source]
        source: serde_json::Error,
    },

    #[error("decision file is invalid: {0}")]
    InvalidDecisionFile(String),

    #[error(
    "failed to write output file '{}': {source}",
    path.display()
    )]
    WriteOutputFile {
        path: PathBuf,

        #[source]
        source: io::Error,
    },
}
