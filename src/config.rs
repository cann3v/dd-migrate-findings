use std::collections::{BTreeMap, HashSet};
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use toml::Value;
use url::Url;

use crate::error::AppError;

pub const DOJO_URL_ENV: &str = "DOJO_URL";
pub const DOJO_API_TOKEN_ENV: &str = "DOJO_API_TOKEN";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub source: SourceConfig,
    pub destination: DestinationConfig,

    #[serde(default)]
    pub matching: MatchingConfig,

    #[serde(default)]
    pub http: HttpConfig,

    #[serde(default)]
    pub output: OutputConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceConfig {
    pub product_id: u64,

    #[serde(default)]
    pub filters: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DestinationConfig {
    pub product_ids: Vec<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MatchingConfig {
    pub trim_title: bool,
    pub require_found_by: bool,
}

impl Default for MatchingConfig {
    fn default() -> Self {
        Self {
            trim_title: true,
            require_found_by: true,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OutputConfig {
    pub directory: PathBuf,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            directory: PathBuf::from("output"),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HttpConfig {
    pub timeout_seconds: u64,
    pub page_size: usize,
    pub max_pages: usize,
    pub accept_invalid_certificates: bool,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            timeout_seconds: 30,
            page_size: 200,
            max_pages: 10_000,
            accept_invalid_certificates: false,
        }
    }
}

impl AppConfig {
    pub fn load(path: &Path) -> Result<Self, AppError> {
        let raw = fs::read_to_string(path).map_err(|source| AppError::ReadConfig {
            path: path.to_path_buf(),
            source,
        })?;

        let config = toml::from_str::<Self>(&raw).map_err(|source| AppError::ParseConfig {
            path: path.to_path_buf(),
            source,
        })?;

        config.validate()?;

        Ok(config)
    }

    pub fn validate(&self) -> Result<(), AppError> {
        let mut errors = Vec::new();

        if self.source.product_id == 0 {
            errors.push("source.product_id must be greater than zero".to_owned());
        }

        if self.destination.product_ids.is_empty() {
            errors.push("destination.product_ids must contain at least one product ID".to_owned());
        }

        let mut unique_destination_ids = HashSet::new();

        for product_id in &self.destination.product_ids {
            if *product_id == 0 {
                errors.push("destination.product_ids cannot contain zero".to_owned());
            }

            if *product_id == self.source.product_id {
                errors.push(format!(
                    "source product {} cannot also be a destination product",
                    product_id
                ));
            }

            if !unique_destination_ids.insert(*product_id) {
                errors.push(format!(
                    "destination product ID {} is specified more than once",
                    product_id
                ));
            }
        }

        if !self.matching.trim_title {
            errors
                .push("matching.trim_title must be true for the agreed matching policy".to_owned());
        }

        if !self.matching.require_found_by {
            errors.push(
                "matching.require_found_by must be true for the agreed matching policy".to_owned(),
            );
        }

        if self.http.timeout_seconds == 0 {
            errors.push("http.timeout_seconds must be greater than 0".to_owned());
        }

        if self.http.page_size == 0 {
            errors.push("http.page_size must be greater than 0".to_owned());
        }

        if self.http.page_size > 1_000 {
            errors.push("http.page_size cannot be greater than 1000".to_owned());
        }

        if self.http.max_pages == 0 {
            errors.push("http.max_pages must be greater than 0".to_owned());
        }

        if self.output.directory.as_os_str().is_empty() {
            errors.push("output.directory cannot be empty".to_owned());
        }

        for (name, value) in &self.source.filters {
            if matches!(
                name.as_str(),
                "test__engagement__product" | "limit" | "offset"
            ) {
                errors.push(format!(
                    "source filter '{name}' is managed by the application \
                    and cannot be set in the configuration"
                ));
            }

            validate_filter_name(name, &mut errors);
            validate_filter_value(name, value, &mut errors);
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(AppError::InvalidConfig(format_validation_errors(&errors)))
        }
    }
}

fn validate_filter_name(name: &str, errors: &mut Vec<String>) {
    if name.is_empty() {
        errors.push("source filter name cannot be empty".to_owned());
        return;
    }

    let is_valid = name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_');

    if !is_valid {
        errors.push(format!(
            "source filter '{name}' contains unsupported characters; \
             only ASCII letters, digits and underscores are allowed"
        ));
    }
}

fn validate_filter_value(name: &str, value: &Value, errors: &mut Vec<String>) {
    match value {
        Value::String(_) | Value::Integer(_) | Value::Float(_) | Value::Boolean(_) => {}

        Value::Array(values) => {
            if values.is_empty() {
                errors.push(format!(
                    "source filter '{name}' cannot contain an empty array"
                ));
                return;
            }

            for value in values {
                match value {
                    Value::String(_) | Value::Integer(_) | Value::Float(_) | Value::Boolean(_) => {}

                    Value::Array(_) | Value::Table(_) | Value::Datetime(_) => {
                        errors.push(format!(
                            "source filter '{name}' must contain only scalar array values"
                        ));
                        break;
                    }
                }
            }
        }

        Value::Table(_) | Value::Datetime(_) => {
            errors.push(format!(
                "source filter '{name}' must be a string, number, boolean, \
                 or an array of scalar values"
            ));
        }
    }
}

fn format_validation_errors(errors: &[String]) -> String {
    errors
        .iter()
        .map(|error| format!("  - {error}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub struct ApiToken(String);

impl ApiToken {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ApiToken([REDACTED])")
    }
}

pub struct RuntimeEnvironment {
    pub base_url: Url,
    pub api_token: ApiToken,
}

impl RuntimeEnvironment {
    pub fn load() -> Result<Self, AppError> {
        let raw_url = read_required_environment_variable(DOJO_URL_ENV)?;
        let raw_token = read_required_environment_variable(DOJO_API_TOKEN_ENV)?;

        let base_url =
            Url::parse(&raw_url).map_err(|error| AppError::InvalidEnvironmentVariable {
                name: DOJO_URL_ENV,
                reason: error.to_string(),
            })?;

        if !matches!(base_url.scheme(), "http" | "https") {
            return Err(AppError::InvalidEnvironmentVariable {
                name: DOJO_URL_ENV,
                reason: "URL scheme must be http or https".to_owned(),
            });
        }

        if base_url.host_str().is_none() {
            return Err(AppError::InvalidEnvironmentVariable {
                name: DOJO_URL_ENV,
                reason: "URL must contain a host".to_owned(),
            });
        }

        Ok(Self {
            base_url: normalize_base_url(base_url),
            api_token: ApiToken(raw_token),
        })
    }
}

fn read_required_environment_variable(name: &'static str) -> Result<String, AppError> {
    let value = env::var(name).map_err(|_| AppError::MissingEnvironmentVariable { name })?;

    let value = value.trim().to_owned();

    if value.is_empty() {
        return Err(AppError::InvalidEnvironmentVariable {
            name,
            reason: "value cannot be empty".to_owned(),
        });
    }

    Ok(value)
}

fn normalize_base_url(mut url: Url) -> Url {
    if !url.path().ends_with('/') {
        let normalized_path = format!("{}/", url.path());
        url.set_path(&normalized_path);
    }

    url
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_config() -> AppConfig {
        AppConfig {
            source: SourceConfig {
                product_id: 2,
                filters: BTreeMap::from([
                    ("duplicate".to_owned(), Value::Boolean(false)),
                    ("false_p".to_owned(), Value::Boolean(true)),
                ]),
            },
            destination: DestinationConfig {
                product_ids: vec![1, 4],
            },
            matching: MatchingConfig::default(),
            http: HttpConfig::default(),
            output: OutputConfig::default(),
        }
    }

    #[test]
    fn valid_configuration_is_accepted() {
        assert!(valid_config().validate().is_ok());
    }

    #[test]
    fn source_cannot_be_destination() {
        let mut config = valid_config();
        config.destination.product_ids.push(2);

        let error = config.validate().unwrap_err().to_string();

        assert!(error.contains("source product 2 cannot also be a destination product"));
    }

    #[test]
    fn duplicate_destination_is_rejected() {
        let mut config = valid_config();
        config.destination.product_ids.push(4);

        let error = config.validate().unwrap_err().to_string();

        assert!(error.contains("destination product ID 4 is specified more than once"));
    }

    #[test]
    fn invalid_filter_name_is_rejected() {
        let mut config = valid_config();

        config
            .source
            .filters
            .insert("false-positive".to_owned(), Value::Boolean(true));

        let error = config.validate().unwrap_err().to_string();

        assert!(error.contains("source filter 'false-positive' contains unsupported characters"));
    }

    #[test]
    fn nested_filter_array_is_rejected() {
        let mut config = valid_config();

        config.source.filters.insert(
            "severity".to_owned(),
            Value::Array(vec![Value::Array(vec![Value::String("High".to_owned())])]),
        );

        let error = config.validate().unwrap_err().to_string();

        assert!(error.contains("source filter 'severity' must contain only scalar array values"));
    }

    #[test]
    fn base_url_gets_trailing_slash() {
        let url = Url::parse("https://dojo.example.org/api").unwrap();

        assert_eq!(
            normalize_base_url(url).as_str(),
            "https://dojo.example.org/api/"
        );
    }

    #[test]
    fn application_managed_filter_is_rejected() {
        let mut config = valid_config();

        config
            .source
            .filters
            .insert("limit".to_owned(), Value::Integer(500));

        let error = config.validate().unwrap_err().to_string();

        assert!(error.contains("source filter 'limit' is managed by the application"));
    }
}
