use std::collections::{BTreeMap, HashMap};

use crate::api::models::Finding;
use crate::matching::keys::{
    ComponentBaseKey, ComponentKey, FileKey, component_base_key, component_key, file_key,
};
use crate::matching::models::{CorrelationClass, CorrelationResult};

pub fn correlate_findings(
    source_findings: &[Finding],
    target_findings: &[Finding],
    target_product_id: u64,
) -> Vec<CorrelationResult> {
    let index = TargetIndex::new(target_findings);

    source_findings
        .iter()
        .map(|source| correlate_one(source, &index, target_product_id))
        .collect()
}

struct TargetIndex<'a> {
    by_file_key: HashMap<FileKey, Vec<&'a Finding>>,
    by_component_key: HashMap<ComponentKey, Vec<&'a Finding>>,
    by_component_base_key: HashMap<ComponentBaseKey, Vec<&'a Finding>>,
}

impl<'a> TargetIndex<'a> {
    fn new(findings: &'a [Finding]) -> Self {
        let mut index = Self {
            by_file_key: HashMap::new(),
            by_component_key: HashMap::new(),
            by_component_base_key: HashMap::new(),
        };

        for finding in findings {
            if let Some(key) = file_key(finding) {
                index.by_file_key.entry(key).or_default().push(finding);
            }

            if let Some(key) = component_key(finding) {
                index.by_component_key.entry(key).or_default().push(finding);
            }

            if let Some(key) = component_base_key(finding) {
                index
                    .by_component_base_key
                    .entry(key)
                    .or_default()
                    .push(finding);
            }
        }

        index
    }

    fn exact_candidates(&self, source: &Finding) -> BTreeMap<u64, &'a Finding> {
        let mut candidates = BTreeMap::new();

        if let Some(key) = file_key(source)
            && let Some(findings) = self.by_file_key.get(&key)
        {
            for &finding in findings {
                candidates.insert(finding.id, finding);
            }
        }

        if let Some(key) = component_key(source)
            && let Some(findings) = self.by_component_key.get(&key)
        {
            for &finding in findings {
                candidates.insert(finding.id, finding);
            }
        }

        candidates
    }

    fn possible_component_candidates(&self, source: &Finding) -> BTreeMap<u64, &'a Finding> {
        let mut candidates = BTreeMap::new();

        let Some(key) = component_base_key(source) else {
            return candidates;
        };

        let Some(findings) = self.by_component_base_key.get(&key) else {
            return candidates;
        };

        for &finding in findings {
            if source.component_version.as_deref() != finding.component_version.as_deref() {
                candidates.insert(finding.id, finding);
            }
        }

        candidates
    }
}

fn correlate_one(
    source: &Finding,
    index: &TargetIndex<'_>,
    target_product_id: u64,
) -> CorrelationResult {
    let has_file_key = file_key(source).is_some();
    let has_component_key = component_key(source).is_some();

    if !has_file_key && !has_component_key {
        return result(
            source,
            target_product_id,
            CorrelationClass::InsufficientData,
            Vec::new(),
        );
    }

    let exact_candidates = index.exact_candidates(source);

    if exact_candidates.len() > 1 {
        return result(
            source,
            target_product_id,
            CorrelationClass::Ambiguous,
            exact_candidates.keys().copied().collect(),
        );
    }

    if let Some(target) = exact_candidates.values().next() {
        let class = classify_status(source, target);

        return result(source, target_product_id, class, vec![target.id]);
    }

    let possible_candidates = index.possible_component_candidates(source);

    if !possible_candidates.is_empty() {
        return result(
            source,
            target_product_id,
            CorrelationClass::PossibleMatch,
            possible_candidates.keys().copied().collect(),
        );
    }

    result(
        source,
        target_product_id,
        CorrelationClass::NotFound,
        Vec::new(),
    )
}

