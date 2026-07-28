use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

use reqwest::Method;
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT};
use serde::Serialize;
use serde::de::DeserializeOwned;
use toml::Value;
use url::Url;

use crate::api::models::{
    CreateFindingNoteRequest, Finding, FindingPatchRequest, FindingToNotes, PaginatedResponse,
    Product,
};
use crate::config::{HttpConfig, RuntimeEnvironment};
use crate::error::AppError;

pub struct DefectDojoClient {
    http: reqwest::Client,
    base_url: Url,
    page_size: usize,
    max_pages: usize,
}

impl DefectDojoClient {
    pub fn new(environment: &RuntimeEnvironment, config: &HttpConfig) -> Result<Self, AppError> {
        let mut headers = HeaderMap::new();

        let authorization = format!("Token {}", environment.api_token.expose());

        let authorization =
            HeaderValue::from_str(&authorization).map_err(AppError::InvalidAuthorizationHeader)?;

        headers.insert(AUTHORIZATION, authorization);
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION")
            )),
        );

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(config.timeout_seconds))
            .danger_accept_invalid_certs(config.accept_invalid_certificates)
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .map_err(AppError::BuildHttpClient)?;

        Ok(Self {
            http,
            base_url: environment.base_url.clone(),
            page_size: config.page_size,
            max_pages: config.max_pages,
        })
    }

    pub async fn get_product(&self, product_id: u64) -> Result<Product, AppError> {
        let url = self.endpoint(&format!("api/v2/products/{product_id}/"))?;

        self.get_json(url).await
    }

    pub async fn list_findings(
        &self,
        product_id: u64,
        filters: &BTreeMap<String, Value>,
        observer: Option<&dyn PaginationObserver>,
    ) -> Result<Vec<Finding>, AppError> {
        let mut url = self.endpoint("api/v2/findings/")?;

        {
            let mut query = url.query_pairs_mut();

            query.append_pair("test__engagement__product", &product_id.to_string());

            query.append_pair("limit", &self.page_size.to_string());

            for (name, value) in filters {
                for serialized_value in serialize_filter_value(value)? {
                    query.append_pair(name, &serialized_value);
                }
            }
        }

        self.get_all_pages(url, observer).await
    }

    pub async fn get_finding(&self, finding_id: u64) -> Result<Finding, AppError> {
        let url = self.endpoint(&format!("api/v2/findings/{finding_id}/"))?;

        self.get_json(url).await
    }

    pub async fn get_finding_notes(&self, finding_id: u64) -> Result<FindingToNotes, AppError> {
        let url = self.endpoint(&format!("api/v2/findings/{finding_id}/notes/"))?;

        self.get_json(url).await
    }

    pub async fn patch_finding(
        &self,
        finding_id: u64,
        request: &FindingPatchRequest,
    ) -> Result<Finding, AppError> {
        let url = self.endpoint(&format!("api/v2/findings/{finding_id}/"))?;

        self.send_json(Method::PATCH, "PATCH", url, request).await
    }

    pub async fn create_finding_note(
        &self,
        finding_id: u64,
        request: &CreateFindingNoteRequest,
    ) -> Result<FindingToNotes, AppError> {
        let url = self.endpoint(&format!("api/v2/findings/{finding_id}/notes/"))?;

        self.send_json(Method::POST, "POST", url, request).await
    }

    async fn get_all_pages<T>(
        &self,
        mut url: Url,
        observer: Option<&dyn PaginationObserver>,
    ) -> Result<Vec<T>, AppError>
    where
        T: DeserializeOwned,
    {
        let mut results = Vec::new();
        let mut visited_urls = HashSet::new();
        let mut expected_count = None;

        for page_number in 1..=self.max_pages {
            self.ensure_same_origin(&url)?;

            let canonical_url = url.as_str().to_owned();

            if !visited_urls.insert(canonical_url.clone()) {
                return Err(AppError::PaginationLoop { url: canonical_url });
            }

            let page: PaginatedResponse<T> = self.get_json(url.clone()).await?;

            if page_number == 1 {
                expected_count = Some(page.count);
                results.reserve(page.count);
            }

            results.extend(page.results);

            if let Some(observer) = observer {
                observer.page_loaded(results.len(), expected_count.unwrap_or(results.len()));
            }

            let Some(next) = page.next else {
                let expected = expected_count.unwrap_or(results.len());

                if results.len() != expected {
                    return Err(AppError::PaginationCountMismatch {
                        expected,
                        actual: results.len(),
                    });
                }

                return Ok(results);
            };

            url = parse_next_page_url(&url, &next)?;
        }

        Err(AppError::TooManyPages {
            maximum: self.max_pages,
        })
    }

    async fn get_json<T>(&self, url: Url) -> Result<T, AppError>
    where
        T: DeserializeOwned,
    {
        self.ensure_same_origin(&url)?;

        let response =
            self.http
                .get(url.clone())
                .send()
                .await
                .map_err(|source| AppError::HttpRequest {
                    method: "GET",
                    url: url.to_string(),
                    source,
                })?;

        let status = response.status();

        let body = response
            .text()
            .await
            .map_err(|source| AppError::ReadResponseBody {
                method: "GET",
                url: url.to_string(),
                source,
            })?;

        if !status.is_success() {
            return Err(AppError::ApiStatus {
                method: "GET",
                url: url.to_string(),
                status,
                body: truncate_body(&body),
            });
        }

        serde_json::from_str(&body).map_err(|source| AppError::DecodeResponse {
            method: "GET",
            url: url.to_string(),
            source,
        })
    }

    async fn send_json<Request, Response>(
        &self,
        method: Method,
        method_name: &'static str,
        url: Url,
        request: &Request,
    ) -> Result<Response, AppError>
    where
        Request: Serialize + ?Sized,
        Response: DeserializeOwned,
    {
        self.ensure_same_origin(&url)?;

        let response = self
            .http
            .request(method, url.clone())
            .json(request)
            .send()
            .await
            .map_err(|source| AppError::HttpRequest {
                method: method_name,
                url: url.to_string(),
                source,
            })?;

        let status = response.status();

        let body = response
            .text()
            .await
            .map_err(|source| AppError::ReadResponseBody {
                method: method_name,
                url: url.to_string(),
                source,
            })?;

        if !status.is_success() {
            return Err(AppError::ApiStatus {
                method: method_name,
                url: url.to_string(),
                status,
                body: truncate_body(&body),
            });
        }

        serde_json::from_str(&body).map_err(|source| AppError::DecodeResponse {
            method: method_name,
            url: url.to_string(),
            source,
        })
    }

    fn endpoint(&self, relative_path: &str) -> Result<Url, AppError> {
        self.base_url
            .join(relative_path)
            .map_err(|source| AppError::BuildApiUrl {
                path: relative_path.to_owned(),
                source,
            })
    }

    fn ensure_same_origin(&self, url: &Url) -> Result<(), AppError> {
        let same_origin = url.scheme() == self.base_url.scheme()
            && url.host_str() == self.base_url.host_str()
            && url.port_or_known_default() == self.base_url.port_or_known_default();

        if same_origin {
            Ok(())
        } else {
            Err(AppError::UnsafePaginationUrl {
                url: url.to_string(),
            })
        }
    }
}

