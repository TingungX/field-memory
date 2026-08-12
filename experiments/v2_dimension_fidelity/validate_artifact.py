#!/usr/bin/env python3
"""Offline validator for the pre-registered v2 dimension-fidelity artifact."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import re
from pathlib import Path
from typing import Any

import numpy as np


SCHEMA_VERSION = 1
FORMAL_DATASET_SHA256 = "ed5a9836f277ea8242fce0bd477363bb21e81d606226dc607b92554d7b7d9901"
FORMAL_MODEL = "bge-m3:latest"
FORMAL_MODEL_DIGEST = "7907646426070047a77226ac3e684fbbe8410524f7b4a74d02837e43f2146bab"
FORMAL_OLLAMA_VERSION = "0.21.2"
FORMAL_SOURCE_DIMENSION = 1024
FORMAL_DIMENSIONS = [3, 8, 16, 32, 64, 128, 256, 384, 512, 768]
FORMAL_RANDOM_DIMENSIONS = [3, 32, 128, 512]
FORMAL_RANDOM_SEEDS = [20260812, 20260813, 20260814]
FORMAL_PAIR_SEED = 20260812
FORMAL_PAIR_COUNT = 100_000
FORMAL_PAIR_SCOPE = "heldout_memory"
PREFLIGHT_PAIR_COUNT = 2_000
PREFLIGHT_PAIR_SCOPE = "all_memory"
FORMAL_MEMORY_COUNT = 6_611
FORMAL_QUERY_COUNT = 2_161
SPLIT_SEED = 20260812
PCA_EIGENGAP_REL_TOL = 1e-12
THRESHOLDS = {
    "invalid_projected_rows_max": 0,
    "global_cosine_abs_error_p95_max": 0.05,
    "global_cosine_spearman_min": 0.98,
    "local_angular_abs_error_p95_max": 0.10,
    "query_neighbor_recall_at_10_mean_min": 0.90,
    "query_neighbor_recall_at_50_mean_min": 0.95,
    "heldout_memory_neighbor_recall_at_10_mean_min": 0.90,
    "ground_truth_recall_at_5_ratio_overall_min": 0.95,
    "ground_truth_recall_at_5_ratio_locomo_min": 0.95,
    "ground_truth_recall_at_5_ratio_project_min": 0.95,
}

SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
GIT_COMMIT_RE = re.compile(r"^[0-9a-f]{40,64}$")
ROOT = Path(__file__).resolve().parents[2]
RUNNER_PATH = Path(__file__).with_name("run_dimension_fidelity.py")
PROTOCOL_PATH = ROOT / "docs/specs/2026-08-12-v2-dimension-fidelity-experiment.md"


class AuditError(ValueError):
    """Raised when an artifact conflicts with the frozen experiment protocol."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AuditError(message)


def require_mapping(value: Any, label: str) -> dict[str, Any]:
    require(isinstance(value, dict), f"{label} must be an object")
    return value


def require_list(value: Any, label: str) -> list[Any]:
    require(isinstance(value, list), f"{label} must be a list")
    return value


def require_sha256(value: Any, label: str) -> str:
    require(isinstance(value, str) and SHA256_RE.fullmatch(value) is not None, f"{label} must be a SHA-256 hex string")
    return value


def require_number(value: Any, label: str) -> float:
    require(isinstance(value, (int, float)) and not isinstance(value, bool), f"{label} must be numeric")
    number = float(value)
    require(math.isfinite(number), f"{label} must be finite")
    return number


def require_equal(actual: Any, expected: Any, label: str) -> None:
    require(actual == expected, f"{label}: expected {expected!r}, got {actual!r}")


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def resolve_recorded_path(value: Any, label: str) -> Path:
    require(isinstance(value, str) and value, f"{label} must be a non-empty path")
    path = Path(value).expanduser()
    if not path.is_absolute():
        path = Path.cwd() / path
    path = path.resolve()
    require(path.is_file(), f"{label} does not exist or is not a file: {path}")
    return path


def require_file_sha256(path: Path, expected: Any, label: str) -> None:
    require_sha256(expected, label)
    require_equal(sha256_file(path), expected, f"{label} (actual file hash)")


def matrices_sha256(*matrices: np.ndarray) -> str:
    digest = hashlib.sha256()
    for matrix in matrices:
        contiguous = np.ascontiguousarray(matrix, dtype=np.float32)
        digest.update(str(contiguous.shape).encode("ascii"))
        digest.update(b"\0")
        digest.update(contiguous.tobytes(order="C"))
    return digest.hexdigest()


def canonical_array_hash(metadata: dict[str, Any], *arrays: np.ndarray) -> str:
    digest = hashlib.sha256()
    digest.update(json.dumps(metadata, sort_keys=True, separators=(",", ":")).encode())
    for array in arrays:
        contiguous = np.ascontiguousarray(array)
        digest.update(contiguous.dtype.str.encode("ascii"))
        digest.update(json.dumps(contiguous.shape, separators=(",", ":")).encode())
        digest.update(contiguous.tobytes(order="C"))
    return digest.hexdigest()


