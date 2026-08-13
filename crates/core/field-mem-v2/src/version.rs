//! Frozen v2 backend and embedding identities.

use serde::{Deserialize, Serialize};

use crate::{
    error::{Result, V2Error},
    numeric::SIGMA,
};

pub const STATE_SCHEMA_VERSION: u32 = 2;
pub const ARTIFACT_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_SAMPLE_BUDGET: u32 = 64;
pub const MIN_SAMPLE_BUDGET: u32 = 4;

pub const ALGORITHM_ID: &str = "fm-v2c-wendland-residual-edge-fvm-v2";
pub const SCALAR_ID: &str = "f64";
pub const MEASURE_ID: &str = "sphere_normalized";
pub const KERNEL_ID: &str = "wendland_c2_log_normalized_v2";
pub const PROJECTION_ARITHMETIC_ID: &str = "f64_index_kahan_v1";
pub const SAMPLE_PROJECTION_ID: &str = "residual_greedy_controls_v2";
pub const GRAPH_ID: &str = "endpoint_radial_finite_v1";
pub const GAUGE_ID: &str = "density_label_frame_v1";

pub const PHYSICS_REFERENCE_DIMENSION: u32 = 3;
pub const SEMANTIC_PRODUCTION_DIMENSION: u32 = 384;

pub const FROZEN_EMBEDDING_MODEL: &str = "bge-m3:latest";
pub const FROZEN_EMBEDDING_MODEL_DIGEST: &str =
    "7907646426070047a77226ac3e684fbbe8410524f7b4a74d02837e43f2146bab";
pub const FROZEN_DATASET_SHA256: &str =
    "ed5a9836f277ea8242fce0bd477363bb21e81d606226dc607b92554d7b7d9901";
pub const FROZEN_SOURCE_DIMENSION: u32 = 1024;
pub const FROZEN_PROJECTION_FAMILY: &str = "uncentered_spherical_pca_eigh_v1";
pub const FROZEN_PROJECTION_ARCHIVE_SHA256: &str =
    "2ba52a192937c9d4f8b6ab34980cf733539c9417af1b4db7c5c9ae636d80dd68";
pub const FROZEN_PROJECTION_CONTENT_SHA256: &str =
    "17f2932d84ce59107e1f080ddd9279e30390fc6822f41a89f75b72a5cd8cbc11";
pub const FROZEN_OLLAMA_VERSION: &str = "0.21.2";

/// The two backend implementations are separate, non-interchangeable objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    PhysicsReference = 0,
    SemanticProduction = 1,
}

impl BackendKind {
    /// Stable wire tag used by the canonical binary codec.
    pub const fn wire_tag(self) -> u8 {
        self as u8
    }
}

/// The ambient direction space associated with a backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub enum SpaceKind {
    PhysicsReferenceS2 = 0,
    #[serde(rename = "semantic_production_384")]
    SemanticProduction384 = 1,
}

impl SpaceKind {
    /// Stable wire tag used by the canonical binary codec.
    pub const fn wire_tag(self) -> u8 {
        self as u8
    }
}

/// The full, frozen semantic-coordinate provenance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingIdentity {
    pub model: String,
    pub model_digest: String,
    pub dataset_sha256: String,
    pub source_dimension: u32,
    pub target_dimension: u32,
    pub projection_family: String,
    pub projection_archive_sha256: String,
    pub projection_content_sha256: String,
    pub ollama_version: String,
}

impl EmbeddingIdentity {
    /// The only identity accepted by semantic-production v2 fields.
    pub fn frozen_semantic_384() -> Self {
        Self {
            model: FROZEN_EMBEDDING_MODEL.to_owned(),
            model_digest: FROZEN_EMBEDDING_MODEL_DIGEST.to_owned(),
            dataset_sha256: FROZEN_DATASET_SHA256.to_owned(),
            source_dimension: FROZEN_SOURCE_DIMENSION,
            target_dimension: SEMANTIC_PRODUCTION_DIMENSION,
            projection_family: FROZEN_PROJECTION_FAMILY.to_owned(),
            projection_archive_sha256: FROZEN_PROJECTION_ARCHIVE_SHA256.to_owned(),
            projection_content_sha256: FROZEN_PROJECTION_CONTENT_SHA256.to_owned(),
            ollama_version: FROZEN_OLLAMA_VERSION.to_owned(),
        }
    }

