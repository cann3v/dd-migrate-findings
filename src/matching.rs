mod engine;
mod keys;
mod models;

pub use engine::correlate_findings;
pub use models::{
    CorrelationReport, NotFoundReason, SourceCorrelation, SourceCoverageClass, TargetActionClass,
    TargetOperation,
};
