use thiserror::Error;

pub type Result<T> = std::result::Result<T, V2Error>;

#[derive(Debug, Error)]
pub enum V2Error {
    #[error("invalid dimension: expected {expected}, got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },
    #[error("dimension must be at least two")]
    InvalidDimension,
    #[error("sample budget is below the contract minimum")]
    InvalidSampleBudget,
    #[error("coordinate contains a non-finite value")]
    NonFiniteCoordinate,
    #[error("coordinate norm is zero")]
    ZeroCoordinate,
    #[error("vector contains a non-finite value: {0}")]
    NonFiniteVector(&'static str),
    #[error("invalid tangent vector: {0}")]
    InvalidTangent(String),
    #[error("operation reaches the spherical cut locus")]
    CutLocus,
    #[error("invalid field version: {0}")]
    InvalidVersion(String),
    #[error("invalid lifecycle for operation")]
    InvalidLifecycle,
    #[error("resolution is infeasible for the current state")]
    ResolutionInfeasible,
    #[error("kernel normalization did not converge")]
    KernelUnresolved,
    #[error("continuous cubature did not converge: {0}")]
    CubatureUnresolved(String),
    #[error("continuous ray reference did not converge: {0}")]
    RayReferenceUnresolved(String),
    #[error("sample directions are numerically degenerate")]
    SampleDegenerate,
    #[error("sample field is structurally invalid: {0}")]
    InvalidSampleField(String),
    #[error("transport is unresolved: {0}")]
    TransportUnresolved(String),
    #[error("response refinement is unresolved: {0}")]
    ResponseUnresolved(String),
    #[error("edge flux is inconsistent with the owner state")]
    InconsistentFlux,
    #[error("absorption is inconsistent with the owner state")]
    InconsistentAbsorption,
    #[error("source budget must be finite and in (0, 1]")]
    InvalidBudget,
    #[error("response cannot be closed because raw absorption is zero")]
    NoEffectiveAbsorption,
    #[error("persistence error: {0}")]
    Persistence(String),
    #[error("serialization error: {0}")]
    Serialization(String),
}