    /// Requires exact equality for every frozen provenance component.
    pub fn validate(&self) -> Result<()> {
        let frozen = Self::frozen_semantic_384();
        validate_exact_string("embedding model", &self.model, &frozen.model)?;
        validate_exact_string(
            "embedding model digest",
            &self.model_digest,
            &frozen.model_digest,
        )?;
        validate_exact_string(
            "embedding dataset SHA-256",
            &self.dataset_sha256,
            &frozen.dataset_sha256,
        )?;
        validate_exact_u32(
            "embedding source dimension",
            self.source_dimension,
            frozen.source_dimension,
        )?;
        validate_exact_u32(
            "embedding target dimension",
            self.target_dimension,
            frozen.target_dimension,
        )?;
        validate_exact_string(
            "embedding projection family",
            &self.projection_family,
            &frozen.projection_family,
        )?;
        validate_exact_string(
            "embedding projection archive SHA-256",
            &self.projection_archive_sha256,
            &frozen.projection_archive_sha256,
        )?;
        validate_exact_string(
            "embedding projection content SHA-256",
            &self.projection_content_sha256,
            &frozen.projection_content_sha256,
        )?;
        validate_exact_string(
            "embedding Ollama version",
            &self.ollama_version,
            &frozen.ollama_version,
        )
    }
}

/// All frozen choices that identify a persistent v2 field.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldVersion {
    pub state_schema_version: u32,
    pub algorithm_id: String,
    pub scalar: String,
    pub dimension: u32,
    pub sample_budget: u32,
    pub backend_kind: BackendKind,
    pub space_kind: SpaceKind,
    pub measure: String,
    pub kernel_id: String,
    pub projection_arithmetic: String,
    pub sample_projection_id: String,
    pub graph_id: String,
    pub gauge_id: String,
    pub sigma: f64,
    pub dataset_sha256: Option<String>,
    pub embedding_identity: Option<EmbeddingIdentity>,
}

impl FieldVersion {
    /// Constructs the only valid continuous S² reference version for `K`.
    pub fn physics_reference_s2(sample_budget: u32) -> Result<Self> {
        let version = Self::with_common_fields(
            PHYSICS_REFERENCE_DIMENSION,
            sample_budget,
            BackendKind::PhysicsReference,
            SpaceKind::PhysicsReferenceS2,
            None,
            None,
        );
        version.validate()?;
        Ok(version)
    }

    /// Constructs the only valid frozen semantic-production 384D version for `K`.
    pub fn semantic_production_384(sample_budget: u32) -> Result<Self> {
        let identity = EmbeddingIdentity::frozen_semantic_384();
        let version = Self::with_common_fields(
            SEMANTIC_PRODUCTION_DIMENSION,
            sample_budget,
            BackendKind::SemanticProduction,
            SpaceKind::SemanticProduction384,
            Some(identity.dataset_sha256.clone()),
            Some(identity),
        );
        version.validate()?;
        Ok(version)
    }