fn serialize_filter_value(value: &Value) -> Result<Vec<String>, AppError> {
    match value {
        Value::String(value) => Ok(vec![value.clone()]),

        Value::Integer(value) => Ok(vec![value.to_string()]),

        Value::Float(value) => Ok(vec![value.to_string()]),

        Value::Boolean(value) => Ok(vec![value.to_string()]),

        Value::Array(values) => values
            .iter()
            .map(|value| match value {
                Value::String(value) => Ok(value.clone()),
                Value::Integer(value) => Ok(value.to_string()),
                Value::Float(value) => Ok(value.to_string()),
                Value::Boolean(value) => Ok(value.to_string()),

                Value::Array(_) | Value::Table(_) | Value::Datetime(_) => {
                    Err(AppError::UnsupportedFilterValue)
                }
            })
            .collect(),

        Value::Table(_) | Value::Datetime(_) => Err(AppError::UnsupportedFilterValue),
    }
}

fn parse_next_page_url(current_url: &Url, next: &str) -> Result<Url, AppError> {
    match Url::parse(next) {
        Ok(url) => Ok(url),

        Err(url::ParseError::RelativeUrlWithoutBase) => {
            current_url
                .join(next)
                .map_err(|source| AppError::InvalidNextPageUrl {
                    value: next.to_owned(),
                    source,
                })
        }

        Err(source) => Err(AppError::InvalidNextPageUrl {
            value: next.to_owned(),
            source,
        }),
    }
}

fn truncate_body(body: &str) -> String {
    const MAX_CHARACTERS: usize = 2_000;

    let truncated = body.chars().take(MAX_CHARACTERS).collect::<String>();

    if body.chars().count() > MAX_CHARACTERS {
        format!("{truncated}…")
    } else {
        truncated
    }
}

pub trait PaginationObserver: Send + Sync {
    fn page_loaded(&self, loaded_records: usize, total_records: usize);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boolean_filter_is_serialized() {
        assert_eq!(
            serialize_filter_value(&Value::Boolean(false)).unwrap(),
            vec!["false"]
        );
    }

    #[test]
    fn array_filter_produces_multiple_values() {
        let value = Value::Array(vec![
            Value::String("Critical".to_owned()),
            Value::String("High".to_owned()),
        ]);

        assert_eq!(
            serialize_filter_value(&value).unwrap(),
            vec!["Critical", "High"]
        );
    }

    #[test]
    fn absolute_next_url_is_accepted_by_parser() {
        let current = Url::parse("https://dojo.example/api/v2/findings/").unwrap();

        let next =
            parse_next_page_url(&current, "https://dojo.example/api/v2/findings/?offset=200")
                .unwrap();

        assert_eq!(
            next.as_str(),
            "https://dojo.example/api/v2/findings/?offset=200"
        );
    }

    #[test]
    fn relative_next_url_is_resolved() {
        let current = Url::parse("https://dojo.example/api/v2/findings/").unwrap();

        let next = parse_next_page_url(&current, "?limit=200&offset=200").unwrap();

        assert_eq!(
            next.as_str(),
            "https://dojo.example/api/v2/findings/?limit=200&offset=200"
        );
    }
}
