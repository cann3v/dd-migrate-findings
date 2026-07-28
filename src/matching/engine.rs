use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::api::models::Finding;
use crate::matching::keys::{
    ComponentBaseKey, ComponentKey, FileKey, ScannerTitleKey, TitleKey, component_base_key,
    component_key, file_key, same_location_ignoring_scanner, scanner_title_key, title_key,
};
use crate::matching::models::{
    CorrelationReport, NotFoundReason, SourceCorrelation, SourceCoverageClass, TargetActionClass,
    TargetOperation,
};

pub fn correlate_findings(
    source_findings: &[Finding],
    target_findings: &[Finding],
    target_product_id: u64,
) -> CorrelationReport {
    let index = TargetIndex::new(target_findings);

    let mut sources = Vec::with_capacity(source_findings.len());
    let mut target_operations = Vec::new();

    for source in source_findings {
        correlate_one(
            source,
            &index,
            target_product_id,
            &mut sources,
            &mut target_operations,
        );
    }

    CorrelationReport {
        sources,
        target_operations,
    }
}

struct TargetIndex<'a> {
    by_file_key: HashMap<FileKey, Vec<&'a Finding>>,
    by_component_key: HashMap<ComponentKey, Vec<&'a Finding>>,
    by_component_base_key: HashMap<ComponentBaseKey, Vec<&'a Finding>>,
    by_scanner_title: HashMap<ScannerTitleKey, Vec<&'a Finding>>,
    by_title: HashMap<TitleKey, Vec<&'a Finding>>,
}