    /// Validates every versioned constant and the only two allowed identities.
    pub fn validate(&self) -> Result<()> {
        validate_exact_u32(
            "state schema version",
            self.state_schema_version,
            STATE_SCHEMA_VERSION,
        )?;
        validate_exact_string("algorithm id", &self.algorithm_id, ALGORITHM_ID)?;
        validate_exact_string("scalar", &self.scalar, SCALAR_ID)?;
        if self.dimension < 2 {
            return Err(V2Error::InvalidDimension);
        }
        if self.sample_budget < MIN_SAMPLE_BUDGET {
            return Err(V2Error::InvalidSampleBudget);
        }
        validate_exact_string("measure", &self.measure, MEASURE_ID)?;
        validate_exact_string("kernel id", &self.kernel_id, KERNEL_ID)?;
        validate_exact_string(
            "projection arithmetic",
            &self.projection_arithmetic,
            PROJECTION_ARITHMETIC_ID,
        )?;
        validate_exact_string(
            "sample projection id",
            &self.sample_projection_id,
            SAMPLE_PROJECTION_ID,
        )?;
        validate_exact_string("graph id", &self.graph_id, GRAPH_ID)?;
        validate_exact_string("gauge id", &self.gauge_id, GAUGE_ID)?;
        if self.sigma.to_bits() != SIGMA.to_bits() {
            return Err(V2Error::InvalidVersion(
                "sigma does not match the frozen 1/pi bit pattern".to_owned(),
            ));
        }

        match (self.backend_kind, self.space_kind) {
            (BackendKind::PhysicsReference, SpaceKind::PhysicsReferenceS2) => {
                validate_exact_u32(
                    "physics-reference dimension",
                    self.dimension,
                    PHYSICS_REFERENCE_DIMENSION,
                )?;
                if self.dataset_sha256.is_some() || self.embedding_identity.is_some() {
                    return Err(V2Error::InvalidVersion(
                        "physics reference must not carry semantic identity".to_owned(),
                    ));
                }
            }
            (BackendKind::SemanticProduction, SpaceKind::SemanticProduction384) => {
                validate_exact_u32(
                    "semantic-production dimension",
                    self.dimension,
                    SEMANTIC_PRODUCTION_DIMENSION,
                )?;
                let identity = self.embedding_identity.as_ref().ok_or_else(|| {
                    V2Error::InvalidVersion(
                        "semantic production requires frozen embedding identity".to_owned(),
                    )
                })?;
                identity.validate()?;
                let dataset_sha256 = self.dataset_sha256.as_deref().ok_or_else(|| {
                    V2Error::InvalidVersion(
                        "semantic production requires dataset SHA-256".to_owned(),
                    )
                })?;
                validate_exact_string(
                    "field dataset SHA-256",
                    dataset_sha256,
                    FROZEN_DATASET_SHA256,
                )?;
                validate_exact_string(
                    "field/embedding dataset SHA-256",
                    dataset_sha256,
                    &identity.dataset_sha256,
                )?;
            }
            _ => {
                return Err(V2Error::InvalidVersion(
                    "backend and space identities are an invalid combination".to_owned(),
                ));
            }
        }
        Ok(())
    }

    fn with_common_fields(
        dimension: u32,
        sample_budget: u32,
        backend_kind: BackendKind,
        space_kind: SpaceKind,
        dataset_sha256: Option<String>,
        embedding_identity: Option<EmbeddingIdentity>,
    ) -> Self {
        Self {
            state_schema_version: STATE_SCHEMA_VERSION,
            algorithm_id: ALGORITHM_ID.to_owned(),
            scalar: SCALAR_ID.to_owned(),
            dimension,
            sample_budget,
            backend_kind,
            space_kind,
            measure: MEASURE_ID.to_owned(),
            kernel_id: KERNEL_ID.to_owned(),
            projection_arithmetic: PROJECTION_ARITHMETIC_ID.to_owned(),
            sample_projection_id: SAMPLE_PROJECTION_ID.to_owned(),
            graph_id: GRAPH_ID.to_owned(),
            gauge_id: GAUGE_ID.to_owned(),
            sigma: SIGMA,
            dataset_sha256,
            embedding_identity,
        }
    }
}

fn validate_exact_string(name: &str, actual: &str, expected: &str) -> Result<()> {
    if actual == expected {
        Ok(())
    } else {
        Err(V2Error::InvalidVersion(format!(
            "{name} does not match the frozen identity"
        )))
    }
}