def text_key(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def stable_fraction(value: str) -> float:
    return int(text_key(value), 16) / float(1 << 256)


def memory_group(row: dict[str, Any]) -> str:
    if row.get("source") == "locomo":
        return f"locomo:{row.get('source_sample', '')}"
    return f"project:{row.get('source_path', '')}"


def fit_mask(memories: list[dict[str, Any]]) -> np.ndarray:
    return np.asarray(
        [stable_fraction(f"{SPLIT_SEED}:{memory_group(row)}") < 0.8 for row in memories],
        dtype=bool,
    )


def selected_rows(dataset: dict[str, Any], mode: str) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    memories = require_list(dataset.get("memories"), "dataset.memories")
    queries = require_list(dataset.get("queries"), "dataset.queries")
    parsed_memories = [require_mapping(row, f"dataset.memories[{index}]") for index, row in enumerate(memories)]
    parsed_queries = [require_mapping(row, f"dataset.queries[{index}]") for index, row in enumerate(queries)]
    if mode == "formal":
        return parsed_memories, parsed_queries
    selected_queries: list[dict[str, Any]] = []
    for source in ("locomo", "project"):
        source_queries = [row for row in parsed_queries if row.get("source") == source]
        selected_queries.extend(sorted(source_queries, key=lambda row: text_key(f"preflight-query:{row['id']}"))[:16])
    required_ids = {memory_id for query in selected_queries for memory_id in query["relevant_ids"]}
    require(len(required_ids) <= 128, "preflight query relevance requires more than 128 memories")
    extras = sorted(
        (row for row in parsed_memories if row["id"] not in required_ids),
        key=lambda row: text_key(f"preflight-memory:{row['id']}"),
    )[: 128 - len(required_ids)]
    selected_ids = required_ids | {row["id"] for row in extras}
    selected_memories = [row for row in parsed_memories if row["id"] in selected_ids]
    require(len(selected_memories) == 128 and len(selected_queries) == 32, "preflight subset construction did not produce 128 memories / 32 queries")
    return selected_memories, selected_queries


def load_cache_vectors(
    cache_path: Path, metadata: dict[str, Any], texts: list[str]
) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    try:
        with np.load(cache_path, allow_pickle=False) as archive:
            stored = json.loads(str(archive["metadata_json"].item()))
            require_equal(stored, metadata, "embedding cache metadata")
            keys = np.asarray(archive["keys"])
            vectors = np.asarray(archive["vectors"], dtype=np.float32)
    except (KeyError, OSError, ValueError, json.JSONDecodeError) as error:
        raise AuditError(f"invalid embedding cache {cache_path}: {error}") from error
    require(vectors.ndim == 2 and vectors.shape == (len(keys), FORMAL_SOURCE_DIMENSION), "embedding cache vector shape is invalid")
    require(len(set(str(key) for key in keys)) == len(keys), "embedding cache keys are not unique")
    require(np.all(np.isfinite(vectors)), "embedding cache contains non-finite vectors")
    norms = np.linalg.norm(vectors.astype(np.float64), axis=1)
    require(np.all(np.isfinite(norms)) and np.all(norms > 1e-12), "embedding cache contains zero vectors")
    normalized = vectors / norms[:, None]
    by_key = {str(key): normalized[index] for index, key in enumerate(keys)}
    missing = [text for text in texts if text_key(text) not in by_key]
    require(not missing, f"embedding cache is missing {len(missing)} vectors required by artifact")
    return np.asarray([by_key[text_key(text)] for text in texts], dtype=np.float32), keys, vectors


def recompute_pair_indices_sha256(memories: list[dict[str, Any]], scope: str, pair_count: int) -> str:
    if scope == PREFLIGHT_PAIR_SCOPE:
        candidates = np.arange(len(memories), dtype=np.int64)
    elif scope == FORMAL_PAIR_SCOPE:
        candidates = np.flatnonzero(~fit_mask(memories)).astype(np.int64)
    else:
        raise AuditError(f"unsupported pair scope {scope!r}")
    require(len(candidates) >= 2, f"pair scope {scope} has fewer than two memories")
    pair_space = len(candidates) * (len(candidates) - 1) // 2
    require(pair_count <= pair_space, f"pair scope {scope} cannot draw {pair_count} unique pairs")
    rng = np.random.default_rng(FORMAL_PAIR_SEED)
    seen: set[tuple[int, int]] = set()
    ordered: list[tuple[int, int]] = []
    while len(ordered) < pair_count:
        remaining = pair_count - len(ordered)
        left = rng.integers(0, len(candidates), size=remaining * 2, dtype=np.int64)
        right = rng.integers(0, len(candidates) - 1, size=remaining * 2, dtype=np.int64)
        right += right >= left
        for a, b in zip(left, right):
            pair = (min(int(candidates[a]), int(candidates[b])), max(int(candidates[a]), int(candidates[b])))
            if pair not in seen:
                seen.add(pair)
                ordered.append(pair)
                if len(ordered) == pair_count:
                    break
    pairs = np.asarray(ordered, dtype="<i8")
    return hashlib.sha256(pairs.tobytes(order="C")).hexdigest()


def require_close(actual: Any, expected: float, label: str) -> None:
    number = require_number(actual, label)
    require(math.isclose(number, expected, rel_tol=0.0, abs_tol=1e-12), f"{label}: expected {expected}, got {number}")


def assert_finite_json(value: Any, label: str = "artifact") -> None:
    """Reject NaN/Infinity anywhere, while permitting documented JSON null fields."""

    if value is None or isinstance(value, (str, bool)):
        return
    if isinstance(value, (int, float)):
        require(math.isfinite(float(value)), f"{label} contains a non-finite number")
        return
    if isinstance(value, list):
        for index, item in enumerate(value):
            assert_finite_json(item, f"{label}[{index}]")
        return
    if isinstance(value, dict):
        for key, item in value.items():
            require(isinstance(key, str), f"{label} contains a non-string object key")
            assert_finite_json(item, f"{label}.{key}")
        return
    raise AuditError(f"{label} contains an unsupported JSON value")


def nested(mapping: dict[str, Any], path: str) -> Any:
    value: Any = mapping
    for part in path.split("."):
        if not isinstance(value, dict) or part not in value:
            raise AuditError(f"missing metric {path}")
        value = value[part]
    return value


def gate_values(metrics: dict[str, Any], invalid_rows: Any) -> dict[str, float | int | None]:
    invalid = require_number(invalid_rows, "invalid_projected_rows")
    require(invalid.is_integer() and invalid >= 0, "invalid_projected_rows must be a non-negative integer")
    return {
        "invalid_projected_rows": int(invalid),
        "global_cosine_abs_error_p95": require_number(
            nested(metrics, "global_pairs.cosine_abs_error.p95"),
            "global_pairs.cosine_abs_error.p95",
        ),
        "global_cosine_spearman": require_number(
            nested(metrics, "global_pairs.cosine_spearman"),
            "global_pairs.cosine_spearman",
        ),
        "local_angular_abs_error_p95": require_number(
            nested(metrics, "local_angular_abs_error_rad.p95"),
            "local_angular_abs_error_rad.p95",
        ),
        "query_neighbor_recall_at_10_mean": require_number(
            nested(metrics, "query_neighbors.recall_at_10.mean"),
            "query_neighbors.recall_at_10.mean",
        ),
        "query_neighbor_recall_at_50_mean": require_number(
            nested(metrics, "query_neighbors.recall_at_50.mean"),
            "query_neighbors.recall_at_50.mean",
        ),
        "heldout_memory_neighbor_recall_at_10_mean": require_number(
            nested(metrics, "heldout_memory_neighbors.recall_at_10.mean"),
            "heldout_memory_neighbors.recall_at_10.mean",
        ),
        "ground_truth_recall_at_5_ratio_overall": require_number(
            nested(metrics, "ground_truth_recall_at_5_ratio.overall"),
            "ground_truth_recall_at_5_ratio.overall",
        ),
        "ground_truth_recall_at_5_ratio_locomo": require_number(
            nested(metrics, "ground_truth_recall_at_5_ratio.locomo"),
            "ground_truth_recall_at_5_ratio.locomo",
        ),
        "ground_truth_recall_at_5_ratio_project": require_number(
            nested(metrics, "ground_truth_recall_at_5_ratio.project"),
            "ground_truth_recall_at_5_ratio.project",
        ),
    }


def recompute_gate(metrics: dict[str, Any], invalid_rows: Any) -> tuple[bool, dict[str, bool], dict[str, float | int | None]]:
    values = gate_values(metrics, invalid_rows)
    checks: dict[str, bool] = {}
    for threshold_name, threshold in THRESHOLDS.items():
        if threshold_name.endswith("_max"):
            metric_name = threshold_name[: -len("_max")]
            checks[metric_name] = bool(values[metric_name] <= threshold)
        else:
            metric_name = threshold_name[: -len("_min")]
            checks[metric_name] = bool(values[metric_name] >= threshold)
    return all(checks.values()), checks, values


def validate_recorded_gate(recorded: Any, computed_pass: bool, computed_checks: dict[str, bool], values: dict[str, float | int | None], label: str) -> None:
    gate = require_mapping(recorded, f"{label}.gate")
    require_equal(gate.get("pass"), computed_pass, f"{label}.gate.pass")
    rows = require_list(gate.get("checks"), f"{label}.gate.checks")
    require(len(rows) == len(THRESHOLDS), f"{label}.gate.checks count does not match frozen thresholds")
    actual: dict[str, dict[str, Any]] = {}
    for index, raw in enumerate(rows):
        row = require_mapping(raw, f"{label}.gate.checks[{index}]")
        metric = row.get("metric")
        require(isinstance(metric, str) and metric not in actual, f"{label}.gate has duplicate or invalid metric")
        actual[metric] = row
    require_equal(set(actual), set(computed_checks), f"{label}.gate metric names")
    for threshold_name, threshold in THRESHOLDS.items():
        metric = threshold_name.removesuffix("_max").removesuffix("_min")
        row = actual[metric]
        require_close(row.get("value"), float(values[metric]), f"{label}.gate.{metric}.value")
        require_equal(row.get("threshold"), threshold, f"{label}.gate.{metric}.threshold")
        require_equal(row.get("operator"), "<=" if threshold_name.endswith("_max") else ">=", f"{label}.gate.{metric}.operator")
        require_equal(row.get("pass"), computed_checks[metric], f"{label}.gate.{metric}.pass")


def validate_population(artifact: dict[str, Any], formal: bool) -> None:
    population = require_mapping(artifact.get("population"), "population")
    if formal:
        require_equal(population.get("memory_count"), FORMAL_MEMORY_COUNT, "population.memory_count")
        require_equal(population.get("query_count"), FORMAL_QUERY_COUNT, "population.query_count")
        require_equal(population.get("heldout_probe_count"), 512, "population.heldout_probe_count")
    else:
        require_equal(population.get("memory_count"), 128, "preflight population.memory_count")
        require_equal(population.get("query_count"), 32, "preflight population.query_count")
        require_equal(population.get("heldout_probe_count"), 32, "preflight population.heldout_probe_count")
    fit_count = population.get("fit_memory_count")
    heldout_count = population.get("heldout_memory_count")
    require(isinstance(fit_count, int) and isinstance(heldout_count, int), "population split counts must be integers")
    require(fit_count > 0 and heldout_count > 0 and fit_count + heldout_count == population["memory_count"], "population split counts are inconsistent")
    require_equal(population.get("split_seed"), SPLIT_SEED, "population.split_seed")
    require_sha256(population.get("split_sha256"), "population.split_sha256")
    require_sha256(population.get("probe_ids_sha256"), "population.probe_ids_sha256")
    require(isinstance(population.get("fit_group_count"), int) and population["fit_group_count"] > 0, "population.fit_group_count must be positive")
    require(isinstance(population.get("heldout_group_count"), int) and population["heldout_group_count"] > 0, "population.heldout_group_count must be positive")


def validate_provenance(artifact: dict[str, Any], formal: bool) -> None:
    provenance = require_mapping(artifact.get("provenance"), "provenance")
    require_equal(provenance.get("dataset_sha256"), FORMAL_DATASET_SHA256, "provenance.dataset_sha256")
    require_equal(provenance.get("source_dimension"), FORMAL_SOURCE_DIMENSION, "provenance.source_dimension")
    dataset_path = resolve_recorded_path(provenance.get("dataset_path"), "provenance.dataset_path")
    require_file_sha256(dataset_path, provenance.get("dataset_sha256"), "provenance.dataset_sha256")
    cache_path = resolve_recorded_path(provenance.get("cache_path"), "provenance.cache_path")
    require_file_sha256(cache_path, provenance.get("cache_sha256"), "provenance.cache_sha256")
    require_sha256(provenance.get("cache_content_sha256"), "provenance.cache_content_sha256")
    require_file_sha256(RUNNER_PATH, provenance.get("runner_sha256"), "provenance.runner_sha256")
    require_file_sha256(PROTOCOL_PATH, provenance.get("protocol_sha256"), "provenance.protocol_sha256")
    require(isinstance(provenance.get("cache_vector_count"), int) and provenance["cache_vector_count"] > 0, "provenance.cache_vector_count must be positive")
    model = require_mapping(provenance.get("model"), "provenance.model")
    require_equal(model.get("model"), FORMAL_MODEL, "provenance.model.model")
    require_equal(model.get("model_digest"), FORMAL_MODEL_DIGEST, "provenance.model.model_digest")
    require_equal(model.get("ollama_version"), FORMAL_OLLAMA_VERSION, "provenance.model.ollama_version")
    git = require_mapping(provenance.get("git"), "provenance.git")
    commit = git.get("commit")
    require(isinstance(commit, str) and GIT_COMMIT_RE.fullmatch(commit) is not None, "provenance.git.commit must be a git object ID")
    require_list(git.get("status_short"), "provenance.git.status_short")
    for key in ("python", "numpy", "scipy", "platform"):
        require(isinstance(provenance.get(key), str) and provenance[key], f"provenance.{key} must be present")
    thread_environment = require_mapping(provenance.get("thread_environment"), "provenance.thread_environment")
    require_equal(set(thread_environment), {"OMP_NUM_THREADS", "OPENBLAS_NUM_THREADS", "VECLIB_MAXIMUM_THREADS", "NUMEXPR_NUM_THREADS"}, "provenance.thread_environment keys")
    require(all(value == "1" for value in thread_environment.values()), "provenance.thread_environment values must all be '1'")


def validate_protocol(artifact: dict[str, Any], formal: bool) -> tuple[list[int], list[int], list[int]]:
    protocol = require_mapping(artifact.get("protocol"), "protocol")
    dimensions = require_list(protocol.get("dimensions"), "protocol.dimensions")
    random_dimensions = require_list(protocol.get("random_dimensions"), "protocol.random_dimensions")
    random_seeds = require_list(protocol.get("random_seeds"), "protocol.random_seeds")
    require(all(isinstance(value, int) and value > 0 for value in dimensions), "protocol.dimensions must be positive integers")
    require(all(isinstance(value, int) and value > 0 for value in random_dimensions), "protocol.random_dimensions must be positive integers")
    require(all(isinstance(value, int) and value > 0 for value in random_seeds), "protocol.random_seeds must be positive integers")
    if formal:
        require_equal(dimensions, FORMAL_DIMENSIONS, "protocol.dimensions")
        require_equal(random_dimensions, FORMAL_RANDOM_DIMENSIONS, "protocol.random_dimensions")
        require_equal(random_seeds, FORMAL_RANDOM_SEEDS, "protocol.random_seeds")
        require_equal(protocol.get("pair_count"), FORMAL_PAIR_COUNT, "protocol.pair_count")
        require_equal(protocol.get("pair_scope"), FORMAL_PAIR_SCOPE, "protocol.pair_scope")
    else:
        require_equal(dimensions, [3, 16, 32, 64], "preflight protocol.dimensions")
        require_equal(random_dimensions, [3, 32], "preflight protocol.random_dimensions")
        require_equal(random_seeds, [FORMAL_RANDOM_SEEDS[0]], "preflight protocol.random_seeds")
        require_equal(protocol.get("pair_count"), PREFLIGHT_PAIR_COUNT, "preflight protocol.pair_count")
        require_equal(protocol.get("pair_scope"), PREFLIGHT_PAIR_SCOPE, "preflight protocol.pair_scope")
    require_equal(protocol.get("pair_seed"), FORMAL_PAIR_SEED, "protocol.pair_seed")
    require_sha256(protocol.get("pair_indices_sha256"), "protocol.pair_indices_sha256")
    require_equal(protocol.get("candidate_projection"), "uncentered_spherical_pca_eigh_v1", "protocol.candidate_projection")
    require_equal(protocol.get("random_projection"), "numpy_pcg64_standard_normal_v1", "protocol.random_projection")
    require_equal(protocol.get("thresholds"), THRESHOLDS, "protocol.thresholds")
    return dimensions, random_dimensions, random_seeds


def ground_truth_counts(queries: list[dict[str, Any]]) -> dict[str, int]:
    counts = {"overall": 0, "locomo": 0, "project": 0}
    for query in queries:
        relevant = query.get("relevant_ids")
        if not isinstance(relevant, list) or not relevant:
            continue
        source = str(query.get("source", "unknown"))
        require(source in {"locomo", "project"}, f"query has unsupported source {source!r}")
        counts["overall"] += 1
        counts[source] += 1
    require(all(value > 0 for value in counts.values()), "ground-truth group counts must all be positive")
    return counts


def validate_reference(artifact: dict[str, Any], native_vectors_sha256: str, expected_counts: dict[str, int]) -> None:
    reference = require_mapping(artifact.get("native_reference"), "native_reference")
    require_equal(reference.get("dimension"), FORMAL_SOURCE_DIMENSION, "native_reference.dimension")
    # The runner must explicitly record identity rather than letting an artifact
    # claim an arbitrary same-width transform as the 1,024-dimensional reference.
    require_equal(reference.get("projection"), "identity", "native_reference.projection")
    require_equal(reference.get("normalization"), "finite_nonzero_l2_float32_v1", "native_reference.normalization")
    require_equal(reference.get("vectors_sha256"), native_vectors_sha256, "native_reference.vectors_sha256")
    ground_truth = require_mapping(reference.get("ground_truth_recall_at_5"), "native_reference.ground_truth_recall_at_5")
    for group in ("overall", "locomo", "project"):
        row = require_mapping(ground_truth.get(group), f"native_reference.ground_truth_recall_at_5.{group}")
        require_equal(row.get("count"), expected_counts[group], f"native reference {group} count")
        require_number(row.get("value"), f"native reference {group} value")


def validate_input_cache_and_pairs(artifact: dict[str, Any], mode: str) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    provenance = require_mapping(artifact["provenance"], "provenance")
    protocol = require_mapping(artifact["protocol"], "protocol")
    dataset_path = resolve_recorded_path(provenance["dataset_path"], "provenance.dataset_path")
    dataset = require_mapping(json.loads(dataset_path.read_text(encoding="utf-8")), "dataset")
    memories, queries = selected_rows(dataset, mode)
    population = require_mapping(artifact["population"], "population")
    duplicate_relevance_entry_count = sum(
        len(query["relevant_ids"]) - len(set(query["relevant_ids"])) for query in queries
    )
    require_equal(
        population.get("duplicate_relevance_entry_count"),
        duplicate_relevance_entry_count,
        "population.duplicate_relevance_entry_count",
    )
    model = require_mapping(provenance["model"], "provenance.model")
    cache_metadata = {
        "schema_version": 1,
        "dataset_sha256": provenance["dataset_sha256"],
        "model": FORMAL_MODEL,
        "model_digest": FORMAL_MODEL_DIGEST,
        "ollama_version": model["ollama_version"],
        "dimension": FORMAL_SOURCE_DIMENSION,
    }
    cache_path = resolve_recorded_path(provenance["cache_path"], "provenance.cache_path")
    memory_vectors, cache_keys, cache_vectors = load_cache_vectors(cache_path, cache_metadata, [str(row["text"]) for row in memories])
    query_vectors, _, _ = load_cache_vectors(cache_path, cache_metadata, [str(row["text"]) for row in queries])
    require_equal(
        provenance.get("cache_content_sha256"),
        canonical_array_hash(cache_metadata, cache_keys, cache_vectors),
        "provenance.cache_content_sha256",
    )
    validate_reference(artifact, matrices_sha256(memory_vectors, query_vectors), ground_truth_counts(queries))
    expected_pair_hash = recompute_pair_indices_sha256(
        memories,
        str(protocol["pair_scope"]),
        int(protocol["pair_count"]),
    )
    require_equal(protocol.get("pair_indices_sha256"), expected_pair_hash, "protocol.pair_indices_sha256")
    return memories, queries


def validate_summary(summary: Any, label: str, expected_count: int) -> None:
    row = require_mapping(summary, label)
    require_equal(row.get("count"), expected_count, f"{label}.count")
    for field in ("mean", "p50", "p95", "p99", "max"):
        require_number(row.get(field), f"{label}.{field}")


def validate_metrics(metrics: dict[str, Any], query_count: int, probe_count: int, pair_count: int, expected_ground_truth_counts: dict[str, int], label: str) -> None:
    global_pairs = require_mapping(metrics.get("global_pairs"), f"{label}.global_pairs")
    require_equal(global_pairs.get("count"), pair_count, f"{label}.global_pairs.count")
    validate_summary(global_pairs.get("cosine_abs_error"), f"{label}.global_pairs.cosine_abs_error", pair_count)
    validate_summary(global_pairs.get("angular_abs_error_rad"), f"{label}.global_pairs.angular_abs_error_rad", pair_count)
    spearman = require_number(global_pairs.get("cosine_spearman"), f"{label}.global_pairs.cosine_spearman")
    require(-1.0 <= spearman <= 1.0, f"{label}.global_pairs.cosine_spearman must be in [-1, 1]")
    quintiles = require_list(global_pairs.get("reference_cosine_quintiles"), f"{label}.global_pairs.reference_cosine_quintiles")
    require_equal(len(quintiles), 5, f"{label}.global_pairs.reference_cosine_quintiles count")
    require(sum(require_mapping(item, f"{label}.global quintile").get("count", 0) for item in quintiles) == pair_count, f"{label}.global quintile counts")
    for index, raw in enumerate(quintiles):
        row = require_mapping(raw, f"{label}.global quintile {index}")
        require_equal(row.get("index"), index, f"{label}.global quintile {index}.index")
        count = row.get("count")
        require(isinstance(count, int) and count > 0, f"{label}.global quintile {index}.count")
        require_number(row.get("reference_cosine_min"), f"{label}.global quintile {index}.reference_cosine_min")
        require_number(row.get("reference_cosine_max"), f"{label}.global quintile {index}.reference_cosine_max")
        validate_summary(row.get("cosine_abs_error"), f"{label}.global quintile {index}.cosine_abs_error", count)
        validate_summary(row.get("angular_abs_error_rad"), f"{label}.global quintile {index}.angular_abs_error_rad", count)
    validate_summary(metrics.get("local_angular_abs_error_rad"), f"{label}.local_angular_abs_error_rad", 10 * (query_count + probe_count))
    query_neighbors = require_mapping(metrics.get("query_neighbors"), f"{label}.query_neighbors")
    for name in ("recall_at_10", "recall_at_50", "jaccard_at_10"):
        validate_summary(query_neighbors.get(name), f"{label}.query_neighbors.{name}", query_count)
    heldout = require_mapping(metrics.get("heldout_memory_neighbors"), f"{label}.heldout_memory_neighbors")
    require_equal(heldout.get("probe_count"), probe_count, f"{label}.heldout_memory_neighbors.probe_count")
    validate_summary(heldout.get("recall_at_10"), f"{label}.heldout_memory_neighbors.recall_at_10", probe_count)
    recall = require_mapping(metrics.get("ground_truth_recall_at_5"), f"{label}.ground_truth_recall_at_5")
    ratios = require_mapping(metrics.get("ground_truth_recall_at_5_ratio"), f"{label}.ground_truth_recall_at_5_ratio")
    for source in ("overall", "locomo", "project"):
        row = require_mapping(recall.get(source), f"{label}.ground_truth_recall_at_5.{source}")
        require_equal(row.get("count"), expected_ground_truth_counts[source], f"{label}.ground_truth_recall_at_5.{source}.count")
        require_number(row.get("value"), f"{label}.ground_truth_recall_at_5.{source}.value")
        require_number(ratios.get(source), f"{label}.ground_truth_recall_at_5_ratio.{source}")


def validate_projection_bundle(artifact: dict[str, Any], max_dimension: int) -> None:
    pca = require_mapping(artifact["spherical_pca"], "spherical_pca")
    provenance = require_mapping(artifact["provenance"], "provenance")
    population = require_mapping(artifact["population"], "population")
    bundle_path = resolve_recorded_path(pca.get("bundle_path"), "spherical_pca.bundle_path")
    require_file_sha256(bundle_path, pca.get("bundle_sha256"), "spherical_pca.bundle_sha256")
    try:
        with np.load(bundle_path, allow_pickle=False) as archive:
            components = np.asarray(archive["components"])
            eigenvalues = np.asarray(archive["eigenvalues"])
            metadata = json.loads(str(archive["metadata_json"].item()))
    except (KeyError, OSError, ValueError, json.JSONDecodeError) as error:
        raise AuditError(f"invalid PCA bundle {bundle_path}: {error}") from error
    require(components.shape == (FORMAL_SOURCE_DIMENSION, max_dimension), "PCA bundle component shape is invalid")
    require(eigenvalues.ndim == 1 and len(eigenvalues) >= max_dimension, "PCA bundle eigenvalue shape is invalid")
    require(np.all(np.isfinite(components)) and np.all(np.isfinite(eigenvalues)), "PCA bundle contains non-finite values")
    expected_metadata = {
        "algorithm": "uncentered_spherical_pca_eigh_v1",
        "dataset_sha256": provenance["dataset_sha256"],
        "model_digest": FORMAL_MODEL_DIGEST,
        "split_seed": SPLIT_SEED,
        "split_sha256": population["split_sha256"],
        "source_dimension": FORMAL_SOURCE_DIMENSION,
        "max_target_dimension": max_dimension,
        "eigensolver": "numpy.linalg.eigh",
        "eigengap_relative_tolerance": PCA_EIGENGAP_REL_TOL,
        "boundary_eigengaps": metadata.get("boundary_eigengaps"),
    }
    boundary_eigengaps = require_mapping(metadata.get("boundary_eigengaps"), "PCA bundle metadata.boundary_eigengaps")
    protocol = require_mapping(artifact["protocol"], "protocol")
    require_equal(
        set(boundary_eigengaps),
        {
            str(dimension)
            for dimension in require_list(protocol["dimensions"], "protocol.dimensions")
        },
        "PCA bundle metadata.boundary_eigengaps keys",
    )
    require_close(metadata.get("eigengap_relative_tolerance"), PCA_EIGENGAP_REL_TOL, "PCA bundle metadata.eigengap_relative_tolerance")
    eigenvalue_floor = PCA_EIGENGAP_REL_TOL * max(float(eigenvalues[0]), 1.0)
    for dimension_text, gap in boundary_eigengaps.items():
        dimension = int(dimension_text)
        require(0 < dimension < len(eigenvalues), f"PCA boundary eigengap dimension {dimension} is invalid")
        actual_gap = float(eigenvalues[dimension - 1] - eigenvalues[dimension])
        require_number(gap, f"PCA boundary eigengap d={dimension}")
        require_close(gap, actual_gap, f"PCA boundary eigengap d={dimension}")
        require(actual_gap > eigenvalue_floor, f"PCA boundary eigengap d={dimension} does not exceed the frozen floor")
    require_equal(metadata, expected_metadata, "PCA bundle metadata")
    require_equal(pca.get("bundle_metadata"), expected_metadata, "spherical_pca.bundle_metadata")
    require_equal(
        pca.get("bundle_content_sha256"),
        canonical_array_hash(metadata, components, eigenvalues),
        "spherical_pca.bundle_content_sha256",
    )


def validate_pca_results(
    artifact: dict[str, Any], dimensions: list[int], query_count: int, probe_count: int, pair_count: int, expected_ground_truth_counts: dict[str, int]
) -> tuple[list[int], str]:
    pca = require_mapping(artifact.get("spherical_pca"), "spherical_pca")
    bundle_sha256 = require_sha256(pca.get("bundle_sha256"), "spherical_pca.bundle_sha256")
    require(isinstance(pca.get("bundle_path"), str) and pca["bundle_path"], "spherical_pca.bundle_path must be present")
    rows = require_list(pca.get("results"), "spherical_pca.results")
    require(len(rows) == len(dimensions), "spherical_pca results count does not match dimensions")
    by_dimension: dict[int, dict[str, Any]] = {}
    passing: list[int] = []
    for index, raw in enumerate(rows):
        row = require_mapping(raw, f"spherical_pca.results[{index}]")
        dimension = row.get("target_dimension")
        require(isinstance(dimension, int) and dimension in dimensions and dimension not in by_dimension, "spherical_pca result dimensions must be unique frozen candidates")
        by_dimension[dimension] = row
        require_equal(row.get("projection"), "spherical_pca", f"PCA d={dimension}.projection")
        require_equal(row.get("eligible_for_production_gate"), True, f"PCA d={dimension}.eligible_for_production_gate")
        require_equal(row.get("projection_bundle_sha256"), bundle_sha256, f"PCA d={dimension}.projection_bundle_sha256")
        require(isinstance(row.get("retained_second_moment"), (int, float)), f"PCA d={dimension}.retained_second_moment must be numeric")
        require(0.0 <= require_number(row["retained_second_moment"], f"PCA d={dimension}.retained_second_moment") <= 1.0, f"PCA d={dimension}.retained_second_moment must be within [0, 1]")
        metrics = require_mapping(row.get("metrics"), f"PCA d={dimension}.metrics")
        validate_metrics(metrics, query_count, probe_count, pair_count, expected_ground_truth_counts, f"PCA d={dimension}")
        computed_pass, computed_checks, values = recompute_gate(metrics, row.get("invalid_projected_rows"))
        validate_recorded_gate(row.get("gate"), computed_pass, computed_checks, values, f"PCA d={dimension}")
        if computed_pass:
            passing.append(dimension)
    require_equal(sorted(by_dimension), sorted(dimensions), "spherical_pca result dimension set")
    return sorted(passing), bundle_sha256


def validate_random_diagnostics(artifact: dict[str, Any], random_dimensions: list[int], random_seeds: list[int], pair_count: int) -> None:
    rows = require_list(artifact.get("gaussian_random_diagnostic"), "gaussian_random_diagnostic")
    expected = {(seed, dimension) for seed in random_seeds for dimension in random_dimensions}
    require(len(rows) == len(expected), "random diagnostic is not the full seed x dimension Cartesian product")
    actual: set[tuple[int, int]] = set()
    matrix_hashes: dict[int, str] = {}
    for index, raw in enumerate(rows):
        row = require_mapping(raw, f"gaussian_random_diagnostic[{index}]")
        seed, dimension = row.get("seed"), row.get("target_dimension")
        require(isinstance(seed, int) and isinstance(dimension, int), "random diagnostic seed and dimension must be integers")
        key = (seed, dimension)
        require(key in expected and key not in actual, "random diagnostic has an unexpected or duplicate seed x dimension row")
        actual.add(key)
        require_equal(row.get("projection"), "gaussian_random", f"random {key}.projection")
        require_equal(row.get("eligible_for_production_gate"), False, f"random {key}.eligible_for_production_gate")
        require_equal(row.get("invalid_projected_rows"), 0, f"random {key}.invalid_projected_rows")
        matrix_sha256 = require_sha256(row.get("projection_matrix_sha256"), f"random {key}.projection_matrix_sha256")
        if seed in matrix_hashes:
            require_equal(matrix_sha256, matrix_hashes[seed], f"random seed={seed} projection matrix hash")
        matrix_hashes[seed] = matrix_sha256
        metrics = require_mapping(row.get("global_pairs"), f"random {key}.global_pairs")
        require_equal(metrics.get("count"), pair_count, f"random {key}.global_pairs.count")
        validate_summary(metrics.get("cosine_abs_error"), f"random {key}.global cosine", pair_count)
        validate_summary(metrics.get("angular_abs_error_rad"), f"random {key}.global angular", pair_count)
        spearman = require_number(metrics.get("cosine_spearman"), f"random {key}.global cosine spearman")
        require(-1.0 <= spearman <= 1.0, f"random {key}.global cosine spearman must be in [-1, 1]")
        quintiles = require_list(metrics.get("reference_cosine_quintiles"), f"random {key}.global quintiles")
        require_equal(len(quintiles), 5, f"random {key}.global quintile count")
        require(sum(require_mapping(item, f"random {key}.quintile").get("count", 0) for item in quintiles) == pair_count, f"random {key}.global quintile counts")
        for quintile_index, raw_quintile in enumerate(quintiles):
            quintile = require_mapping(raw_quintile, f"random {key}.quintile {quintile_index}")
            require_equal(quintile.get("index"), quintile_index, f"random {key}.quintile {quintile_index}.index")
            count = quintile.get("count")
            require(isinstance(count, int) and count > 0, f"random {key}.quintile {quintile_index}.count")
            require_number(quintile.get("reference_cosine_min"), f"random {key}.quintile {quintile_index}.reference_cosine_min")
            require_number(quintile.get("reference_cosine_max"), f"random {key}.quintile {quintile_index}.reference_cosine_max")
            validate_summary(quintile.get("cosine_abs_error"), f"random {key}.quintile {quintile_index}.cosine", count)
            validate_summary(quintile.get("angular_abs_error_rad"), f"random {key}.quintile {quintile_index}.angular", count)
    require_equal(actual, expected, "random diagnostic Cartesian product")


def validate_selection(artifact: dict[str, Any], mode: str, passing_dimensions: list[int]) -> None:
    selection = require_mapping(artifact.get("selection"), "selection")
    if mode == "preflight":
        require_equal(selection.get("status"), "preflight_only", "selection.status")
        require_equal(selection.get("minimum_passing_dimension"), None, "selection.minimum_passing_dimension")
        require_equal(selection.get("passing_dimensions_observed"), passing_dimensions, "selection.passing_dimensions_observed")
        return
    if passing_dimensions:
        require_equal(selection.get("status"), "dimension_fidelity_passed", "selection.status")
        require_equal(selection.get("projection"), "spherical_pca", "selection.projection")
        require_equal(selection.get("minimum_passing_dimension"), min(passing_dimensions), "selection.minimum_passing_dimension")
        require_equal(selection.get("passing_dimensions"), passing_dimensions, "selection.passing_dimensions")
    else:
        require_equal(selection.get("status"), "no_compressed_production_candidate", "selection.status")
        require_equal(selection.get("projection"), None, "selection.projection")
        require_equal(selection.get("minimum_passing_dimension"), None, "selection.minimum_passing_dimension")
        require_equal(selection.get("passing_dimensions"), [], "selection.passing_dimensions")
    require_equal(selection.get("native_fallback_dimension"), FORMAL_SOURCE_DIMENSION, "selection.native_fallback_dimension")


def validate_resources(artifact: dict[str, Any], formal: bool, artifact_path: Path) -> None:
    resources = require_mapping(artifact.get("resources"), "resources")
    limit = 3600 if formal else 600
    require(0.0 <= require_number(resources.get("wall_time_seconds"), "resources.wall_time_seconds") <= limit, "resources.wall_time_seconds exceeds the mode limit")
    rss_limit = 4 * 1024 * 1024 * 1024 if formal else 2 * 1024 * 1024 * 1024
    rss = require_number(resources.get("peak_rss_bytes"), "resources.peak_rss_bytes")
    require(rss > 0 and rss <= rss_limit, "resources.peak_rss_bytes exceeds the mode limit")
    require_equal(resources.get("rss_limit_bytes"), rss_limit, "resources.rss_limit_bytes")
    require_equal(resources.get("artifact_limit_bytes"), 1024 * 1024 * 1024, "resources.artifact_limit_bytes")
    provenance = require_mapping(artifact["provenance"], "provenance")
    cache_path = resolve_recorded_path(provenance["cache_path"], "provenance.cache_path")
    pca = require_mapping(artifact["spherical_pca"], "spherical_pca")
    bundle_path = resolve_recorded_path(pca["bundle_path"], "spherical_pca.bundle_path")
    actual_cache_bytes = cache_path.stat().st_size
    actual_projection_bytes = sum(
        path.stat().st_size for path in bundle_path.parent.rglob("*") if path.is_file()
    )
    actual_result_bytes = artifact_path.stat().st_size
    actual_combined = actual_cache_bytes + actual_projection_bytes + actual_result_bytes
    require_equal(resources.get("cache_bytes"), actual_cache_bytes, "resources.cache_bytes")
    require_equal(resources.get("projection_bytes"), actual_projection_bytes, "resources.projection_bytes")
    require_equal(resources.get("result_json_bytes"), actual_result_bytes, "resources.result_json_bytes")
    require_equal(resources.get("combined_artifact_bytes"), actual_combined, "resources.combined_artifact_bytes")
    require(actual_combined <= resources["artifact_limit_bytes"], "resources combined artifact size exceeds configured limit")


def validate_artifact(artifact: dict[str, Any], artifact_path: Path) -> dict[str, Any]:
    assert_finite_json(artifact)
    require_equal(artifact.get("schema_version"), SCHEMA_VERSION, "schema_version")
    mode = artifact.get("mode")
    require(mode in {"preflight", "formal"}, "mode must be preflight or formal")
    formal = mode == "formal"
    require_equal(artifact.get("status"), "complete", "status")
    require_equal(artifact.get("decision_scope"), "representation fidelity only; no v2 dynamics", "decision_scope")
    validate_provenance(artifact, formal)
    validate_population(artifact, formal)
    dimensions, random_dimensions, random_seeds = validate_protocol(artifact, formal)
    memories, queries = validate_input_cache_and_pairs(artifact, mode)
    population = require_mapping(artifact["population"], "population")
    validate_projection_bundle(artifact, max(dimensions))
    passing_dimensions, _ = validate_pca_results(
        artifact,
        dimensions,
        len(queries),
        int(population["heldout_probe_count"]),
        int(require_mapping(artifact["protocol"], "protocol")["pair_count"]),
        ground_truth_counts(queries),
    )
    validate_random_diagnostics(
        artifact,
        random_dimensions,
        random_seeds,
        int(
            require_number(
                require_mapping(artifact["protocol"], "protocol").get("pair_count"),
                "protocol.pair_count",
            )
        ),
    )
    validate_selection(artifact, mode, passing_dimensions)
    validate_resources(artifact, formal, artifact_path)
    return {
        "status": "passed",
        "mode": mode,
        "selection": require_mapping(artifact["selection"], "selection").get("status"),
        "minimum_passing_dimension": require_mapping(artifact["selection"], "selection").get("minimum_passing_dimension"),
        "passing_dimensions": passing_dimensions,
        "pca_candidate_count": len(dimensions),
        "random_diagnostic_count": len(random_dimensions) * len(random_seeds),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifact", type=Path, required=True)
    args = parser.parse_args()
    try:
        artifact = json.loads(args.artifact.read_text(encoding="utf-8"))
        result = validate_artifact(require_mapping(artifact, "artifact"), args.artifact.resolve())
        result["artifact"] = str(args.artifact)
        print(json.dumps(result, ensure_ascii=False, indent=2, sort_keys=True))
        return 0
    except Exception as error:
        print(f"[dimension-validator] {type(error).__name__}: {error}")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