impl<'a> TargetIndex<'a> {
    fn new(findings: &'a [Finding]) -> Self {
        let mut index = Self {
            by_file_key: HashMap::new(),
            by_component_key: HashMap::new(),
            by_component_base_key: HashMap::new(),
            by_scanner_title: HashMap::new(),
            by_title: HashMap::new(),
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

            if let Some(key) = scanner_title_key(finding) {
                index.by_scanner_title.entry(key).or_default().push(finding);
            }

            index
                .by_title
                .entry(title_key(finding))
                .or_default()
                .push(finding);
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

    fn diagnose_not_found(&self, source: &Finding) -> Vec<NotFoundReason> {
        let mut reasons = BTreeSet::new();

        let same_scanner_and_title = scanner_title_key(source)
            .and_then(|key| self.by_scanner_title.get(&key))
            .map(Vec::as_slice)
            .unwrap_or_default();

        if let Some(source_path) = source.file_path.as_deref() {
            for target in same_scanner_and_title {
                let Some(target_path) = target.file_path.as_deref() else {
                    continue;
                };

                if source_path == target_path && source.line != target.line {
                    reasons.insert(NotFoundReason::DifferentLine);
                }

                if source_path != target_path {
                    reasons.insert(NotFoundReason::DifferentFilePath);
                }
            }
        }

        let same_title = self
            .by_title
            .get(&title_key(source))
            .map(Vec::as_slice)
            .unwrap_or_default();

        let source_scanner = source.found_by.as_slice();

        if same_title.iter().any(|target| {
            target.found_by.as_slice() != source_scanner
                && same_location_ignoring_scanner(source, target)
        }) {
            reasons.insert(NotFoundReason::ScannerMismatch);
        }

        if reasons.is_empty() && !same_title.is_empty() {
            reasons.insert(NotFoundReason::TitleOnlyMatch);
        }

        if reasons.is_empty() {
            reasons.insert(NotFoundReason::NoCandidate);
        }

        reasons.into_iter().collect()
    }
}

fn correlate_one(
    source: &Finding,
    index: &TargetIndex<'_>,
    target_product_id: u64,
    sources: &mut Vec<SourceCorrelation>,
    target_operations: &mut Vec<TargetOperation>,
) {
    let has_file_key = file_key(source).is_some();
    let has_component_key = component_key(source).is_some();

    if !has_file_key && !has_component_key {
        sources.push(source_result(
            source,
            target_product_id,
            SourceCoverageClass::InsufficientData,
            Vec::new(),
            Vec::new(),
        ));

        return;
    }

    let exact_candidates = index.exact_candidates(source);

    if !exact_candidates.is_empty() {
        let candidate_ids = exact_candidates.keys().copied().collect();

        for target in exact_candidates.values() {
            target_operations.push(TargetOperation {
                source_finding_id: source.id,
                source_title: source.title.clone(),
                target_product_id,
                target_finding_id: target.id,
                class: classify_status(source, target),
            });
        }

        sources.push(source_result(
            source,
            target_product_id,
            SourceCoverageClass::ExactMatch,
            candidate_ids,
            Vec::new(),
        ));

        return;
    }

    let possible_candidates = index.possible_component_candidates(source);

    if !possible_candidates.is_empty() {
        let class = if possible_candidates.len() == 1 {
            SourceCoverageClass::PossibleMatch
        } else {
            SourceCoverageClass::Ambiguous
        };

        sources.push(source_result(
            source,
            target_product_id,
            class,
            possible_candidates.keys().copied().collect(),
            Vec::new(),
        ));

        return;
    }

    sources.push(source_result(
        source,
        target_product_id,
        SourceCoverageClass::NotFound,
        Vec::new(),
        index.diagnose_not_found(source),
    ));
}

fn classify_status(source: &Finding, target: &Finding) -> TargetActionClass {
    if transferred_status(source) == transferred_status(target)
        && source.duplicate == target.duplicate
        && source.risk_accepted == target.risk_accepted
    {
        return TargetActionClass::AlreadyUpToDate;
    }

    if is_untriaged_active(target) {
        TargetActionClass::ReadyToApply
    } else {
        TargetActionClass::StatusConflict
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

fn source_result(
    source: &Finding,
    target_product_id: u64,
    class: SourceCoverageClass,
    candidate_ids: Vec<u64>,
    not_found_reasons: Vec<NotFoundReason>,
) -> SourceCorrelation {
    SourceCorrelation {
        source_finding_id: source.id,
        source_title: source.title.clone(),
        target_product_id,
        class,
        candidate_ids,
        not_found_reasons,
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
        finding.is_mitigated = true;

        finding
    }

    #[test]
    fn multiple_exact_targets_become_separate_operations() {
        let source = false_positive(1);
        let first_target = finding(2);
        let second_target = false_positive(3);

        let report = correlate_findings(&[source], &[first_target, second_target], 4);

        assert_eq!(report.sources[0].class, SourceCoverageClass::ExactMatch);

        assert_eq!(report.sources[0].candidate_ids, vec![2, 3]);

        assert_eq!(report.target_operations.len(), 2);

        assert_eq!(
            report.target_operations[0].class,
            TargetActionClass::ReadyToApply
        );

        assert_eq!(
            report.target_operations[1].class,
            TargetActionClass::AlreadyUpToDate
        );
    }

    #[test]
    fn triaged_different_target_is_conflict() {
        let source = false_positive(1);
        let mut target = finding(2);

        target.active = false;

        let report = correlate_findings(&[source], &[target], 4);

        assert_eq!(
            report.target_operations[0].class,
            TargetActionClass::StatusConflict
        );
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

        let report = correlate_findings(&[source], &[target], 4);

        assert_eq!(report.sources[0].class, SourceCoverageClass::PossibleMatch);

        assert_eq!(report.sources[0].candidate_ids, vec![2]);
    }

    #[test]
    fn multiple_possible_matches_remain_ambiguous() {
        let mut source = false_positive(1);
        let mut first_target = finding(2);
        let mut second_target = finding(3);

        source.file_path = None;
        source.line = None;
        source.component_version = Some("1.0".to_owned());

        first_target.file_path = None;
        first_target.line = None;
        first_target.component_version = Some("2.0".to_owned());

        second_target.file_path = None;
        second_target.line = None;
        second_target.component_version = Some("3.0".to_owned());

        let report = correlate_findings(&[source], &[first_target, second_target], 4);

        assert_eq!(report.sources[0].class, SourceCoverageClass::Ambiguous);

        assert_eq!(report.sources[0].candidate_ids, vec![2, 3]);
    }

    #[test]
    fn missing_version_on_both_sides_is_exact() {
        let mut source = false_positive(1);
        let mut target = finding(2);

        source.file_path = None;
        source.line = None;
        source.component_version = None;

        target.file_path = None;
        target.line = None;
        target.component_version = None;

        let report = correlate_findings(&[source], &[target], 4);

        assert_eq!(report.sources[0].class, SourceCoverageClass::ExactMatch);
    }

    #[test]
    fn empty_found_by_is_insufficient_data() {
        let mut source = false_positive(1);

        source.found_by.clear();

        let report = correlate_findings(&[source], &[], 4);

        assert_eq!(
            report.sources[0].class,
            SourceCoverageClass::InsufficientData
        );
    }

    #[test]
    fn different_line_is_explained() {
        let mut source = false_positive(1);
        let mut target = finding(2);

        source.component_name = None;
        source.component_version = None;

        target.component_name = None;
        target.component_version = None;
        target.line = Some(43);

        let report = correlate_findings(&[source], &[target], 4);

        assert_eq!(report.sources[0].class, SourceCoverageClass::NotFound);

        assert_eq!(
            report.sources[0].not_found_reasons,
            vec![NotFoundReason::DifferentLine]
        );
    }

    #[test]
    fn scanner_mismatch_is_explained() {
        let source = false_positive(1);
        let mut target = finding(2);

        target.found_by = vec![999];

        let report = correlate_findings(&[source], &[target], 4);

        assert_eq!(
            report.sources[0].not_found_reasons,
            vec![NotFoundReason::ScannerMismatch]
        );
    }

    #[test]
    fn completely_absent_finding_has_no_candidate() {
        let source = false_positive(1);

        let report = correlate_findings(&[source], &[], 4);

        assert_eq!(
            report.sources[0].not_found_reasons,
            vec![NotFoundReason::NoCandidate]
        );
    }
}
