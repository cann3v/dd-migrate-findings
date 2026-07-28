use std::collections::HashMap;

use crate::api::models::Finding;
use crate::plan::{ApprovedOperation, MigrationPlan};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DryRunOutcome {
    Ready,
    SourceChanged,
    SourceMissing,
    TargetChanged,
    TargetMissingFromPlan,
}

impl DryRunOutcome {
    pub const ALL: [Self; 5] = [
        Self::Ready,
        Self::SourceChanged,
        Self::SourceMissing,
        Self::TargetChanged,
        Self::TargetMissingFromPlan,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::SourceChanged => "SourceChanged",
            Self::SourceMissing => "SourceMissing",
            Self::TargetChanged => "TargetChanged",
            Self::TargetMissingFromPlan => "TargetMissingFromPlan",
        }
    }
}

#[expect(
    dead_code,
    reason = "prepared status fields will be serialized into PATCH requests at the next stage"
)]
#[derive(Debug, Clone)]
pub struct PreparedFindingPatch {
    pub active: bool,
    pub verified: bool,
    pub fixed: bool,
    pub unfixed: bool,
    pub false_p: bool,
    pub out_of_scope: bool,
    pub is_mitigated: bool,

    pub description: Option<String>,
    pub mitigation: Option<String>,
    pub impact: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DryRunItem {
    pub row_id: String,
    pub source_finding_id: u64,
    pub target_product_id: u64,
    pub target_finding_id: u64,
    pub outcome: DryRunOutcome,
    pub patch: Option<PreparedFindingPatch>,
}

#[derive(Debug)]
pub struct DryRunReport {
    pub items: Vec<DryRunItem>,
}

pub fn build_dry_run_report(
    plan: &MigrationPlan,
    approved_operations: &[ApprovedOperation],
    current_sources: &HashMap<u64, Finding>,
    current_targets: &HashMap<u64, Finding>,
) -> DryRunReport {
    let planned_sources = plan
        .source_findings
        .iter()
        .map(|finding| (finding.id, finding))
        .collect::<HashMap<_, _>>();

    let planned_targets = plan
        .target_findings
        .iter()
        .map(|finding| (finding.id, finding))
        .collect::<HashMap<_, _>>();

    let mut items = Vec::with_capacity(approved_operations.len());

    for operation in approved_operations {
        let Some(planned_source) = planned_sources.get(&operation.source_finding_id) else {
            items.push(item(operation, DryRunOutcome::SourceMissing, None));
            continue;
        };

        let Some(current_source) = current_sources.get(&operation.source_finding_id) else {
            items.push(item(operation, DryRunOutcome::SourceMissing, None));
            continue;
        };

        if *planned_source != current_source {
            items.push(item(operation, DryRunOutcome::SourceChanged, None));
            continue;
        }

        let Some(planned_target) = planned_targets.get(&operation.target_finding_id) else {
            items.push(item(operation, DryRunOutcome::TargetMissingFromPlan, None));
            continue;
        };

        let Some(current_target) = current_targets.get(&operation.target_finding_id) else {
            items.push(item(operation, DryRunOutcome::TargetChanged, None));
            continue;
        };

        if *planned_target != current_target {
            items.push(item(operation, DryRunOutcome::TargetChanged, None));
            continue;
        }

        let patch = prepare_patch(plan, current_source, current_target);

        items.push(item(operation, DryRunOutcome::Ready, Some(patch)));
    }

    DryRunReport { items }
}

fn prepare_patch(plan: &MigrationPlan, source: &Finding, target: &Finding) -> PreparedFindingPatch {
    PreparedFindingPatch {
        active: source.active,
        verified: source.verified,
        fixed: source.fixed,
        unfixed: source.unfixed,
        false_p: source.false_p,
        out_of_scope: source.out_of_scope,
        is_mitigated: source.is_mitigated,

        description: prepare_text_field(
            &plan.source_product.name,
            plan.source_product.id,
            source.id,
            "description",
            Some(source.description.as_str()),
            Some(target.description.as_str()),
        ),

        mitigation: prepare_text_field(
            &plan.source_product.name,
            plan.source_product.id,
            source.id,
            "mitigation",
            source.mitigation.as_deref(),
            target.mitigation.as_deref(),
        ),

        impact: prepare_text_field(
            &plan.source_product.name,
            plan.source_product.id,
            source.id,
            "impact",
            source.impact.as_deref(),
            target.impact.as_deref(),
        ),
    }
}

fn prepare_text_field(
    source_product_name: &str,
    source_product_id: u64,
    source_finding_id: u64,
    field_name: &str,
    source_value: Option<&str>,
    target_value: Option<&str>,
) -> Option<String> {
    let source_value = source_value.unwrap_or_default();
    let target_value = target_value.unwrap_or_default();

    if source_value.is_empty() {
        return None;
    }

    if source_value == target_value {
        return None;
    }

    let marker = format!(
        "[dojo-migrate source-product={source_product_id} \
         source-finding={source_finding_id} \
         field={field_name}]"
    );

    if target_value.contains(&marker) {
        return None;
    }

    let transferred = format!(
        "{marker}\n\
         Перенесено из продукта {source_product_name} \
         [{source_product_id}], finding \
         #{source_finding_id}:\n\
         {source_value}"
    );

    if target_value.is_empty() {
        Some(transferred)
    } else {
        Some(format!("{transferred}\n\n---\n\n{target_value}"))
    }
}

fn item(
    operation: &ApprovedOperation,
    outcome: DryRunOutcome,
    patch: Option<PreparedFindingPatch>,
) -> DryRunItem {
    DryRunItem {
        row_id: operation.row_id.clone(),
        source_finding_id: operation.source_finding_id,
        target_product_id: operation.target_product_id,
        target_finding_id: operation.target_finding_id,
        outcome,
        patch,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use toml::Value;

    use super::*;
    use crate::api::models::Product;
    use crate::plan::MigrationPlan;
    use crate::plan::PLAN_SCHEMA_VERSION;

    fn finding(id: u64) -> Finding {
        Finding {
            id,
            title: "Command Injection".to_owned(),
            found_by: vec![187],
            line: Some(42),
            file_path: Some("src/main.rs".to_owned()),
            component_name: None,
            component_version: None,
            active: true,
            verified: false,
            fixed: false,
            unfixed: false,
            false_p: false,
            duplicate: false,
            out_of_scope: false,
            risk_accepted: false,
            is_mitigated: false,
            description: "target description".to_owned(),
            mitigation: None,
            impact: None,
        }
    }

    fn plan(source: Finding, target: Finding) -> MigrationPlan {
        MigrationPlan {
            schema_version: PLAN_SCHEMA_VERSION,
            created_at_unix_seconds: 1,
            dojo_base_url: "https://dojo.example/".to_owned(),
            source_product: Product {
                id: 2,
                name: "source".to_owned(),
            },
            destination_products: Vec::new(),
            source_filters: BTreeMap::<String, Value>::new(),
            source_findings: vec![source],
            target_findings: vec![target],
            coverage: Vec::new(),
            operations: Vec::new(),
        }
    }

    #[test]
    fn transferred_text_is_added_above_target_text() {
        let mut source = finding(1);
        let target = finding(2);

        source.description = "source description".to_owned();

        let patch = prepare_patch(&plan(source.clone(), target.clone()), &source, &target);

        let description = patch.description.unwrap();

        assert!(description.starts_with(
            "[dojo-migrate source-product=2 \
                 source-finding=1 field=description]"
        ));

        assert!(description.ends_with("target description"));
    }

    #[test]
    fn equal_text_is_not_added() {
        let source = finding(1);
        let target = finding(2);

        let patch = prepare_patch(&plan(source.clone(), target.clone()), &source, &target);

        assert!(patch.description.is_none());
    }

    #[test]
    fn existing_marker_prevents_duplicate_append() {
        let source = finding(1);
        let mut target = finding(2);

        target.description = concat!(
            "[dojo-migrate source-product=2 ",
            "source-finding=1 field=description]\n",
            "already migrated"
        )
        .to_owned();

        let patch = prepare_patch(&plan(source.clone(), target.clone()), &source, &target);

        assert!(patch.description.is_none());
    }
}
