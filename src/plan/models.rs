use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use toml::Value;

use crate::api::models::{Finding, Product};
use crate::matching::{SourceCorrelation, TargetActionClass};

pub const PLAN_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationPlan {
    pub schema_version: u32,
    pub created_at_unix_seconds: u64,
    pub dojo_base_url: String,

    pub source_product: Product,
    pub destination_products: Vec<Product>,
    pub source_filters: BTreeMap<String, Value>,

    pub source_findings: Vec<Finding>,
    pub target_findings: Vec<Finding>,

    pub coverage: Vec<SourceCorrelation>,
    pub operations: Vec<PlannedTargetOperation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedTargetOperation {
    pub source_finding_id: u64,
    pub source_title: String,
    pub target_product_id: u64,
    pub target_finding_id: u64,
    pub class: TargetActionClass,
    pub default_action: DecisionAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionAction {
    ApplyAll,
    Skip,
}
