use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct PaginatedResponse<T> {
    pub count: usize,
    pub next: Option<String>,
    pub results: Vec<T>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Product {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub id: u64,
    pub title: String,

    #[serde(default)]
    pub found_by: Vec<u64>,

    pub line: Option<i64>,
    pub file_path: Option<String>,
    pub component_name: Option<String>,
    pub component_version: Option<String>,

    pub active: bool,
    pub verified: bool,
    pub fixed: bool,
    pub unfixed: bool,
    pub false_p: bool,
    pub duplicate: bool,
    pub out_of_scope: bool,
    pub risk_accepted: bool,
    pub is_mitigated: bool,

    pub description: String,
    pub mitigation: Option<String>,
    pub impact: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FindingToNotes {
    #[serde(default)]
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Note {
    pub id: u64,
    pub author: UserStub,
    pub date: String,
    pub entry: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserStub {
    pub username: String,

    #[serde(default)]
    pub first_name: String,

    #[serde(default)]
    pub last_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FindingPatchRequest {
    pub active: bool,
    pub verified: bool,
    pub fixed: bool,
    pub unfixed: bool,
    pub false_p: bool,
    pub out_of_scope: bool,
    pub is_mitigated: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub mitigation: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub impact: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreateFindingNoteRequest {
    pub entry: String,
    pub private: bool,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn finding_patch_contains_only_transferred_fields() {
        let request = FindingPatchRequest {
            active: false,
            verified: true,
            fixed: false,
            unfixed: false,
            false_p: true,
            out_of_scope: false,
            is_mitigated: true,
            description: Some("transferred description".to_owned()),
            mitigation: None,
            impact: Some("transferred impact".to_owned()),
        };

        let value = serde_json::to_value(request).unwrap();

        assert_eq!(
            value,
            json!({
                "active": false,
                "verified": true,
                "fixed": false,
                "unfixed": false,
                "false_p": true,
                "out_of_scope": false,
                "is_mitigated": true,
                "description": "transferred description",
                "impact": "transferred impact"
            })
        );
    }

    #[test]
    fn absent_text_fields_are_not_serialized() {
        let request = FindingPatchRequest {
            active: true,
            verified: false,
            fixed: false,
            unfixed: false,
            false_p: false,
            out_of_scope: false,
            is_mitigated: false,
            description: None,
            mitigation: None,
            impact: None,
        };

        let value = serde_json::to_value(request).unwrap();

        assert_eq!(
            value,
            json!({
                "active": true,
                "verified": false,
                "fixed": false,
                "unfixed": false,
                "false_p": false,
                "out_of_scope": false,
                "is_mitigated": false
            })
        );
    }

    #[test]
    fn finding_note_request_is_serialized() {
        let request = CreateFindingNoteRequest {
            entry: "Transferred note".to_owned(),
            private: false,
        };

        let value = serde_json::to_value(request).unwrap();

        assert_eq!(
            value,
            json!({
                "entry": "Transferred note",
                "private": false
            })
        );
    }
}
