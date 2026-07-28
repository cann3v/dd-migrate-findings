#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CorrelationClass {
    ReadyToApply,
    AlreadyUpToDate,
    StatusConflict,
    PossibleMatch,
    Ambiguous,
    NotFound,
    InsufficientData,
}

impl CorrelationClass {
    pub const ALL: [Self; 7] = [
        Self::ReadyToApply,
        Self::AlreadyUpToDate,
        Self::StatusConflict,
        Self::PossibleMatch,
        Self::Ambiguous,
        Self::NotFound,
        Self::InsufficientData,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::ReadyToApply => "ReadyToApply",
            Self::AlreadyUpToDate => "AlreadyUpToDate",
            Self::StatusConflict => "StatusConflict",
            Self::PossibleMatch => "PossibleMatch",
            Self::Ambiguous => "Ambiguous",
            Self::NotFound => "NotFound",
            Self::InsufficientData => "InsufficientData",
        }
    }
}

#[derive(Debug, Clone)]
pub struct CorrelationResult {
    pub source_finding_id: u64,
    pub source_title: String,
    pub target_product_id: u64,
    pub class: CorrelationClass,
    pub candidate_ids: Vec<u64>,
}

impl CorrelationResult {
    pub fn candidate_ids_display(&self) -> String {
        if self.candidate_ids.is_empty() {
            return "-".to_owned();
        }

        self.candidate_ids
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}
