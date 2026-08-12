#!/usr/bin/env python3
"""Pre-registered representation-fidelity experiment for field-memory v2."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import platform
import resource
import signal
import subprocess
import sys
import time
import urllib.request
from pathlib import Path
from typing import Any

import numpy as np
from scipy.stats import spearmanr


SCHEMA_VERSION = 1
FORMAL_DATASET_SHA256 = "ed5a9836f277ea8242fce0bd477363bb21e81d606226dc607b92554d7b7d9901"
FORMAL_MODEL = "bge-m3:latest"
FORMAL_MODEL_DIGEST = "7907646426070047a77226ac3e684fbbe8410524f7b4a74d02837e43f2146bab"
FORMAL_OLLAMA_VERSION = "0.21.2"
FORMAL_SOURCE_DIMENSION = 1024
FORMAL_DIMENSIONS = [3, 8, 16, 32, 64, 128, 256, 384, 512, 768]
FORMAL_RANDOM_DIMENSIONS = [3, 32, 128, 512]
FORMAL_RANDOM_SEEDS = [20260812, 20260813, 20260814]
SPLIT_SEED = 20260812
PAIR_SEED = 20260812
FORMAL_PAIR_COUNT = 100_000
PREFLIGHT_PAIR_COUNT = 2_000
PREFLIGHT_DIMENSIONS = [3, 16, 32, 64]
PREFLIGHT_RANDOM_DIMENSIONS = [3, 32]
PREFLIGHT_RANDOM_SEEDS = [20260812]
FORMAL_MEMORY_COUNT = 6_611
FORMAL_QUERY_COUNT = 2_161
PREFLIGHT_RSS_LIMIT_BYTES = 2 * 1024**3
FORMAL_RSS_LIMIT_BYTES = 4 * 1024**3
ARTIFACT_LIMIT_BYTES = 1024**3
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


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(1024 * 1024):
            digest.update(chunk)
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
    return sha256_bytes(text.encode("utf-8"))


def stable_fraction(value: str) -> float:
    return int(sha256_bytes(value.encode("utf-8")), 16) / float(1 << 256)


def atomic_write_json(path: Path, payload: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(
        json.dumps(
            payload,
            ensure_ascii=False,
            indent=2,
            sort_keys=True,
            allow_nan=False,
        )
        + "\n",
        encoding="utf-8",
    )
    temporary.replace(path)


def serialized_json(payload: dict[str, Any]) -> bytes:
    return (
        json.dumps(
            payload,
            ensure_ascii=False,
            indent=2,
            sort_keys=True,
            allow_nan=False,
        )
        + "\n"
    ).encode("utf-8")


def get_json(url: str, timeout: float = 20.0) -> dict[str, Any]:
    with urllib.request.urlopen(url, timeout=timeout) as response:
        return json.loads(response.read().decode("utf-8"))


def post_json(url: str, payload: dict[str, Any], timeout: float = 180.0) -> dict[str, Any]:
    request = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return json.loads(response.read().decode("utf-8"))


def normalize_rows(matrix: np.ndarray, label: str) -> np.ndarray:
    matrix = np.asarray(matrix, dtype=np.float32)
    if matrix.ndim != 2 or not np.all(np.isfinite(matrix)):
        raise ValueError(f"{label}: expected a finite rank-2 matrix, got {matrix.shape}")
    norms = np.linalg.norm(matrix.astype(np.float64), axis=1)
    if np.any(~np.isfinite(norms)) or np.any(norms <= 1e-12):
        bad = int(np.sum((~np.isfinite(norms)) | (norms <= 1e-12)))
        raise ValueError(f"{label}: found {bad} non-finite or zero rows")
    return np.asarray(matrix / norms[:, None], dtype=np.float32)


class StrictEmbeddingCache:
    def __init__(self, path: Path, metadata: dict[str, Any]) -> None:
        self.path = path
        self.metadata = metadata
        self.dimension = int(metadata["dimension"])
        self.vectors: dict[str, np.ndarray] = {}
        if path.exists():
            try:
                with np.load(path, allow_pickle=False) as archive:
                    stored = json.loads(str(archive["metadata_json"].item()))
                    if stored != metadata:
                        raise ValueError(
                            "embedding cache provenance mismatch: "
                            f"stored={stored!r}, expected={metadata!r}"
                        )
                    keys = np.asarray(archive["keys"])
                    vectors = np.asarray(archive["vectors"], dtype=np.float32)
                    if vectors.shape != (len(keys), self.dimension):
                        raise ValueError(
                            f"invalid cache shape {vectors.shape}; expected ({len(keys)}, {self.dimension})"
                        )
                    vectors = normalize_rows(vectors, "embedding cache")
                    self.vectors = {
                        str(key): vectors[index].copy() for index, key in enumerate(keys)
                    }
            except Exception as error:
                raise ValueError(f"invalid formal embedding cache {path}: {error}") from error

    def flush(self) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        keys = np.asarray(sorted(self.vectors), dtype="U64")
        vectors = np.asarray([self.vectors[str(key)] for key in keys], dtype=np.float32)
        temporary = self.path.with_name(self.path.name + ".tmp.npz")
        np.savez_compressed(
            temporary,
            metadata_json=json.dumps(self.metadata, sort_keys=True, separators=(",", ":")),
            keys=keys,
            vectors=vectors,
        )
        temporary.replace(self.path)

    def embed(
        self,
        texts: list[str],
        url: str,
        model: str,
        batch_size: int,
        save_every_batches: int,
    ) -> tuple[np.ndarray, int]:
        missing: dict[str, str] = {}
        for text in texts:
            key = text_key(text)
            if key not in self.vectors:
                missing.setdefault(key, text)
        items = list(missing.items())
        for batch_index, start in enumerate(range(0, len(items), batch_size), start=1):
            batch = items[start : start + batch_size]
            payload = post_json(url, {"model": model, "input": [text for _, text in batch]})
            raw = payload.get("embeddings")
            if not isinstance(raw, list) or len(raw) != len(batch):
                raise ValueError(
                    f"Ollama returned {len(raw) if isinstance(raw, list) else 'invalid'} "
                    f"embeddings for batch of {len(batch)}"
                )
            vectors = normalize_rows(np.asarray(raw, dtype=np.float32), "Ollama response")
            if vectors.shape[1] != self.dimension:
                raise ValueError(
                    f"Ollama embedding width {vectors.shape[1]} != {self.dimension}"
                )
            for (key, _), vector in zip(batch, vectors):
                self.vectors[key] = vector.copy()
            completed = min(start + len(batch), len(items))
            if batch_index % save_every_batches == 0 or completed == len(items):
                self.flush()
                print(f"[dimension] embedding cache: {completed}/{len(items)} new vectors", flush=True)
        result = np.asarray([self.vectors[text_key(text)] for text in texts], dtype=np.float32)
        return normalize_rows(result, "cached embeddings"), len(items)


def model_provenance(base_url: str, model: str) -> dict[str, Any]:
    version = str(get_json(base_url + "/api/version").get("version", ""))
    tags = get_json(base_url + "/api/tags").get("models", [])
    match = next((entry for entry in tags if entry.get("name") == model), None)
    if match is None:
        raise ValueError(f"Ollama model {model!r} is not installed")
    return {
        "ollama_version": version,
        "model": model,
        "model_digest": str(match.get("digest", "")),
        "details": match.get("details", {}),
    }


def parse_csv_ints(value: str) -> list[int]:
    result = [int(item.strip()) for item in value.split(",") if item.strip()]
    if not result or len(result) != len(set(result)) or any(item <= 0 for item in result):
        raise argparse.ArgumentTypeError("expected unique positive comma-separated integers")
    return result


def deterministic_limit(rows: list[dict[str, Any]], limit: int, namespace: str) -> list[dict[str, Any]]:
    if limit <= 0 or limit >= len(rows):
        return list(rows)
    return sorted(rows, key=lambda row: text_key(f"{namespace}:{row['id']}"))[:limit]


def memory_group(row: dict[str, Any]) -> str:
    if row["source"] == "locomo":
        return f"locomo:{row['source_sample']}"
    return f"project:{row['source_path']}"


def validate_dataset_rows(
    memories: list[dict[str, Any]], queries: list[dict[str, Any]]
) -> None:
    memory_ids: set[str] = set()
    for index, row in enumerate(memories):
        if not isinstance(row, dict):
            raise ValueError(f"memory[{index}] must be an object")
        for key in ("id", "text", "source"):
            if not isinstance(row.get(key), str) or not row[key]:
                raise ValueError(f"memory[{index}].{key} must be a non-empty string")
        if row["source"] == "locomo":
            group_key = "source_sample"
        elif row["source"] == "project":
            group_key = "source_path"
        else:
            raise ValueError(f"memory[{index}] has unknown source {row['source']!r}")
        if not isinstance(row.get(group_key), str) or not row[group_key]:
            raise ValueError(f"memory[{index}].{group_key} must be a non-empty string")
        if row["id"] in memory_ids:
            raise ValueError(f"duplicate memory id {row['id']!r}")
        memory_ids.add(row["id"])

    query_ids: set[str] = set()
    for index, row in enumerate(queries):
        if not isinstance(row, dict):
            raise ValueError(f"query[{index}] must be an object")
        for key in ("id", "text", "source"):
            if not isinstance(row.get(key), str) or not row[key]:
                raise ValueError(f"query[{index}].{key} must be a non-empty string")
        if row["source"] not in {"locomo", "project"}:
            raise ValueError(f"query[{index}] has unknown source {row['source']!r}")
        relevant = row.get("relevant_ids")
        if (
            not isinstance(relevant, list)
            or not relevant
            or any(not isinstance(item, str) or not item for item in relevant)
        ):
            raise ValueError(f"query[{index}].relevant_ids must be non-empty strings")
        unknown = set(relevant) - memory_ids
        if unknown:
            raise ValueError(f"query[{index}] references unknown memories: {sorted(unknown)[:3]}")
        if row["id"] in query_ids:
            raise ValueError(f"duplicate query id {row['id']!r}")
        query_ids.add(row["id"])


def preflight_subset(
    memories: list[dict[str, Any]], queries: list[dict[str, Any]]
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    selected_queries: list[dict[str, Any]] = []
    for source in ("locomo", "project"):
        source_queries = [row for row in queries if row["source"] == source]
        selected_queries.extend(
            sorted(
                source_queries,
                key=lambda row: text_key(f"preflight-query:{row['id']}"),
            )[:16]
        )
    required_ids = {
        memory_id
        for query in selected_queries
        for memory_id in query["relevant_ids"]
    }
    if len(required_ids) > 128:
        raise ValueError("preflight query relevance requires more than 128 memories")
    extras = sorted(
        (row for row in memories if row["id"] not in required_ids),
        key=lambda row: text_key(f"preflight-memory:{row['id']}"),
    )[: 128 - len(required_ids)]
    selected_ids = required_ids | {row["id"] for row in extras}
    selected_memories = [row for row in memories if row["id"] in selected_ids]
    if len(selected_memories) != 128 or len(selected_queries) != 32:
        raise ValueError("preflight subset construction did not produce 128 memories / 32 queries")
    return selected_memories, selected_queries


def fit_mask(memories: list[dict[str, Any]]) -> np.ndarray:
    return np.asarray(
        [stable_fraction(f"{SPLIT_SEED}:{memory_group(row)}") < 0.8 for row in memories],
        dtype=bool,
    )


def heldout_probe_indices(
    memories: list[dict[str, Any]], mask: np.ndarray, count: int
) -> np.ndarray:
    candidates = [index for index, is_fit in enumerate(mask) if not is_fit]
    candidates.sort(key=lambda index: text_key(f"probe:{SPLIT_SEED}:{memories[index]['id']}"))
    if not candidates:
        raise ValueError("group split produced no heldout memories")
    return np.asarray(candidates[:count], dtype=np.int64)


def canonicalize_component_signs(components: np.ndarray) -> np.ndarray:
    result = components.copy()
    for column in range(result.shape[1]):
        pivot = int(np.argmax(np.abs(result[:, column])))
        if result[pivot, column] < 0:
            result[:, column] *= -1
    return result


def fit_spherical_pca(
    vectors: np.ndarray, mask: np.ndarray, max_dimension: int
) -> tuple[np.ndarray, np.ndarray]:
    fit = np.asarray(vectors[mask], dtype=np.float64)
    if len(fit) <= max_dimension:
        raise ValueError(
            f"spherical PCA needs more fit rows than max dimension: {len(fit)} <= {max_dimension}"
        )
    second_moment = fit.T @ fit
    eigenvalues, eigenvectors = np.linalg.eigh(second_moment)
    order = np.argsort(-eigenvalues, kind="stable")
    eigenvalues = np.maximum(eigenvalues[order], 0.0)
    components = canonicalize_component_signs(eigenvectors[:, order[:max_dimension]])
    return np.asarray(components, dtype=np.float32), np.asarray(eigenvalues, dtype=np.float64)


def save_projection_bundle(
    path: Path, components: np.ndarray, eigenvalues: np.ndarray, metadata: dict[str, Any]
) -> tuple[str, str]:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + ".tmp.npz")
    np.savez_compressed(
        temporary,
        components=np.asarray(components, dtype=np.float32),
        eigenvalues=np.asarray(eigenvalues, dtype=np.float64),
        metadata_json=json.dumps(metadata, sort_keys=True, separators=(",", ":")),
    )
    temporary.replace(path)
    return sha256_file(path), canonical_array_hash(metadata, components, eigenvalues)


def project(raw: np.ndarray, dimension: int, label: str) -> tuple[np.ndarray, int]:
    selected = np.asarray(raw[:, :dimension], dtype=np.float32)
    norms = np.linalg.norm(selected.astype(np.float64), axis=1)
    invalid = int(np.sum((~np.isfinite(norms)) | (norms <= 1e-12)))
    if invalid:
        return np.zeros_like(selected), invalid
    return normalize_rows(selected, label), 0


def exact_topk(
    probes: np.ndarray,
    catalog: np.ndarray,
    k: int,
    batch_size: int,
    exclude_catalog_indices: np.ndarray | None = None,
) -> np.ndarray:
    if k >= len(catalog):
        raise ValueError(f"top-k {k} must be smaller than catalog size {len(catalog)}")
    output = np.empty((len(probes), k), dtype=np.int64)
    for start in range(0, len(probes), batch_size):
        stop = min(start + batch_size, len(probes))
        similarities = np.asarray(probes[start:stop] @ catalog.T, dtype=np.float32)
        if exclude_catalog_indices is not None:
            rows = np.arange(stop - start)
            similarities[rows, exclude_catalog_indices[start:stop]] = -np.inf
        output[start:stop] = np.argsort(
            -similarities, axis=1, kind="stable"
        )[:, :k]
    return output


def overlap_recall(reference: np.ndarray, candidate: np.ndarray, k: int) -> np.ndarray:
    return np.asarray(
        [len(set(left[:k]).intersection(right[:k])) / k for left, right in zip(reference, candidate)],
        dtype=np.float64,
    )


def overlap_jaccard(reference: np.ndarray, candidate: np.ndarray, k: int) -> np.ndarray:
    result = []
    for left, right in zip(reference, candidate):
        left_set, right_set = set(left[:k]), set(right[:k])
        result.append(len(left_set & right_set) / len(left_set | right_set))
    return np.asarray(result, dtype=np.float64)


def distribution_summary(values: np.ndarray) -> dict[str, float | int]:
    values = np.asarray(values, dtype=np.float64)
    if values.size == 0 or not np.all(np.isfinite(values)):
        raise ValueError("cannot summarize an empty or non-finite distribution")
    return {
        "count": int(values.size),
        "mean": float(np.mean(values)),
        "p50": float(np.percentile(values, 50)),
        "p95": float(np.percentile(values, 95)),
        "p99": float(np.percentile(values, 99)),
        "max": float(np.max(values)),
    }


def sample_pairs(count: int, indices: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    indices = np.asarray(indices, dtype=np.int64)
    memory_count = len(indices)
    pair_space = memory_count * (memory_count - 1) // 2
    if count > pair_space:
        raise ValueError(
            f"cannot draw {count} unique unordered pairs from {memory_count} memories"
        )
    rng = np.random.default_rng(PAIR_SEED)
    seen: set[tuple[int, int]] = set()
    ordered: list[tuple[int, int]] = []
    while len(ordered) < count:
        remaining = count - len(ordered)
        left = rng.integers(0, memory_count, size=remaining * 2, dtype=np.int64)
        right = rng.integers(0, memory_count - 1, size=remaining * 2, dtype=np.int64)
        right += right >= left
        for a, b in zip(left, right):
            pair = (min(int(a), int(b)), max(int(a), int(b)))
            if pair in seen:
                continue
            seen.add(pair)
            ordered.append(pair)
            if len(ordered) == count:
                break
    return (
        indices[np.asarray([pair[0] for pair in ordered], dtype=np.int64)],
        indices[np.asarray([pair[1] for pair in ordered], dtype=np.int64)],
    )


def pair_metrics(
    reference: np.ndarray,
    candidate: np.ndarray,
    left: np.ndarray,
    right: np.ndarray,
) -> dict[str, Any]:
    reference_cosine = np.sum(reference[left] * reference[right], axis=1, dtype=np.float64)
    candidate_cosine = np.sum(candidate[left] * candidate[right], axis=1, dtype=np.float64)
    cosine_error = np.abs(candidate_cosine - reference_cosine)
    reference_angle = np.arccos(np.clip(reference_cosine, -1.0, 1.0))
    candidate_angle = np.arccos(np.clip(candidate_cosine, -1.0, 1.0))
    angular_error = np.abs(candidate_angle - reference_angle)
    correlation = float(spearmanr(reference_cosine, candidate_cosine).statistic)
    if not math.isfinite(correlation):
        raise ValueError("pair cosine Spearman correlation is not finite")
    boundaries = np.quantile(reference_cosine, [0.0, 0.2, 0.4, 0.6, 0.8, 1.0])
    strata: list[dict[str, Any]] = []
    for index in range(5):
        if index == 4:
            selected = (reference_cosine >= boundaries[index]) & (
                reference_cosine <= boundaries[index + 1]
            )
        else:
            selected = (reference_cosine >= boundaries[index]) & (
                reference_cosine < boundaries[index + 1]
            )
        if not np.any(selected):
            raise ValueError(f"reference cosine quintile {index} is empty")
        strata.append(
            {
                "index": index,
                "count": int(np.sum(selected)),
                "reference_cosine_min": float(boundaries[index]),
                "reference_cosine_max": float(boundaries[index + 1]),
                "cosine_abs_error": distribution_summary(cosine_error[selected]),
                "angular_abs_error_rad": distribution_summary(angular_error[selected]),
            }
        )
    return {
        "count": int(len(left)),
        "cosine_abs_error": distribution_summary(cosine_error),
        "angular_abs_error_rad": distribution_summary(angular_error),
        "cosine_spearman": correlation,
        "reference_cosine_quintiles": strata,
    }


def local_angular_error(
    reference_queries: np.ndarray,
    reference_memories: np.ndarray,
    candidate_queries: np.ndarray,
    candidate_memories: np.ndarray,
    query_neighbors: np.ndarray,
    memory_probe_indices: np.ndarray,
    memory_neighbors: np.ndarray,
) -> dict[str, float]:
    query_rows = np.repeat(np.arange(len(reference_queries)), 10)
    query_cols = query_neighbors[:, :10].reshape(-1)
    ref_query_cos = np.sum(
        reference_queries[query_rows] * reference_memories[query_cols], axis=1, dtype=np.float64
    )
    cand_query_cos = np.sum(
        candidate_queries[query_rows] * candidate_memories[query_cols], axis=1, dtype=np.float64
    )
    memory_rows = np.repeat(memory_probe_indices, 10)
    memory_cols = memory_neighbors[:, :10].reshape(-1)
    ref_memory_cos = np.sum(
        reference_memories[memory_rows] * reference_memories[memory_cols], axis=1, dtype=np.float64
    )
    cand_memory_cos = np.sum(
        candidate_memories[memory_rows] * candidate_memories[memory_cols], axis=1, dtype=np.float64
    )
    reference_cosine = np.concatenate([ref_query_cos, ref_memory_cos])
    candidate_cosine = np.concatenate([cand_query_cos, cand_memory_cos])
    error = np.abs(
        np.arccos(np.clip(candidate_cosine, -1.0, 1.0))
        - np.arccos(np.clip(reference_cosine, -1.0, 1.0))
    )
    return distribution_summary(error)


def ground_truth_recall_at_5(
    queries: list[dict[str, Any]], top_indices: np.ndarray, memory_ids: list[str]
) -> dict[str, dict[str, Any]]:
    available = set(memory_ids)
    rows: dict[str, list[float]] = {"overall": [], "locomo": [], "project": []}
    for query, indices in zip(queries, top_indices):
        relevant = list(
            dict.fromkeys(
                item for item in query.get("relevant_ids", []) if item in available
            )
        )
        if not relevant:
            continue
        retrieved = {memory_ids[int(index)] for index in indices[:5]}
        recall = len(retrieved.intersection(relevant)) / len(relevant)
        rows["overall"].append(recall)
        rows.setdefault(str(query.get("source", "unknown")), []).append(recall)
    return {
        key: {"count": len(values), "value": float(np.mean(values)) if values else None}
        for key, values in rows.items()
    }


def recall_ratios(
    reference: dict[str, dict[str, Any]], candidate: dict[str, dict[str, Any]]
) -> dict[str, float | None]:
    result: dict[str, float | None] = {}
    for key in ("overall", "locomo", "project"):
        baseline = reference[key]["value"]
        value = candidate[key]["value"]
        result[key] = None if baseline in (None, 0.0) or value is None else float(value / baseline)
    return result


def gate_result(metrics: dict[str, Any], invalid_rows: int) -> dict[str, Any]:
    values: dict[str, float | int | None] = {
        "invalid_projected_rows": invalid_rows,
        "global_cosine_abs_error_p95": metrics["global_pairs"]["cosine_abs_error"]["p95"],
        "global_cosine_spearman": metrics["global_pairs"]["cosine_spearman"],
        "local_angular_abs_error_p95": metrics["local_angular_abs_error_rad"]["p95"],
        "query_neighbor_recall_at_10_mean": metrics["query_neighbors"]["recall_at_10"]["mean"],
        "query_neighbor_recall_at_50_mean": metrics["query_neighbors"]["recall_at_50"]["mean"],
        "heldout_memory_neighbor_recall_at_10_mean": metrics["heldout_memory_neighbors"]["recall_at_10"]["mean"],
        "ground_truth_recall_at_5_ratio_overall": metrics["ground_truth_recall_at_5_ratio"]["overall"],
        "ground_truth_recall_at_5_ratio_locomo": metrics["ground_truth_recall_at_5_ratio"]["locomo"],
        "ground_truth_recall_at_5_ratio_project": metrics["ground_truth_recall_at_5_ratio"]["project"],
    }
    checks: list[dict[str, Any]] = []
    for threshold_name, threshold in THRESHOLDS.items():
        if threshold_name.endswith("_max"):
            metric_name = threshold_name[: -len("_max")]
            operator = "<="
            passed = values[metric_name] is not None and values[metric_name] <= threshold
        else:
            metric_name = threshold_name[: -len("_min")]
            operator = ">="
            passed = values[metric_name] is not None and values[metric_name] >= threshold
        checks.append(
            {
                "metric": metric_name,
                "value": values[metric_name],
                "operator": operator,
                "threshold": threshold,
                "pass": bool(passed),
            }
        )
    return {"pass": all(item["pass"] for item in checks), "checks": checks}


def evaluate_candidate(
    reference_memories: np.ndarray,
    reference_queries: np.ndarray,
    candidate_memories: np.ndarray,
    candidate_queries: np.ndarray,
    queries: list[dict[str, Any]],
    memory_ids: list[str],
    global_left: np.ndarray,
    global_right: np.ndarray,
    native_query_top50: np.ndarray,
    memory_probe_indices: np.ndarray,
    native_memory_top10: np.ndarray,
    native_ground_truth: dict[str, dict[str, Any]],
    top_batch_size: int,
) -> dict[str, Any]:
    query_top50 = exact_topk(candidate_queries, candidate_memories, 50, top_batch_size)
    probe_top10 = exact_topk(
        candidate_memories[memory_probe_indices],
        candidate_memories,
        10,
        top_batch_size,
        exclude_catalog_indices=memory_probe_indices,
    )
    query_recall10 = overlap_recall(native_query_top50, query_top50, 10)
    query_recall50 = overlap_recall(native_query_top50, query_top50, 50)
    query_jaccard10 = overlap_jaccard(native_query_top50, query_top50, 10)
    memory_recall10 = overlap_recall(native_memory_top10, probe_top10, 10)
    candidate_ground_truth = ground_truth_recall_at_5(queries, query_top50, memory_ids)
    return {
        "global_pairs": pair_metrics(
            reference_memories, candidate_memories, global_left, global_right
        ),
        "local_angular_abs_error_rad": local_angular_error(
            reference_queries,
            reference_memories,
            candidate_queries,
            candidate_memories,
            native_query_top50,
            memory_probe_indices,
            native_memory_top10,
        ),
        "query_neighbors": {
            "recall_at_10": distribution_summary(query_recall10),
            "recall_at_50": distribution_summary(query_recall50),
            "jaccard_at_10": distribution_summary(query_jaccard10),
        },
        "heldout_memory_neighbors": {
            "probe_count": int(len(memory_probe_indices)),
            "recall_at_10": distribution_summary(memory_recall10),
        },
        "ground_truth_recall_at_5": candidate_ground_truth,
        "ground_truth_recall_at_5_ratio": recall_ratios(
            native_ground_truth, candidate_ground_truth
        ),
    }


def git_metadata(workdir: Path) -> dict[str, Any]:
    def capture(arguments: list[str]) -> str:
        completed = subprocess.run(
            arguments,
            cwd=workdir,
            check=True,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        return completed.stdout.strip()

    return {
        "commit": capture(["git", "rev-parse", "HEAD"]),
        "status_short": capture(["git", "status", "--short"]).splitlines(),
    }


def peak_rss_bytes() -> int:
    value = int(resource.getrusage(resource.RUSAGE_SELF).ru_maxrss)
    return value if platform.system() == "Darwin" else value * 1024


def check_runtime_limits(args: argparse.Namespace, started: float, checkpoint: str) -> None:
    elapsed = time.monotonic() - started
    if elapsed > args.wall_time_limit_seconds:
        raise TimeoutError(
            f"wall-time limit reached at {checkpoint}: "
            f"{elapsed:.1f}s > {args.wall_time_limit_seconds}s"
        )
    rss_limit = (
        FORMAL_RSS_LIMIT_BYTES if args.mode == "formal" else PREFLIGHT_RSS_LIMIT_BYTES
    )
    rss = peak_rss_bytes()
    if rss > rss_limit:
        raise MemoryError(
            f"RSS limit reached at {checkpoint}: {rss} bytes > {rss_limit} bytes"
        )


def matrices_sha256(*matrices: np.ndarray) -> str:
    digest = hashlib.sha256()
    for matrix in matrices:
        contiguous = np.ascontiguousarray(matrix, dtype=np.float32)
        digest.update(str(contiguous.shape).encode("ascii"))
        digest.update(b"\0")
        digest.update(contiguous.tobytes(order="C"))
    return digest.hexdigest()


def protocol_check(args: argparse.Namespace, dataset_sha256: str, counts: tuple[int, int]) -> None:
    if dataset_sha256 != FORMAL_DATASET_SHA256:
        raise ValueError(f"frozen dataset SHA mismatch: {dataset_sha256}")
    if counts != (FORMAL_MEMORY_COUNT, FORMAL_QUERY_COUNT):
        raise ValueError(f"frozen dataset counts mismatch: {counts}")
    expected = (
        {
            "dimensions": FORMAL_DIMENSIONS,
            "random_dimensions": FORMAL_RANDOM_DIMENSIONS,
            "random_seeds": FORMAL_RANDOM_SEEDS,
            "pair_count": FORMAL_PAIR_COUNT,
            "memory_probes": 512,
            "limit_memories": 0,
            "limit_queries": 0,
            "wall_time_limit_seconds": 3600,
        }
        if args.mode == "formal"
        else {
            "dimensions": PREFLIGHT_DIMENSIONS,
            "random_dimensions": PREFLIGHT_RANDOM_DIMENSIONS,
            "random_seeds": PREFLIGHT_RANDOM_SEEDS,
            "pair_count": PREFLIGHT_PAIR_COUNT,
            "memory_probes": 32,
            "limit_memories": 128,
            "limit_queries": 32,
            "wall_time_limit_seconds": 600,
        }
    )
    expected.update(
        {
            "ollama_base_url": "http://127.0.0.1:11434",
            "embedding_url": "http://127.0.0.1:11434/api/embed",
            "embedding_batch_size": 32,
            "cache_save_every_batches": 10,
            "top_batch_size": 128,
        }
    )
    for name, value in expected.items():
        if getattr(args, name) != value:
            raise ValueError(f"{args.mode} protocol mismatch for {name}: {getattr(args, name)!r} != {value!r}")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mode", choices=["preflight", "formal"], required=True)
    parser.add_argument(
        "--dataset",
        type=Path,
        default=Path("experiments/v2_feasibility/dataset_scale.json"),
    )
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--projection-dir", type=Path, required=True)
    parser.add_argument("--ollama-base-url", default="http://127.0.0.1:11434")
    parser.add_argument("--embedding-url", default="http://127.0.0.1:11434/api/embed")
    parser.add_argument("--model", default=FORMAL_MODEL)
    parser.add_argument("--source-dimension", type=int, default=FORMAL_SOURCE_DIMENSION)
    parser.add_argument("--embedding-batch-size", type=int, default=32)
    parser.add_argument("--cache-save-every-batches", type=int, default=10)
    parser.add_argument(
        "--dimensions", type=parse_csv_ints, default=FORMAL_DIMENSIONS
    )
    parser.add_argument(
        "--random-dimensions", type=parse_csv_ints, default=FORMAL_RANDOM_DIMENSIONS
    )
    parser.add_argument(
        "--random-seeds", type=parse_csv_ints, default=FORMAL_RANDOM_SEEDS
    )
    parser.add_argument("--pair-count", type=int, default=FORMAL_PAIR_COUNT)
    parser.add_argument("--memory-probes", type=int, default=512)
    parser.add_argument("--top-batch-size", type=int, default=128)
    parser.add_argument("--limit-memories", type=int, default=0)
    parser.add_argument("--limit-queries", type=int, default=0)
    parser.add_argument("--wall-time-limit-seconds", type=int, default=3600)
    return parser


def run(args: argparse.Namespace) -> dict[str, Any]:
    started = time.monotonic()
    if args.pair_count <= 0 or args.memory_probes <= 0 or args.top_batch_size <= 0:
        raise ValueError("pair count, memory probes, and top-k batch size must be positive")
    if args.embedding_batch_size <= 0 or args.cache_save_every_batches <= 0:
        raise ValueError("embedding batch sizes must be positive")
    if args.wall_time_limit_seconds <= 0:
        raise ValueError("wall-time limit must be positive")
    all_dimensions = args.dimensions + args.random_dimensions
    if any(dimension >= args.source_dimension for dimension in all_dimensions):
        raise ValueError("all projected dimensions must be smaller than source dimension")
    for name in (
        "OMP_NUM_THREADS",
        "OPENBLAS_NUM_THREADS",
        "VECLIB_MAXIMUM_THREADS",
        "NUMEXPR_NUM_THREADS",
    ):
        if os.environ.get(name) != "1":
            raise ValueError(f"controlled execution requires {name}=1")
    dataset_sha256 = sha256_file(args.dataset)
    dataset = json.loads(args.dataset.read_text(encoding="utf-8"))
    all_memories = list(dataset["memories"])
    all_queries = list(dataset["queries"])
    validate_dataset_rows(all_memories, all_queries)
    protocol_check(args, dataset_sha256, (len(all_memories), len(all_queries)))

    if args.mode == "preflight":
        memories, queries = preflight_subset(all_memories, all_queries)
    else:
        memories, queries = list(all_memories), list(all_queries)
    if len(memories) <= 50 or not queries:
        raise ValueError("experiment requires more than 50 memories and at least one query")

    model = model_provenance(args.ollama_base_url.rstrip("/"), args.model)
    if model["model_digest"] != FORMAL_MODEL_DIGEST:
        raise ValueError(f"model digest mismatch: {model['model_digest']}")
    if model["ollama_version"] != FORMAL_OLLAMA_VERSION:
        raise ValueError(f"Ollama version mismatch: {model['ollama_version']}")
    if args.model != FORMAL_MODEL or args.source_dimension != FORMAL_SOURCE_DIMENSION:
        raise ValueError("model name and source dimension are frozen by the protocol")

    cache_metadata = {
        "schema_version": 1,
        "dataset_sha256": dataset_sha256,
        "model": args.model,
        "model_digest": model["model_digest"],
        "ollama_version": model["ollama_version"],
        "dimension": args.source_dimension,
    }
    cache = StrictEmbeddingCache(args.cache, cache_metadata)
    memory_texts = [row["text"] for row in memories]
    query_texts = [row["text"] for row in queries]
    print(f"[dimension] embedding {len(memory_texts)} memories", flush=True)
    memory_vectors, memory_misses = cache.embed(
        memory_texts,
        args.embedding_url,
        args.model,
        args.embedding_batch_size,
        args.cache_save_every_batches,
    )
    print(f"[dimension] embedding {len(query_texts)} queries", flush=True)
    query_vectors, query_misses = cache.embed(
        query_texts,
        args.embedding_url,
        args.model,
        args.embedding_batch_size,
        args.cache_save_every_batches,
    )
    cache.flush()
    check_runtime_limits(args, started, "embedding")
    with np.load(args.cache, allow_pickle=False) as cache_archive:
        cache_content_sha256 = canonical_array_hash(
            cache_metadata,
            np.asarray(cache_archive["keys"]),
            np.asarray(cache_archive["vectors"], dtype=np.float32),
        )

    mask = fit_mask(memories)
    probes = heldout_probe_indices(memories, mask, min(args.memory_probes, len(memories)))
    max_dimension = max(args.dimensions)
    print(
        f"[dimension] fitting spherical PCA: fit={int(np.sum(mask))}, "
        f"heldout={int(np.sum(~mask))}, max_dim={max_dimension}",
        flush=True,
    )
    components, eigenvalues = fit_spherical_pca(memory_vectors, mask, max_dimension)
    eigengap_floor = PCA_EIGENGAP_REL_TOL * max(float(eigenvalues[0]), 1.0)
    boundary_eigengaps: dict[str, float] = {}
    for dimension in args.dimensions:
        gap = float(eigenvalues[dimension - 1] - eigenvalues[dimension])
        if not math.isfinite(gap) or gap <= eigengap_floor:
            raise ValueError(
                f"PCA boundary d={dimension} is not uniquely defined: "
                f"eigengap={gap}, floor={eigengap_floor}"
            )
        boundary_eigengaps[str(dimension)] = gap
    check_runtime_limits(args, started, "PCA fit")
    split_hash = sha256_bytes(
        "\n".join(
            f"{row['id']}:{'fit' if selected else 'heldout'}"
            for row, selected in zip(memories, mask)
        ).encode("utf-8")
    )
    bundle_metadata = {
        "algorithm": "uncentered_spherical_pca_eigh_v1",
        "dataset_sha256": dataset_sha256,
        "model_digest": model["model_digest"],
        "split_seed": SPLIT_SEED,
        "split_sha256": split_hash,
        "source_dimension": args.source_dimension,
        "max_target_dimension": max_dimension,
        "eigensolver": "numpy.linalg.eigh",
        "eigengap_relative_tolerance": PCA_EIGENGAP_REL_TOL,
        "boundary_eigengaps": boundary_eigengaps,
    }
    bundle_path = args.projection_dir / "spherical_pca.npz"
    bundle_sha256, bundle_content_sha256 = save_projection_bundle(
        bundle_path, components, eigenvalues, bundle_metadata
    )

    memory_ids = [row["id"] for row in memories]
    pair_scope = "heldout_memory" if args.mode == "formal" else "all_memory"
    pair_candidates = (
        np.flatnonzero(~mask)
        if args.mode == "formal"
        else np.arange(len(memories), dtype=np.int64)
    )
    global_left, global_right = sample_pairs(args.pair_count, pair_candidates)
    pair_ids_sha256 = sha256_bytes(
        np.column_stack((global_left, global_right)).astype("<i8", copy=False).tobytes()
    )
    print("[dimension] computing native exact neighbors", flush=True)
    native_query_top50 = exact_topk(
        query_vectors, memory_vectors, 50, args.top_batch_size
    )
    native_memory_top10 = exact_topk(
        memory_vectors[probes],
        memory_vectors,
        10,
        args.top_batch_size,
        exclude_catalog_indices=probes,
    )
    native_ground_truth = ground_truth_recall_at_5(
        queries, native_query_top50, memory_ids
    )
    native_vectors_sha256 = matrices_sha256(memory_vectors, query_vectors)
    check_runtime_limits(args, started, "native reference")

    raw_memory_pca = np.asarray(memory_vectors @ components, dtype=np.float32)
    raw_query_pca = np.asarray(query_vectors @ components, dtype=np.float32)
    total_energy = float(np.sum(eigenvalues))
    pca_results: list[dict[str, Any]] = []
    for dimension in args.dimensions:
        check_runtime_limits(args, started, f"before PCA d={dimension}")
        print(f"[dimension] spherical PCA d={dimension}", flush=True)
        candidate_memories, invalid_memory = project(
            raw_memory_pca, dimension, f"PCA memories d={dimension}"
        )
        candidate_queries, invalid_query = project(
            raw_query_pca, dimension, f"PCA queries d={dimension}"
        )
        invalid = invalid_memory + invalid_query
        if invalid:
            metrics: dict[str, Any] = {}
            gate = {"pass": False, "checks": [], "reason": "invalid_projected_rows"}
        else:
            metrics = evaluate_candidate(
                memory_vectors,
                query_vectors,
                candidate_memories,
                candidate_queries,
                queries,
                memory_ids,
                global_left,
                global_right,
                native_query_top50,
                probes,
                native_memory_top10,
                native_ground_truth,
                args.top_batch_size,
            )
            gate = gate_result(metrics, invalid)
        pca_results.append(
            {
                "projection": "spherical_pca",
                "eligible_for_production_gate": True,
            "target_dimension": dimension,
                "invalid_projected_rows": invalid,
                "retained_second_moment": float(
                    np.sum(eigenvalues[:dimension]) / total_energy
                ),
                "projection_bundle_sha256": bundle_sha256,
                "metrics": metrics,
                "gate": gate,
            }
        )
        check_runtime_limits(args, started, f"after PCA d={dimension}")

    random_results: list[dict[str, Any]] = []
    max_random_dimension = max(args.random_dimensions)
    for seed in args.random_seeds:
        check_runtime_limits(args, started, f"before random seed={seed}")
        print(f"[dimension] Gaussian diagnostic seed={seed}", flush=True)
        rng = np.random.default_rng(seed)
        matrix = np.asarray(
            rng.standard_normal((args.source_dimension, max_random_dimension)),
            dtype=np.float32,
        )
        matrix_sha256 = sha256_bytes(matrix.tobytes(order="C"))
        raw_random = np.asarray(memory_vectors @ matrix, dtype=np.float32)
        for dimension in args.random_dimensions:
            candidate, invalid = project(
                raw_random, dimension, f"random memories seed={seed} d={dimension}"
            )
            metrics = (
                {}
                if invalid
                else pair_metrics(memory_vectors, candidate, global_left, global_right)
            )
            random_results.append(
                {
                    "projection": "gaussian_random",
                    "eligible_for_production_gate": False,
                    "seed": seed,
                    "target_dimension": dimension,
                    "invalid_projected_rows": invalid,
                    "projection_matrix_sha256": matrix_sha256,
                    "global_pairs": metrics,
                }
            )
        check_runtime_limits(args, started, f"after random seed={seed}")

    passing = [
        row["target_dimension"] for row in pca_results if row["gate"].get("pass")
    ]
    if args.mode == "formal":
        selection = {
            "status": "dimension_fidelity_passed"
            if passing
            else "no_compressed_production_candidate",
            "projection": "spherical_pca" if passing else None,
            "minimum_passing_dimension": min(passing) if passing else None,
            "passing_dimensions": passing,
            "native_fallback_dimension": FORMAL_SOURCE_DIMENSION,
        }
    else:
        selection = {
            "status": "preflight_only",
            "minimum_passing_dimension": None,
            "passing_dimensions_observed": passing,
        }

    elapsed = time.monotonic() - started
    repository_root = Path(__file__).resolve().parents[2]
    return {
        "schema_version": SCHEMA_VERSION,
        "experiment_id": (
            f"v2-dimension-fidelity-{args.mode}-2026-08-12-"
            f"{int(time.time_ns())}"
        ),
        "mode": args.mode,
        "status": "complete",
        "decision_scope": "representation fidelity only; no v2 dynamics",
        "provenance": {
            "dataset_path": str(args.dataset),
            "dataset_id": dataset.get("dataset_id"),
            "dataset_sha256": dataset_sha256,
            "model": model,
            "embedding_url": args.embedding_url,
            "source_dimension": args.source_dimension,
            "cache_path": str(args.cache),
            "cache_sha256": sha256_file(args.cache),
            "cache_content_sha256": cache_content_sha256,
            "cache_vector_count": len(cache.vectors),
            "memory_cache_misses": memory_misses,
            "query_cache_misses": query_misses,
            "runner_sha256": sha256_file(Path(__file__)),
            "protocol_sha256": sha256_file(
                repository_root
                / "docs/specs/2026-08-12-v2-dimension-fidelity-experiment.md"
            ),
            "git": git_metadata(repository_root),
            "python": sys.version,
            "numpy": np.__version__,
            "scipy": __import__("scipy").__version__,
            "platform": platform.platform(),
            "thread_environment": {
                name: os.environ.get(name)
                for name in (
                    "OMP_NUM_THREADS",
                    "OPENBLAS_NUM_THREADS",
                    "VECLIB_MAXIMUM_THREADS",
                    "NUMEXPR_NUM_THREADS",
                )
            },
        },
        "population": {
            "memory_count": len(memories),
            "query_count": len(queries),
            "duplicate_relevance_entry_count": sum(
                len(query["relevant_ids"]) - len(set(query["relevant_ids"]))
                for query in queries
            ),
            "fit_memory_count": int(np.sum(mask)),
            "heldout_memory_count": int(np.sum(~mask)),
            "heldout_probe_count": len(probes),
            "fit_group_count": len(
                {memory_group(row) for row, selected in zip(memories, mask) if selected}
            ),
            "heldout_group_count": len(
                {memory_group(row) for row, selected in zip(memories, mask) if not selected}
            ),
            "split_seed": SPLIT_SEED,
            "split_sha256": split_hash,
            "probe_ids_sha256": sha256_bytes(
                "\n".join(memory_ids[int(index)] for index in probes).encode("utf-8")
            ),
        },
        "protocol": {
            "dimensions": args.dimensions,
            "random_dimensions": args.random_dimensions,
            "random_seeds": args.random_seeds,
            "pair_seed": PAIR_SEED,
            "pair_count": args.pair_count,
            "pair_scope": pair_scope,
            "pair_indices_sha256": pair_ids_sha256,
            "thresholds": THRESHOLDS,
            "candidate_projection": "uncentered_spherical_pca_eigh_v1",
            "random_projection": "numpy_pcg64_standard_normal_v1",
        },
        "native_reference": {
            "dimension": FORMAL_SOURCE_DIMENSION,
            "projection": "identity",
            "normalization": "finite_nonzero_l2_float32_v1",
            "vectors_sha256": native_vectors_sha256,
            "ground_truth_recall_at_5": native_ground_truth,
        },
        "spherical_pca": {
            "bundle_path": str(bundle_path),
            "bundle_sha256": bundle_sha256,
            "bundle_content_sha256": bundle_content_sha256,
            "bundle_metadata": bundle_metadata,
            "results": pca_results,
        },
        "gaussian_random_diagnostic": random_results,
        "selection": selection,
        "resources": {
            "wall_time_seconds": elapsed,
            "peak_rss_bytes": peak_rss_bytes(),
            "rss_limit_bytes": FORMAL_RSS_LIMIT_BYTES
            if args.mode == "formal"
            else PREFLIGHT_RSS_LIMIT_BYTES,
            "artifact_limit_bytes": ARTIFACT_LIMIT_BYTES,
            "cache_bytes": args.cache.stat().st_size,
            "projection_bytes": sum(
                path.stat().st_size
                for path in args.projection_dir.rglob("*")
                if path.is_file()
            ),
        },
    }


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    def deadline_handler(_signum: int, _frame: Any) -> None:
        raise TimeoutError(
            f"hard wall-time deadline reached: {args.wall_time_limit_seconds}s"
        )

    previous_handler = signal.signal(signal.SIGALRM, deadline_handler)
    signal.setitimer(signal.ITIMER_REAL, args.wall_time_limit_seconds)
    try:
        result = run(args)
        result["resources"]["result_json_bytes"] = 0
        result["resources"]["combined_artifact_bytes"] = 0
        for _ in range(8):
            result_bytes = len(serialized_json(result))
            combined_bytes = (
                result["resources"]["cache_bytes"]
                + result["resources"]["projection_bytes"]
                + result_bytes
            )
            previous = (
                result["resources"]["result_json_bytes"],
                result["resources"]["combined_artifact_bytes"],
            )
            current = (result_bytes, combined_bytes)
            result["resources"]["result_json_bytes"] = result_bytes
            result["resources"]["combined_artifact_bytes"] = combined_bytes
            if current == previous:
                break
        else:
            raise RuntimeError("result JSON resource-size fields did not stabilize")
        if combined_bytes > ARTIFACT_LIMIT_BYTES:
            raise OSError(
                f"artifact limit exceeded: {combined_bytes} bytes > {ARTIFACT_LIMIT_BYTES} bytes"
            )
        atomic_write_json(args.output, result)
        if args.output.stat().st_size != result["resources"]["result_json_bytes"]:
            raise RuntimeError("written result size differs from recorded result_json_bytes")
        print(
            f"[dimension] complete: selection={result['selection']['status']} "
            f"dimension={result['selection'].get('minimum_passing_dimension')}",
            flush=True,
        )
        return 0
    except Exception as error:
        atomic_write_json(
            args.output,
            {
                "schema_version": SCHEMA_VERSION,
                "mode": args.mode,
                "status": "failed",
                "error_type": type(error).__name__,
                "error": str(error),
            },
        )
        raise
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0.0)
        signal.signal(signal.SIGALRM, previous_handler)


if __name__ == "__main__":
    raise SystemExit(main())
