#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceCoverageClass {
    ExactMatch,
    PossibleMatch,
    Ambiguous,
    NotFound,
    InsufficientData,
}

impl SourceCoverageClass {
    pub const ALL: [Self; 5] = [
        Self::ExactMatch,
        Self::PossibleMatch,
        Self::Ambiguous,
        Self::NotFound,
        Self::InsufficientData,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::ExactMatch => "ExactMatch",
            Self::PossibleMatch => "PossibleMatch",
            Self::Ambiguous => "AmbiguousPossibleMatch",
            Self::NotFound => "NotFound",
            Self::InsufficientData => "InsufficientData",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TargetActionClass {
    ReadyToApply,
    AlreadyUpToDate,
    StatusConflict,
}

impl TargetActionClass {
    pub const ALL: [Self; 3] = [
        Self::ReadyToApply,
        Self::AlreadyUpToDate,
        Self::StatusConflict,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::ReadyToApply => "ReadyToApply",
            Self::AlreadyUpToDate => "AlreadyUpToDate",
            Self::StatusConflict => "StatusConflict",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NotFoundReason {
    DifferentLine,
    DifferentFilePath,
    ScannerMismatch,
    TitleOnlyMatch,
    NoCandidate,
}

impl NotFoundReason {
    pub const ALL: [Self; 5] = [
        Self::DifferentLine,
        Self::DifferentFilePath,
        Self::ScannerMismatch,
        Self::TitleOnlyMatch,
        Self::NoCandidate,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::DifferentLine => "DifferentLine",
            Self::DifferentFilePath => "DifferentFilePath",
            Self::ScannerMismatch => "ScannerMismatch",
            Self::TitleOnlyMatch => "TitleOnlyMatch",
            Self::NoCandidate => "NoCandidate",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SourceCorrelation {
    pub source_finding_id: u64,
    pub source_title: String,
    pub target_product_id: u64,
    pub class: SourceCoverageClass,
    pub candidate_ids: Vec<u64>,
    pub not_found_reasons: Vec<NotFoundReason>,
}

impl SourceCorrelation {
    pub fn candidate_ids_display(&self) -> String {
        const MAX_IDS: usize = 8;

        if self.candidate_ids.is_empty() {
            return "-".to_owned();
        }

        let displayed = self
            .candidate_ids
            .iter()
            .take(MAX_IDS)
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(", ");

        if self.candidate_ids.len() > MAX_IDS {
            format!(
                "{displayed}, ... (+{} more)",
                self.candidate_ids.len() - MAX_IDS
            )
        } else {
            displayed
        }
    }

    pub fn not_found_reasons_display(&self) -> String {
        if self.not_found_reasons.is_empty() {
            return "-".to_owned();
        }

        self.not_found_reasons
            .iter()
            .map(|reason| reason.label())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[derive(Debug, Clone)]
pub struct TargetOperation {
    pub source_finding_id: u64,
    pub source_title: String,
    pub target_product_id: u64,
    pub target_finding_id: u64,
    pub class: TargetActionClass,
}

#[derive(Debug, Clone)]
pub struct CorrelationReport {
    pub sources: Vec<SourceCorrelation>,
    pub target_operations: Vec<TargetOperation>,
}
