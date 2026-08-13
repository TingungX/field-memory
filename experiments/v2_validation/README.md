# v2 validation fixtures

The canonical validation artifact schema is `artifact-schema-v1.json`, with
SHA-256 `4481c5ad3234b594df3c578c03af5931beed3e158367b7ccb63376fef76c457a`.
Runners and validators must reject any different schema byte stream instead of
accepting equivalent-looking keys.

`fixtures-v1.json` is the committed input manifest for the first v2 validation
fixtures.  Its authority is limited to fixture inputs and expected structural
relationships; the implementation contract remains the authority for v2
semantics and acceptance gates.

The manifest's top-level JSON key is `schema_version: 1`.  Run and result JSON
artifacts use that same top-level key and value; documentation may call it the
artifact schema version, but `schema_version` is the only top-level version
key.

Every fixture object must explicitly carry
`backend_kind="physics_reference"`, `space_kind="physics_reference_s2"`, and
`embedding_identity=null`.  These fields are required even for direct and
analytic fixtures, so a runner cannot reinterpret a reference fixture as a
production semantic artifact.

All state fixtures in this first materialized slice are explicit `D=3` inputs.
Their coordinates, event content, and fixture-source metadata are listed as
JSON f64/string values.  A runner must consume those values directly; it must
not regenerate a basis, a reflection, or a rotation from an algebraic recipe.
Each state-fixture Event `source` is test provenance only and is not a physical
field parameter; the analytic ray oracle's top-level `source` is explicitly
its physical ray source.

Every fixture carries a committed `coordinate_sha256` produced by the
implementation contract's `fm-v2/fixture-coordinates/v1` canonical codec and
`coordinate_sha256_status = "canonical_v1_verified"`.  Admission recomputes
the digest from the manifest values before any fixture is used; a mismatch is
an invalid fixture, not a request to rewrite the manifest.

Fixture admission first uses a strict JSON parse that rejects duplicate object
keys (for example, a read-only Python decoder with `object_pairs_hook`) before
ordinary schema decoding.

`vacuum_transport` is a direct, raw-vacuum structural fixture with `K=4`, two
actual Carrier nodes, and two explicitly unused capacity slots.  Its carriers
use the versioned gauge `b=+x`, `t=+y`, and `offset_rad=0.1`: `carrier-0` is
`b`, while `carrier-1` is materialized as
`[-cos(0.1), sin(0.1), 0]`, not an exact antipode.  Their transport volumes are
each `0.5`, while every physical row is empty or zero.  It intentionally does
not contain sampled `phi`/`chi`, a graph, or generated numerical results; the
Phase 0 builder must generate the graph and record it in the artifact.  It may
only exercise raw vacuum transport and is never eligible for readiness,
activation, or persistence.

`uniform_ray_oracle` is an `origin=analytic_reference`, `D=3` fixture for the
continuous S2 ray reference only.  Its shape is `density={kind:"uniform",
value:1}`, `sigma=1/pi`, `source={q, initial_total_influence}`, and an explicit
`rays[]` list of tangent `omega` vectors, each with its path length, optical
thickness, normalized angular weight, and expected raw residual.  Every listed
ray has weight `0.25`, thickness `1`, and residual `exp(-1)`; their normalized
weighted residual is also `exp(-1)`.  The four rays are a discrete fixture
aggregate diagnostic, not a finite Sample graph or a replacement for the
continuous reference.  The fixture never constructs state and is excluded from
readiness, activation, and persistence.
