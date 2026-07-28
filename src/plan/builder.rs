use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};

use toml::Value;

use crate::api::models::{Finding, Product};
use crate::error::AppError;
use crate::matching::{CorrelationReport, TargetActionClass};
use crate::plan::models::{
    DecisionAction, MigrationPlan, PLAN_SCHEMA_VERSION, PlannedTargetOperation,
};

pub struct MigrationPlanBuilder {
    dojo_base_url: String,
    source_product: Product,
    source_filters: BTreeMap<String, Value>,
    source_findings: Vec<Finding>,

    destination_products: Vec<Product>,
    target_findings: BTreeMap<u64, Finding>,
    coverage: Vec<crate::matching::SourceCorrelation>,
    operations: Vec<PlannedTargetOperation>,
}

impl MigrationPlanBuilder {
    pub fn new(
        dojo_base_url: String,
        source_product: Product,
        source_filters: BTreeMap<String, Value>,
        source_findings: Vec<Finding>,
    ) -> Self {
        Self {
            dojo_base_url,
            source_product,
            source_filters,
            source_findings,
            destination_products: Vec::new(),
            target_findings: BTreeMap::new(),
            coverage: Vec::new(),
            operations: Vec::new(),
        }
    }

    pub fn add_destination(
        &mut self,
        product: Product,
        target_findings: &[Finding],
        report: CorrelationReport,
    ) -> Result<(), AppError> {
        let findings_by_id = target_findings
            .iter()
            .map(|finding| (finding.id, finding))
            .collect::<HashMap<_, _>>();

        let mut required_target_ids = BTreeSet::new();

        for operation in &report.target_operations {
            required_target_ids.insert(operation.target_finding_id);
        }

        for correlation in &report.sources {
            required_target_ids.extend(correlation.candidate_ids.iter().copied());
        }

        for finding_id in required_target_ids {
            let finding = findings_by_id.get(&finding_id).ok_or_else(|| {
                AppError::PlanInvariant(format!(
                    "target finding #{finding_id} is referenced \
                         by correlation but absent from downloaded data"
                ))
            })?;

            self.target_findings.insert(finding_id, (*finding).clone());
        }

        self.coverage.extend(report.sources);

        self.operations
            .extend(
                report
                    .target_operations
                    .into_iter()
                    .map(|operation| PlannedTargetOperation {
                        source_finding_id: operation.source_finding_id,
                        source_title: operation.source_title,
                        target_product_id: operation.target_product_id,
                        target_finding_id: operation.target_finding_id,
                        class: operation.class,
                        default_action: default_action(operation.class),
                    }),
            );

        self.destination_products.push(product);

        Ok(())
    }

    pub fn build(self) -> MigrationPlan {
        MigrationPlan {
            schema_version: PLAN_SCHEMA_VERSION,
            created_at_unix_seconds: current_unix_seconds(),
            dojo_base_url: self.dojo_base_url,
            source_product: self.source_product,
            destination_products: self.destination_products,
            source_filters: self.source_filters,
            source_findings: self.source_findings,
            target_findings: self.target_findings.into_values().collect(),
            coverage: self.coverage,
            operations: self.operations,
        }
    }
}

fn default_action(class: TargetActionClass) -> DecisionAction {
    match class {
        TargetActionClass::ReadyToApply => DecisionAction::ApplyAll,

        TargetActionClass::AlreadyUpToDate | TargetActionClass::StatusConflict => {
            DecisionAction::Skip
        }
    }
}

fn current_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}