fn validate_exact_u32(name: &str, actual: u32, expected: u32) -> Result<()> {
    if actual == expected {
        Ok(())
    } else {
        Err(V2Error::InvalidVersion(format!(
            "{name} does not match the frozen identity"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_and_semantic_constructors_are_the_only_valid_identity_shapes() {
        let reference =
            FieldVersion::physics_reference_s2(MIN_SAMPLE_BUDGET).expect("reference version");
        assert_eq!(reference.backend_kind, BackendKind::PhysicsReference);
        assert_eq!(reference.space_kind, SpaceKind::PhysicsReferenceS2);
        assert_eq!(reference.dimension, PHYSICS_REFERENCE_DIMENSION);
        assert_eq!(reference.dataset_sha256, None);
        assert_eq!(reference.embedding_identity, None);
        reference.validate().expect("validate reference");

        let semantic =
            FieldVersion::semantic_production_384(DEFAULT_SAMPLE_BUDGET).expect("semantic version");
        assert_eq!(semantic.backend_kind, BackendKind::SemanticProduction);
        assert_eq!(semantic.space_kind, SpaceKind::SemanticProduction384);
        assert_eq!(semantic.dimension, SEMANTIC_PRODUCTION_DIMENSION);
        assert_eq!(
            semantic.dataset_sha256.as_deref(),
            Some(FROZEN_DATASET_SHA256)
        );
        semantic.validate().expect("validate semantic");
    }

    #[test]
    fn semantic_identity_rejects_any_mixed_or_unfrozen_field() {
        let mut version =
            FieldVersion::semantic_production_384(DEFAULT_SAMPLE_BUDGET).expect("semantic version");
        version
            .embedding_identity
            .as_mut()
            .expect("semantic identity")
            .projection_content_sha256 = "different".to_owned();
        assert!(matches!(
            version.validate(),
            Err(V2Error::InvalidVersion(_))
        ));

        let mut dataset_mismatch =
            FieldVersion::semantic_production_384(DEFAULT_SAMPLE_BUDGET).expect("semantic version");
        dataset_mismatch.dataset_sha256 = Some("different".to_owned());
        assert!(matches!(
            dataset_mismatch.validate(),
            Err(V2Error::InvalidVersion(_))
        ));
    }

    #[test]
    fn version_rejects_invalid_budget_and_sigma_bits() {
        assert!(matches!(
            FieldVersion::physics_reference_s2(MIN_SAMPLE_BUDGET - 1),
            Err(V2Error::InvalidSampleBudget)
        ));

        let mut version =
            FieldVersion::physics_reference_s2(DEFAULT_SAMPLE_BUDGET).expect("reference version");
        version.sigma = f64::from_bits(SIGMA.to_bits() ^ 1);
        assert!(matches!(
            version.validate(),
            Err(V2Error::InvalidVersion(_))
        ));
    }

    #[test]
    fn enum_wire_tags_and_json_names_are_frozen() {
        assert_eq!(BackendKind::PhysicsReference.wire_tag(), 0);
        assert_eq!(BackendKind::SemanticProduction.wire_tag(), 1);
        assert_eq!(SpaceKind::PhysicsReferenceS2.wire_tag(), 0);
        assert_eq!(SpaceKind::SemanticProduction384.wire_tag(), 1);
        assert_eq!(
            serde_json::to_string(&BackendKind::PhysicsReference).expect("backend JSON"),
            "\"physics_reference\""
        );
        assert_eq!(
            serde_json::to_string(&SpaceKind::SemanticProduction384).expect("space JSON"),
            "\"semantic_production_384\""
        );
    }

    #[test]
    fn version_json_rejects_unknown_fields() {
        let version =
            FieldVersion::physics_reference_s2(DEFAULT_SAMPLE_BUDGET).expect("reference version");
        let mut value = serde_json::to_value(version).expect("serialize version");
        value
            .as_object_mut()
            .expect("object")
            .insert("unexpected".to_owned(), serde_json::Value::Null);
        assert!(serde_json::from_value::<FieldVersion>(value).is_err());
    }
}