fn classify_status(source: &Finding, target: &Finding) -> CorrelationClass {
    if transferred_status(source) == transferred_status(target)
        && source.duplicate == target.duplicate
        && source.risk_accepted == target.risk_accepted
    {
        return CorrelationClass::AlreadyUpToDate;
    }

    if is_untriaged_active(target) {
        CorrelationClass::ReadyToApply
    } else {
        CorrelationClass::StatusConflict
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TransferStatus {
    active: bool,
    verified: bool,
    fixed: bool,
    unfixed: bool,
    false_p: bool,
    out_of_scope: bool,
    is_mitigated: bool,
}

fn transferred_status(finding: &Finding) -> TransferStatus {
    TransferStatus {
        active: finding.active,
        verified: finding.verified,
        fixed: finding.fixed,
        unfixed: finding.unfixed,
        false_p: finding.false_p,
        out_of_scope: finding.out_of_scope,
        is_mitigated: finding.is_mitigated,
    }
}

fn is_untriaged_active(finding: &Finding) -> bool {
    finding.active
        && !finding.verified
        && !finding.false_p
        && !finding.duplicate
        && !finding.out_of_scope
        && !finding.risk_accepted
        && !finding.is_mitigated
}

fn result(
    source: &Finding,
    target_product_id: u64,
    class: CorrelationClass,
    candidate_ids: Vec<u64>,
) -> CorrelationResult {
    CorrelationResult {
        source_finding_id: source.id,
        source_title: source.title.clone(),
        target_product_id,
        class,
        candidate_ids,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(id: u64) -> Finding {
        Finding {
            id,
            title: "Command Injection".to_owned(),
            found_by: vec![187],
            line: Some(42),
            file_path: Some("src/main.rs".to_owned()),
            component_name: Some("library".to_owned()),
            component_version: Some("1.0".to_owned()),
            active: true,
            verified: false,
            fixed: false,
            unfixed: false,
            false_p: false,
            duplicate: false,
            out_of_scope: false,
            risk_accepted: false,
            is_mitigated: false,
            description: String::new(),
            mitigation: None,
            impact: None,
        }
    }

    fn false_positive(id: u64) -> Finding {
        let mut finding = finding(id);

        finding.active = false;
        finding.false_p = true;

        finding
    }

    #[test]
    fn active_target_is_ready_to_apply() {
        let source = false_positive(1);
        let target = finding(2);

        let results = correlate_findings(&[source], &[target], 4);

        assert_eq!(results[0].class, CorrelationClass::ReadyToApply);
        assert_eq!(results[0].candidate_ids, vec![2]);
    }

    #[test]
    fn equal_status_is_already_up_to_date() {
        let source = false_positive(1);
        let target = false_positive(2);

        let results = correlate_findings(&[source], &[target], 4);

        assert_eq!(results[0].class, CorrelationClass::AlreadyUpToDate);
    }

    #[test]
    fn triaged_different_target_is_conflict() {
        let source = false_positive(1);
        let mut target = finding(2);

        target.verified = true;

        let results = correlate_findings(&[source], &[target], 4);

        assert_eq!(results[0].class, CorrelationClass::StatusConflict);
    }

    #[test]
    fn different_component_version_is_possible_match() {
        let mut source = false_positive(1);
        let mut target = finding(2);

        source.file_path = None;
        source.line = None;
        source.component_version = Some("1.0".to_owned());

        target.file_path = None;
        target.line = None;
        target.component_version = Some("2.0".to_owned());

        let results = correlate_findings(&[source], &[target], 4);

        assert_eq!(results[0].class, CorrelationClass::PossibleMatch);
        assert_eq!(results[0].candidate_ids, vec![2]);
    }

    #[test]
    fn version_missing_on_one_side_is_possible_match() {
        let mut source = false_positive(1);
        let mut target = finding(2);

        source.file_path = None;
        source.line = None;
        source.component_version = None;

        target.file_path = None;
        target.line = None;
        target.component_version = Some("2.0".to_owned());

        let results = correlate_findings(&[source], &[target], 4);

        assert_eq!(results[0].class, CorrelationClass::PossibleMatch);
    }

    #[test]
    fn version_missing_on_both_sides_is_exact_match() {
        let mut source = false_positive(1);
        let mut target = finding(2);

        source.file_path = None;
        source.line = None;
        source.component_version = None;

        target.file_path = None;
        target.line = None;
        target.component_version = None;

        let results = correlate_findings(&[source], &[target], 4);

        assert_eq!(results[0].class, CorrelationClass::ReadyToApply);
    }

    #[test]
    fn scanner_is_mandatory() {
        let mut source = false_positive(1);
        let target = finding(2);

        source.found_by.clear();

        let results = correlate_findings(&[source], &[target], 4);

        assert_eq!(results[0].class, CorrelationClass::InsufficientData);
    }

    #[test]
    fn scanner_must_match() {
        let source = false_positive(1);
        let mut target = finding(2);

        target.found_by = vec![999];

        let results = correlate_findings(&[source], &[target], 4);

        assert_eq!(results[0].class, CorrelationClass::NotFound);
    }

    #[test]
    fn multiple_exact_candidates_are_ambiguous() {
        let source = false_positive(1);
        let first_target = finding(2);
        let second_target = finding(3);

        let results = correlate_findings(&[source], &[first_target, second_target], 4);

        assert_eq!(results[0].class, CorrelationClass::Ambiguous);
        assert_eq!(results[0].candidate_ids, vec![2, 3]);
    }

    #[test]
    fn same_target_found_by_both_branches_is_not_ambiguous() {
        let source = false_positive(1);
        let target = finding(2);

        let results = correlate_findings(&[source], &[target], 4);

        assert_eq!(results[0].class, CorrelationClass::ReadyToApply);
        assert_eq!(results[0].candidate_ids, vec![2]);
    }
}
