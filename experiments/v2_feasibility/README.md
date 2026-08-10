# field-memory v2 feasibility observation

This is an isolated experiment, not a production implementation and not an
acceptance of the v2 architecture. It compares the same memory stream under:

1. a conventional hybrid RAG read (`dense cosine + lexical overlap + a small
   recency term`), shaped after the open-source Mem0 memory API and vector-store
   path; and
2. an observational v2 field (`density-weighted sample replicas -> serial
   influence propagation -> readout -> feedback`).

## Inputs and outputs

`dataset.json` and the Python harness are versioned inputs. The scale dataset,
raw result files, model-smoke records, Python bytecode, and embedding caches are
reproducible/generated artifacts and are intentionally ignored by Git. A clean
checkout regenerates them with the commands below; the compact evidence snapshot
is retained under `docs/reports/`.

- `dataset.json`: 30 controlled memory items, 11 read queries, and 3 explicit
  write/read cases. It is retained as a fast regression/control fixture, not as
  the scale result.
- `dataset_scale.json`: 6,611 memory records (5,882 LoCoMo conversation turns
  plus 729 repository document/code chunks), 2,161 queries (1,977 evidence-
  linked LoCoMo questions plus 184 project-context probes), and 64 write/read
  cases (32 replay cases plus 32 novel event markers). Nine LoCoMo questions
  with unresolved evidence are retained in provenance diagnostics but excluded
  from scoring.
- `results_scale.json`: the full deterministic comparison, per-query rows,
  write/read observations, seven sample-budget capacity points, and timings.
- `llm_smoke_failure.json`: the initial non-streaming Chat integration attempt
  and its proxy-side SQLite-lock evidence.
- `llm_smoke_traditional_responses_stream.json`: a successful Responses SSE
  transport check for traditional retrieved context. It is intentionally not a
  quality score; its 160-token cap ended the visible answer before completion.
- `llm_smoke_v2_interrupted.json`: the controlled v2-context stream attempt.
  HTTP response headers arrived, but no SSE payload arrived within 90 seconds,
  so the single TTY process was explicitly stopped and not retried.
- `validate_artifacts.py`: a no-network audit of dataset IDs, evidence links,
  dates, diagnostics, result-row coverage, recomputed averages, write summary,
  sensitivity capacity points, and the assertion that no LLM scored the data.
- Ollama `bge-m3` through `http://127.0.0.1:11434/api/embed` for all memory and
  query directions. No embedding request goes through the paid LLM endpoint.
- The optional integration smoke test sends a saved context through
  `http://127.0.0.1:4000/v1/responses` with `test-model`. It is separated from
  deterministic scoring and makes exactly one POST per invocation. The current
  proxy behavior is documented above rather than treated as a v2 score.

## v2 operationalization

The v2 note leaves several equations open. This harness makes the smallest
visible choices needed to observe behavior:

- one anchor per fixture memory, with `seed_hits` as its initial density;
- a sample point is a replica of an anchor direction; the replica count is
  proportional to density and capped by `sample_budget`;
- samples are sorted by cosine similarity to the query center;
- remaining influence is reduced by a density-dependent absorption term and the
  chain stops below `propagation_threshold`;
- the readout is captured before feedback and contains only positive-response
  anchors;
- feedback adds density, applies a bounded direction step, and applies a small
  default collapse to untouched anchors;
- both `write` and `read` call `perturb`; a novel write is kept in the v2 event
  log but does not create a new anchor, matching the current v2 proposal that
  anchors are created by `init_field` only.

All values are recorded in the selected result JSON (`results.json` for the
control run, `results_scale.json` for the scaled run). Changing them is a sensitivity
experiment, not a silent tuning change.

## Prepare and run

The pinned LoCoMo source is downloaded by default. For the run below a local
copy is also accepted, with the same SHA-256 check:

```bash
python3 experiments/v2_feasibility/prepare_scale.py \
  --output experiments/v2_feasibility/dataset_scale.json
```

The control fixture is intentionally small and should be run first when
changing the harness:

```bash
python3 experiments/v2_feasibility/run_experiment.py

# Full scaled comparison; embeddings are cached outside the repository.
python3 experiments/v2_feasibility/run_experiment.py \
  --dataset experiments/v2_feasibility/dataset_scale.json \
  --output experiments/v2_feasibility/results_scale.json \
  --cache /tmp/field-memory-v2-scale-embeddings.npz \
  --embedding-batch-size 32

# Two real model-context calls; the key is supplied only through the process env.
# First verify saved data and accounting; this is local and no-network.
python3 experiments/v2_feasibility/validate_artifacts.py

# Then run exactly one real context-consumption check, with the key only in env.
LLM_API_KEY='<endpoint-key>' LLM_MODEL='test-model' \
python3 experiments/v2_feasibility/run_llm_smoke.py \
  --pipeline traditional \
  --output /tmp/traditional-context-smoke.json \
  --max-questions 1 --max-output-tokens 160 --stream
```

The script requires `numpy`; it uses only Python standard-library HTTP and the
local Ollama endpoint. The `--limit-*` flags and `--skip-sensitivity` are for
short, controlled probes. `results_scale.json` contains retrieval metrics,
per-query readouts, field state deltas, write/read behavior, capacity
sensitivity, timings, cache misses, and HTTP call metadata.

## Scale result at a glance

On the scaled set, the conventional hybrid baseline reached precision@5
`0.1289`, recall@5 `0.5624`, MRR `0.4595`, and NDCG@5 `0.4712`. The current v2
observation reached precision@5 `0.0047`, recall@5 `0.0172`, MRR `0.0170`, and
NDCG@5 `0.0152`. Its default 96-sample projection covers only `1.45%` of the
6,611 anchors per event. Raising the budget to 4,096 covers `61.96%` and raises
recall@5 to `0.2593`, but remains below the baseline while taking roughly 31 s
for the v2-only audit over all 2,161 queries.

The write path is asymmetric: all 64 baseline writes become exact retrievable
self-hits; all 64 v2 writes enter the event log, but none returns the raw event
text, and the 32 replay writes do not read back their expected anchor under the
default projection.

## Reference project

The selected existing project is [Mem0](https://github.com/mem0ai/mem0), pinned
for the source audit at commit `4debc58`. Its public architecture provides a
useful conventional reference: `Memory.add()` extracts or stores memory text,
the embedder produces vectors, a vector store persists them, and `search()`
returns ranked memory records. Qdrant is the default local store in the current
source and can add a BM25 sparse signal. We do not install or run the full Mem0
stack in this experiment because the supplied proxy has no embeddings route;
the harness reproduces its retrieval shape using the real local BGE-M3 vectors
and records that boundary explicitly.
