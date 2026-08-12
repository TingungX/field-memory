# v2 production dimension-fidelity experiment

This directory isolates the pre-registered representation experiment for
field-memory v2. It measures how a frozen BGE-M3 1,024-dimensional direction
space changes after projection. It does not run, implement, or validate v2
DensitySite, Sample, transport, Response, scatter, collapse, or equilibrium.

The authoritative protocol is [the dimension-fidelity spec](../../docs/specs/2026-08-12-v2-dimension-fidelity-experiment.md). The runner and validator must not turn a passing result into a claim that v2 dynamics are correct.

## Files

- `run_dimension_fidelity.py` embeds the frozen input, fits uncentered spherical
  PCA only on Build memories, evaluates the frozen metrics, and emits an atomic
  JSON artifact.
- `validate_artifact.py` is offline. It recomputes every spherical-PCA hard gate
  from saved metrics; enforces the frozen formal provenance, population,
  dimensions, seeds, pair indices, and random-diagnostic Cartesian product; it
  also re-hashes the recorded dataset, runner, protocol, cache, and projection
  bundle before recomputing the minimum passing dimension.
- `artifacts/` is generated and ignored. It contains the strict embedding cache,
  projection bundle, and emitted JSON artifacts.

The frozen formal corpus is `../v2_feasibility/dataset_scale.json`. Do not
regenerate it from the current worktree: the formal input is identified by its
pre-registered SHA-256.

## Requirements

`python3`, NumPy, SciPy, and a local Ollama service with the exact frozen
`bge-m3:latest` model digest are required. The runner uses only Ollama's local
embedding endpoint. It does not call `test-model` or any paid LLM endpoint.

Both modes must run as exactly one Python process with BLAS parallelism pinned:

```bash
export OMP_NUM_THREADS=1
export OPENBLAS_NUM_THREADS=1
export VECLIB_MAXIMUM_THREADS=1
export NUMEXPR_NUM_THREADS=1
```

## Preflight

Run preflight before the formal run. It uses 128 deterministic memories, 32
deterministic queries, candidate dimensions `3,16,32,64`, random dimensions
`3,32`, one Gaussian diagnostic seed, and 2,000 all-memory pairs. Its limits
are 10 minutes wall time and 2 GiB RSS.

```bash
python3 experiments/v2_dimension_fidelity/run_dimension_fidelity.py \
  --mode preflight \
  --cache experiments/v2_dimension_fidelity/artifacts/preflight-embeddings.npz \
  --projection-dir experiments/v2_dimension_fidelity/artifacts/preflight-projection \
  --output experiments/v2_dimension_fidelity/artifacts/preflight.json \
  --limit-memories 128 --limit-queries 32 --memory-probes 32 \
  --dimensions 3,16,32,64 \
  --random-dimensions 3,32 \
  --random-seeds 20260812 \
  --pair-count 2000 \
  --wall-time-limit-seconds 600

python3 experiments/v2_dimension_fidelity/validate_artifact.py \
  --artifact experiments/v2_dimension_fidelity/artifacts/preflight.json
```

`preflight_only` is never a production-dimension decision. Any failure,
embedding mismatch, cache mismatch, NaN, zero row, timeout, or resource limit
stops the sequence; do not reduce the run or retry automatically.

## Formal run

Only run this after the preflight artifact validates. The runner rejects any
formal argument drift: it requires the frozen 6,611-memory/2,161-query corpus,
all ten PCA dimensions, all three random seeds, and 100,000 heldout×heldout
pairs, plus the fixed model digest and Ollama version.

```bash
python3 experiments/v2_dimension_fidelity/run_dimension_fidelity.py \
  --mode formal \
  --cache experiments/v2_dimension_fidelity/artifacts/formal-embeddings.npz \
  --projection-dir experiments/v2_dimension_fidelity/artifacts/formal-projection \
  --output experiments/v2_dimension_fidelity/artifacts/formal.json \
  --wall-time-limit-seconds 3600

python3 experiments/v2_dimension_fidelity/validate_artifact.py \
  --artifact experiments/v2_dimension_fidelity/artifacts/formal.json
```

The validator independently checks every PCA gate and selects the smallest
passing PCA dimension. Formal global-pair metrics use the fixed 100,000 unique
pairs drawn only from heldout memories; preflight uses 2,000 unique pairs from
its 128-memory subset. Random-projection rows are diagnostics only: the result
must contain every frozen `seed × dimension` row, and no random seed can be
used for production selection. If no PCA candidate passes every gate, the only
valid result is `no_compressed_production_candidate` with the 1,024-dimensional
native fallback.

## Artifact and conclusion boundary

Generated cache, projection, and result files remain under `artifacts/` and are
not committed. Keep the emitted formal JSON and its validated provenance when
writing the later report.

`dimension_fidelity_passed` means only that this frozen corpus, BGE-M3 model,
and mapping passed the finite representation checks. It does not establish v2
transport, conservation, locality, collapse, or long-term dynamics. A changed
corpus, model digest, or projection bundle requires a new experiment.
