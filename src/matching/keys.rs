use std::hash::Hash;

use crate::api::models::Finding;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct FileKey {
    scanner_id: u64,
    title: String,
    file_path: String,
    line: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ComponentKey {
    scanner_id: u64,
    title: String,
    component_name: String,
    component_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ComponentBaseKey {
    scanner_id: u64,
    title: String,
    component_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ScannerTitleKey {
    scanner_id: u64,
    title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct TitleKey {
    title: String,
}

pub(crate) fn single_scanner_id(finding: &Finding) -> Option<u64> {
    match finding.found_by.as_slice() {
        [scanner_id] => Some(*scanner_id),
        _ => None,
    }
}

pub(crate) fn file_key(finding: &Finding) -> Option<FileKey> {
    Some(FileKey {
        scanner_id: single_scanner_id(finding)?,
        title: normalized_title(finding),
        file_path: finding.file_path.clone()?,
        line: finding.line?,
    })
}

pub(crate) fn component_key(finding: &Finding) -> Option<ComponentKey> {
    Some(ComponentKey {
        scanner_id: single_scanner_id(finding)?,
        title: normalized_title(finding),
        component_name: finding.component_name.clone()?,
        component_version: finding.component_version.clone(),
    })
}

pub(crate) fn component_base_key(finding: &Finding) -> Option<ComponentBaseKey> {
    Some(ComponentBaseKey {
        scanner_id: single_scanner_id(finding)?,
        title: normalized_title(finding),
        component_name: finding.component_name.clone()?,
    })
}

pub(crate) fn scanner_title_key(finding: &Finding) -> Option<ScannerTitleKey> {
    Some(ScannerTitleKey {
        scanner_id: single_scanner_id(finding)?,
        title: normalized_title(finding),
    })
}

pub(crate) fn title_key(finding: &Finding) -> TitleKey {
    TitleKey {
        title: normalized_title(finding),
    }
}

pub(crate) fn same_location_ignoring_scanner(left: &Finding, right: &Finding) -> bool {
    if normalized_title(left) != normalized_title(right) {
        return false;
    }

    let same_file_location = match (
        left.file_path.as_deref(),
        left.line,
        right.file_path.as_deref(),
        right.line,
    ) {
        (Some(left_path), Some(left_line), Some(right_path), Some(right_line)) => {
            left_path == right_path && left_line == right_line
        }

        _ => false,
    };

    let same_component = match (
        left.component_name.as_deref(),
        left.component_version.as_deref(),
        right.component_name.as_deref(),
        right.component_version.as_deref(),
    ) {
        (Some(left_name), left_version, Some(right_name), right_version) => {
            left_name == right_name && left_version == right_version
        }

        _ => false,
    };

    same_file_location || same_component
}

fn normalized_title(finding: &Finding) -> String {
    finding.title.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding() -> Finding {
        Finding {
            id: 1,
            title: "  Command Injection  ".to_owned(),
            found_by: vec![187],
            line: Some(42),
            file_path: Some("src\\main.rs".to_owned()),
            component_name: Some("example".to_owned()),
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

    #[test]
    fn scanner_must_have_exactly_one_value() {
        let mut finding = finding();

        assert_eq!(single_scanner_id(&finding), Some(187));

        finding.found_by.clear();
        assert_eq!(single_scanner_id(&finding), None);

        finding.found_by = vec![187, 188];
        assert_eq!(single_scanner_id(&finding), None);
    }

    #[test]
    fn title_is_trimmed() {
        let key = file_key(&finding()).unwrap();

        assert_eq!(key.title, "Command Injection");
    }

    #[test]
    fn path_is_not_normalized() {
        let key = file_key(&finding()).unwrap();

        assert_eq!(key.file_path, "src\\main.rs");
    }

    #[test]
    fn scanner_can_be_ignored_for_diagnostics() {
        let left = finding();
        let mut right = finding();

        right.id = 2;
        right.found_by = vec![999];

        assert!(same_location_ignoring_scanner(&left, &right));
    }
}
