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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
