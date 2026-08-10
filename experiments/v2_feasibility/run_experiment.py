#!/usr/bin/env python3
"""Run the field-memory v2 feasibility observation.

The script deliberately keeps the v2 implementation observational.  It does
not claim to implement the unresolved design choices in the v2 note.  Every
choice made here is exposed in CONFIG and in results.json so that the result
can be rerun or challenged.
"""

from __future__ import annotations

import argparse
import copy
import datetime as dt
import hashlib
import json
import math
import os
import re
import time
import urllib.error
import urllib.request
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable

import numpy as np


ROOT = Path(__file__).resolve().parent
DATASET_PATH = ROOT / "dataset.json"
OUTPUT_PATH = ROOT / "results.json"


CONFIG: dict[str, Any] = {
    # These are operationalizations of open v2 choices, not claims from the
    # design note itself.
    "sample_budget": 96,
    "propagation_threshold": 0.06,
    "center_strength": 1.0,
    "relay_absorption_cap": 0.85,
    "collapse_rate": 0.005,
    "response_density_gain": 0.12,
    "direction_step_scale": 0.05,
    "direction_step_max": 0.12,
    "traditional_dense_weight": 0.75,
    "traditional_lexical_weight": 0.20,
    "traditional_recency_weight": 0.05,
    "top_k": 5,
}


def read_json(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def write_json(path: Path, value: Any) -> None:
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def normalize(vector: np.ndarray) -> np.ndarray:
    norm = float(np.linalg.norm(vector))
    if norm == 0.0:
        raise ValueError("received a zero embedding")
    return vector / norm


def cosine(left: np.ndarray, right: np.ndarray) -> float:
    return float(np.dot(left, right) / (np.linalg.norm(left) * np.linalg.norm(right)))


def tokenise(text: str) -> list[str]:
    """Small transparent tokenizer for the hybrid RAG lexical signal."""

    return re.findall(r"[A-Za-z0-9_#.-]+|[\u4e00-\u9fff]", text.lower())


def date_score(memory_date: str, as_of: str) -> float:
    memory_day = dt.date.fromisoformat(memory_date)
    query_day = dt.date.fromisoformat(as_of)
    age = max(0, (query_day - memory_day).days)
    return math.exp(-age / 180.0)


class HttpJsonClient:
    def __init__(self) -> None:
        self.calls: list[dict[str, Any]] = []

    def post(self, url: str, payload: dict[str, Any], headers: dict[str, str] | None = None) -> dict[str, Any]:
        body = json.dumps(payload).encode("utf-8")
        request = urllib.request.Request(
            url,
            data=body,
            method="POST",
            headers={"Content-Type": "application/json", **(headers or {})},
        )
        try:
            started = time.monotonic()
            with urllib.request.urlopen(request, timeout=180) as response:
                response_body = response.read()
                request_id = response.headers.get("x-request-id")
                self.calls.append({
                    "url": url,
                    "status": response.status,
                    "request_id": request_id,
                    "elapsed_ms": round((time.monotonic() - started) * 1000, 2),
                })
                return json.loads(response_body)
        except urllib.error.HTTPError as error:
            response_body = error.read().decode("utf-8", errors="replace")
            self.calls.append({
                "url": url,
                "status": error.code,
                "error": response_body[:500],
                "elapsed_ms": round((time.monotonic() - started) * 1000, 2),
            })
            raise RuntimeError(f"POST {url} returned HTTP {error.code}: {response_body[:500]}") from error


class OllamaEmbedder:
    def __init__(self, client: HttpJsonClient, url: str, model: str, dimension: int, batch_size: int = 32) -> None:
        self.client = client
        self.url = url
        self.model = model
        self.dimension = dimension
        self.batch_size = max(1, batch_size)

    def embed(self, texts: list[str]) -> np.ndarray:
        if not texts:
            return np.empty((0, self.dimension), dtype=np.float32)
        batches: list[np.ndarray] = []
        for start in range(0, len(texts), self.batch_size):
            batches.append(self._embed_batch(texts[start:start + self.batch_size]))
        return np.vstack(batches)

    def _embed_batch(self, texts: list[str]) -> np.ndarray:
        payload = self.client.post(self.url, {"model": self.model, "input": texts})
        vectors = payload.get("embeddings")
        if vectors is None:
            # Older Ollama versions expose /api/embeddings with a singular
            # prompt.  This fallback is still local and is not an LLM call.
            vectors = []
            singular_url = self.url.replace("/api/embed", "/api/embeddings")
            for text in texts:
                item = self.client.post(singular_url, {"model": self.model, "prompt": text})
                vectors.append(item.get("embedding"))
        if not isinstance(vectors, list) or len(vectors) != len(texts):
            raise ValueError(f"Ollama returned {len(vectors) if isinstance(vectors, list) else 'invalid'} embeddings for {len(texts)} texts")
        matrix = np.asarray(vectors, dtype=np.float32)
        if matrix.ndim != 2 or matrix.shape[1] != self.dimension:
            raise ValueError(f"expected embeddings of shape (*, {self.dimension}), got {matrix.shape}")
        return np.asarray([normalize(row) for row in matrix], dtype=np.float32)


class EmbeddingCache:
    """Small on-disk float32 cache keyed by model, dimension, and text hash."""

    def __init__(self, path: Path, model: str, dimension: int) -> None:
        self.path = path
        self.model = model
        self.dimension = dimension
        self.vectors: dict[str, np.ndarray] = {}
        if path.exists():
            try:
                with np.load(path, allow_pickle=False) as archive:
                    if archive.get("model", "") == model and int(archive.get("dimension", -1)) == dimension:
                        keys = archive.get("keys", np.empty((0,), dtype="U1"))
                        matrix = archive.get("vectors", np.empty((0, dimension), dtype=np.float32))
                        if matrix.ndim == 2 and matrix.shape[1] == dimension and len(keys) == len(matrix):
                            self.vectors = {str(key): matrix[index].copy() for index, key in enumerate(keys)}
            except (EOFError, OSError, ValueError, zipfile.BadZipFile) as error:
                print(f"[v2] ignoring invalid embedding cache {path}: {error}", flush=True)

    def key(self, text: str) -> str:
        return hashlib.sha256(text.encode("utf-8")).hexdigest()

    def embed(self, texts: list[str], embedder: OllamaEmbedder) -> tuple[np.ndarray, int]:
        missing_texts: list[str] = []
        missing_keys: list[str] = []
        result: list[np.ndarray | None] = []
        for text in texts:
            key = self.key(text)
            vector = self.vectors.get(key)
            if vector is None:
                missing_texts.append(text)
                missing_keys.append(key)
                result.append(None)
            else:
                result.append(vector)
        if missing_texts:
            batch_size = max(1, embedder.batch_size)
            for start in range(0, len(missing_texts), batch_size):
                batch_texts = missing_texts[start:start + batch_size]
                batch_keys = missing_keys[start:start + batch_size]
                batch_vectors = embedder.embed(batch_texts)
                for key, vector in zip(batch_keys, batch_vectors):
                    self.vectors[key] = vector.copy()
                self.flush()
                completed = start + len(batch_vectors)
                if completed == len(missing_texts) or completed % (batch_size * 10) == 0:
                    print(f"[v2] embedding cache progress: {completed}/{len(missing_texts)} new vectors", flush=True)
            # Fill the result from the cache rather than retaining a large
            # temporary matrix from the whole corpus.
            for index, text in enumerate(texts):
                if result[index] is None:
                    result[index] = self.vectors[self.key(text)]
        return np.asarray(result, dtype=np.float32), len(missing_texts)

    def flush(self) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        keys = np.asarray(list(self.vectors), dtype="U64")
        matrix = np.asarray([self.vectors[key] for key in keys], dtype=np.float32)
        temporary = self.path.with_name(self.path.name + ".tmp.npz")
        np.savez_compressed(temporary, model=self.model, dimension=self.dimension, keys=keys, vectors=matrix)
        temporary.replace(self.path)


class HybridRAG:
    """Mem0-shaped baseline: text memories + dense search + lexical + recency."""

    def __init__(self, memories: list[dict[str, Any]], vectors: np.ndarray, as_of: str = "2026-08-09") -> None:
        self.memories = copy.deepcopy(memories)
        self.vectors = vectors.copy()
        self.as_of = as_of
        self.df: dict[str, int] = {}
        self.token_sets: list[set[str]] = []
        self.memory_dates: np.ndarray = np.empty((0,), dtype=np.int64)
        self._rebuild_lexical_stats()

    def _rebuild_lexical_stats(self) -> None:
        self.df = {}
        self.token_sets = [set(tokenise(memory["text"])) for memory in self.memories]
        self.memory_dates = np.asarray([dt.date.fromisoformat(memory["date"]).toordinal() for memory in self.memories], dtype=np.int64)
        for text_tokens in self.token_sets:
            for token in text_tokens:
                self.df[token] = self.df.get(token, 0) + 1

    def _lexical_score(self, query_tokens: list[str], text_tokens: set[str]) -> float:
        if not query_tokens or not text_tokens:
            return 0.0
        n = max(1, len(self.memories))
        weighted_query = sum(math.log((n + 1) / (self.df.get(token, 0) + 1)) for token in set(query_tokens))
        matched = sum(
            math.log((n + 1) / (self.df.get(token, 0) + 1))
            for token in set(query_tokens)
            if token in text_tokens
        )
        return matched / weighted_query if weighted_query else 0.0

    def write(self, memory: dict[str, Any], vector: np.ndarray) -> None:
        self.memories.append(copy.deepcopy(memory))
        self.vectors = np.vstack([self.vectors, vector])
        self._rebuild_lexical_stats()

    def retrieve(self, query: dict[str, Any], top_k: int = 5) -> list[dict[str, Any]]:
        query_vector = query["vector"]
        query_tokens = tokenise(query["text"])
        if not self.memories:
            return []
        dense_scores = np.maximum(0.0, self.vectors @ query_vector)
        scores = np.empty(len(self.memories), dtype=np.float32)
        lexical_scores: list[float] = []
        query_day = dt.date.fromisoformat(query.get("as_of", self.as_of)).toordinal()
        recency_scores = np.exp(-np.maximum(0, query_day - self.memory_dates) / 180.0)
        for index, memory in enumerate(self.memories):
            lexical_scores.append(self._lexical_score(query_tokens, self.token_sets[index]))
            scores[index] = (
                CONFIG["traditional_dense_weight"] * float(dense_scores[index])
                + CONFIG["traditional_lexical_weight"] * lexical_scores[index]
                + CONFIG["traditional_recency_weight"] * float(recency_scores[index])
            )
        count = min(top_k, len(self.memories))
        candidate_indices = np.argpartition(-scores, count - 1)[:count]
        ranked_indices = sorted(candidate_indices.tolist(), key=lambda index: (-float(scores[index]), self.memories[index]["id"]))
        return [{
                "id": self.memories[index]["id"],
                "text": self.memories[index]["text"],
                "topic": self.memories[index]["topic"],
                "score": float(scores[index]),
                "dense": float(dense_scores[index]),
                "lexical": lexical_scores[index],
                "recency": float(recency_scores[index]),
            } for index in ranked_indices]


@dataclass
class Sample:
    anchor_id: str
    direction: np.ndarray
    density: float


class V2Field:
    """Operational observation of v2, not a production implementation."""

    def __init__(self, memories: list[dict[str, Any]], vectors: np.ndarray, config: dict[str, Any]) -> None:
        self.config = config
        self.anchors: list[dict[str, Any]] = []
        for memory, vector in zip(memories, vectors):
            self.anchors.append({
                "id": memory["id"],
                "text": memory["text"],
                "topic": memory["topic"],
                "direction": vector.copy(),
                "density": float(memory["seed_hits"]),
                "initial_density": float(memory["seed_hits"]),
            })
        self.events: list[dict[str, Any]] = []
        self.traces: list[dict[str, Any]] = []

    def clone(self) -> "V2Field":
        result = copy.copy(self)
        result.config = self.config.copy()
        result.anchors = copy.deepcopy(self.anchors)
        result.events = copy.deepcopy(self.events)
        result.traces = copy.deepcopy(self.traces)
        return result

    def state_summary(self) -> dict[str, Any]:
        densities = [a["density"] for a in self.anchors]
        return {
            "anchor_count": len(self.anchors),
            "event_count": len(self.events),
            "total_density": sum(densities),
            "mean_density": float(np.mean(densities)) if densities else 0.0,
            "max_density": max(densities) if densities else 0.0,
            "min_density": min(densities) if densities else 0.0,
        }

    def _samples(self) -> list[Sample]:
        if not self.anchors:
            return []
        densities = [max(0.01, float(anchor["density"])) for anchor in self.anchors]
        total_density = sum(densities)
        budget = max(1, int(self.config["sample_budget"]))
        counts = [0] * len(self.anchors)
        if len(self.anchors) <= budget:
            # Preserve one interaction point per anchor, then spend the
            # remaining budget in proportion to density.
            counts = [1] * len(self.anchors)
            extra = budget - len(self.anchors)
            if extra:
                ideal = [extra * density / total_density for density in densities]
                floors = [math.floor(value) for value in ideal]
                counts = [count + floor for count, floor in zip(counts, floors)]
                remainder = extra - sum(floors)
                order = sorted(range(len(ideal)), key=lambda index: (-(ideal[index] - floors[index]), index))
                for index in order[:remainder]:
                    counts[index] += 1
        else:
            # A large field cannot expose every anchor through a bounded
            # transient sample budget.  Systematic weighted sampling keeps
            # coverage spread across the field instead of selecting the first
            # N anchors on a tie, and makes that limitation observable.
            cumulative = 0.0
            anchor_index = 0
            next_position = total_density / (2.0 * budget)
            for _ in range(budget):
                while anchor_index < len(densities) - 1 and cumulative + densities[anchor_index] < next_position:
                    cumulative += densities[anchor_index]
                    anchor_index += 1
                counts[anchor_index] += 1
                next_position += total_density / budget
        samples: list[Sample] = []
        for anchor, replica_count in zip(self.anchors, counts):
            for _ in range(replica_count):
                samples.append(Sample(anchor["id"], anchor["direction"].copy(), anchor["density"]))
        return samples

    def perturb(self, text: str, vector: np.ndarray, operation: str) -> dict[str, Any]:
        """Read and write through one path, following the v2 strong equivalence."""

        before = self.state_summary()
        event = {"text": text, "operation": operation, "ordinal": len(self.events)}
        self.events.append(event)
        samples = self._samples()
        center_strength = float(self.config["center_strength"])
        remaining = center_strength
        ordered = sorted(samples, key=lambda sample: cosine(vector, sample.direction), reverse=True)
        # Keep only touched anchors.  Allocating a 1024-dimensional zero
        # vector for every anchor on every event makes a large-field audit
        # quadratic in memory traffic without changing the result.
        aggregate: dict[str, dict[str, Any]] = {}
        touched: set[str] = set()
        trace: list[dict[str, Any]] = []

        for sample in ordered:
            similarity = max(0.0, cosine(vector, sample.direction))
            received = remaining * similarity
            if received < self.config["propagation_threshold"]:
                break
            anchor_data = aggregate.setdefault(sample.anchor_id, {
                "response": 0.0,
                "max_cos": -1.0,
                "replicas": 0,
                "direction_sum": np.zeros_like(vector),
            })
            anchor_data["response"] += received
            anchor_data["max_cos"] = max(anchor_data["max_cos"], similarity)
            anchor_data["replicas"] += 1
            anchor_data["direction_sum"] += received * sample.direction
            touched.add(sample.anchor_id)
            absorbed = received * min(
                self.config["relay_absorption_cap"],
                sample.density / (sample.density + 2.0),
            )
            remaining -= absorbed
            trace.append({
                "anchor_id": sample.anchor_id,
                "similarity": similarity,
                "received": received,
                "absorbed": absorbed,
                "remaining": remaining,
            })
            if remaining < 0.005:
                break

        readout: list[dict[str, Any]] = []
        for anchor in self.anchors:
            data = aggregate.get(anchor["id"])
            if data is None or data["response"] <= 0.0:
                continue
            readout.append({
                "id": anchor["id"],
                "text": anchor["text"],
                "topic": anchor["topic"],
                "score": data["response"],
                "max_cos": max(0.0, data["max_cos"]),
                "replicas": data["replicas"],
                "touched": anchor["id"] in touched,
            })
        readout.sort(key=lambda item: (-item["score"], -item["max_cos"], item["id"]))
        # A v2 readout is a set of activated field regions, not a padded
        # fixed-width list.  Zero-response anchors are not read out; the
        # returned_count field makes this sparsity visible to the report.
        readout = readout[: int(self.config["top_k"])]

        # Feedback occurs only after the readout has been captured.  Unhit
        # anchors undergo the default collapse; hit anchors receive the
        # perturbation and a bounded slow direction update.
        drift_total = 0.0
        for anchor in self.anchors:
            data = aggregate.get(anchor["id"])
            old_density = anchor["density"]
            if data is None:
                anchor["density"] = max(0.05, old_density * (1.0 - self.config["collapse_rate"]))
            else:
                old_direction = anchor["direction"].copy()
                anchor["density"] = old_density * (1.0 - self.config["collapse_rate"] / 4.0)
                anchor["density"] += self.config["response_density_gain"] * data["response"]
                if data["response"] > 0.0:
                    target = normalize(data["direction_sum"])
                    step = min(
                        self.config["direction_step_max"],
                        self.config["direction_step_scale"] * data["response"] / (old_density + 1.0),
                    )
                    anchor["direction"] = normalize((1.0 - step) * old_direction + step * target)
                drift_total += 1.0 - cosine(old_direction, anchor["direction"])

        self.traces.append({"operation": operation, "text": text, "chain": trace, "readout_ids": [x["id"] for x in readout]})
        after = self.state_summary()
        return {
            "operation": operation,
            "text": text,
            "readout": readout,
            "chain": trace[:10],
            "chain_length": len(trace),
            "sample_count": len(samples),
            "sampled_anchor_count": len({sample.anchor_id for sample in samples}),
            "sample_coverage": len({sample.anchor_id for sample in samples}) / len(self.anchors) if self.anchors else 0.0,
            "remaining_influence": remaining,
            "touched_anchor_count": len(touched),
            "state_before": before,
            "state_after": after,
            "density_delta": after["total_density"] - before["total_density"],
            "mean_direction_drift": drift_total / len(self.anchors) if self.anchors else 0.0,
        }


def metric_row(retrieved: list[dict[str, Any]], relevant_ids: Iterable[str], k: int) -> dict[str, float]:
    relevant = set(relevant_ids)
    ids = [item["id"] for item in retrieved[:k]]
    hits = [index for index, item_id in enumerate(ids) if item_id in relevant]
    precision = len(hits) / k if k else 0.0
    precision_returned = len(hits) / len(ids) if ids else 0.0
    recall = len(hits) / len(relevant) if relevant else 0.0
    mrr = 1.0 / (hits[0] + 1) if hits else 0.0
    dcg = sum(1.0 / math.log2(index + 2) for index in hits)
    ideal_hits = min(k, len(relevant))
    idcg = sum(1.0 / math.log2(index + 2) for index in range(ideal_hits))
    return {
        "precision_at_k": precision,
        "precision_of_returned": precision_returned,
        "recall_at_k": recall,
        "mrr": mrr,
        "ndcg_at_k": dcg / idcg if idcg else 0.0,
    }


def evaluate_query(query: dict[str, Any], retrieved: list[dict[str, Any]], k: int) -> dict[str, Any]:
    row = metric_row(retrieved, query["relevant_ids"], k)
    row.update({"query_id": query["id"], "query": query["text"], "returned_count": len(retrieved[:k]), "top_ids": [item["id"] for item in retrieved[:k]], "top": retrieved[:k]})
    return row


def average_metrics(rows: list[dict[str, Any]]) -> dict[str, float]:
    fields = ["precision_at_k", "precision_of_returned", "recall_at_k", "mrr", "ndcg_at_k"]
    return {field: float(np.mean([row[field] for row in rows])) if rows else 0.0 for field in fields}


def run_query_comparison(
    dataset: dict[str, Any],
    embedder: OllamaEmbedder,
    memory_vectors: np.ndarray,
    query_vectors: dict[str, np.ndarray],
    config: dict[str, Any],
) -> dict[str, Any]:
    memories = dataset["memories"]
    queries = dataset["queries"]
    traditional = HybridRAG(memories, memory_vectors)
    field = V2Field(memories, memory_vectors, config)
    baseline_rows: list[dict[str, Any]] = []
    v2_rows: list[dict[str, Any]] = []
    state_rows: list[dict[str, Any]] = []
    for query in queries:
        query_with_vector = {**query, "vector": query_vectors[query["id"]], "as_of": "2026-08-09"}
        baseline_rows.append(evaluate_query(query, traditional.retrieve(query_with_vector, config["top_k"]), config["top_k"]))
        v2_result = field.perturb(query["text"], query_vectors[query["id"]], "read")
        v2_retrieved = [{**item, "score": item["score"]} for item in v2_result["readout"]]
        v2_rows.append(evaluate_query(query, v2_retrieved, config["top_k"]))
        state_rows.append({"query_id": query["id"], **v2_result})
    return {
        "traditional": {"rows": baseline_rows, "average": average_metrics(baseline_rows)},
        "v2_warm_sequential": {"rows": v2_rows, "average": average_metrics(v2_rows), "state_rows": state_rows, "final_state": field.state_summary()},
    }


def run_v2_only_with_vectors(
    dataset: dict[str, Any],
    memory_vectors: np.ndarray,
    query_vectors: dict[str, np.ndarray],
    config: dict[str, Any],
) -> dict[str, Any]:
    field = V2Field(dataset["memories"], memory_vectors, config)
    rows: list[dict[str, Any]] = []
    for query in dataset["queries"]:
        result = field.perturb(query["text"], query_vectors[query["id"]], "read")
        retrieved = result["readout"]
        rows.append(evaluate_query(query, retrieved, config["top_k"]))
    return {"average": average_metrics(rows), "final_state": field.state_summary(), "rows": rows}


def run_write_comparison(
    dataset: dict[str, Any],
    embedder: OllamaEmbedder,
    memory_vectors: np.ndarray,
    config: dict[str, Any],
    write_vectors: np.ndarray | None = None,
) -> dict[str, Any]:
    writes = dataset["write_tests"]
    initial_memories = dataset["memories"]
    baseline = HybridRAG(initial_memories, memory_vectors)
    field = V2Field(initial_memories, memory_vectors, config)
    if write_vectors is None:
        write_texts = [write["text"] for write in writes]
        write_vectors = embedder.embed(write_texts)
    rows: list[dict[str, Any]] = []
    for write, vector in zip(writes, write_vectors):
        traditional_memory = {
            "id": write["id"],
            "topic": "write-test",
            "date": write["date"],
            "seed_hits": 1,
            "text": write["text"],
        }
        baseline.write(traditional_memory, vector)
        baseline_read = baseline.retrieve({"text": write["text"], "vector": vector, "as_of": write["date"]}, config["top_k"])
        v2_write = field.perturb(write["text"], vector, "write")
        v2_read = field.perturb(write["text"], vector, "read")
        v2_ids = [item["id"] for item in v2_read["readout"]]
        rows.append({
            "write_id": write["id"],
            "text": write["text"],
            "expected_anchor_ids": write["expected_anchor_ids"],
            "novel": write.get("novel", False),
            "traditional_after_write": baseline_read,
            "traditional_exact_self_hit": baseline_read[0]["id"] == write["id"],
            "v2_write": v2_write,
            "v2_read_after_write": v2_read,
            "v2_anchor_read_ids": v2_ids,
            "v2_exact_event_text_returned": any(item.get("text") == write["text"] for item in v2_read["readout"]),
            "v2_event_log_contains_write": any(event["text"] == write["text"] for event in field.events),
        })
    def rate(predicate: Any, selected: list[dict[str, Any]]) -> float:
        return float(sum(1 for row in selected if predicate(row)) / len(selected)) if selected else 0.0

    novel_rows = [row for row in rows if row["novel"]]
    replay_rows = [row for row in rows if not row["novel"]]
    summary = {
        "total": len(rows),
        "novel_count": len(novel_rows),
        "replay_count": len(replay_rows),
        "traditional_exact_self_hit_rate": rate(lambda row: row["traditional_exact_self_hit"], rows),
        "v2_exact_event_text_returned_rate": rate(lambda row: row["v2_exact_event_text_returned"], rows),
        "v2_event_log_contains_write_rate": rate(lambda row: row["v2_event_log_contains_write"], rows),
        "novel_v2_event_log_contains_write_rate": rate(lambda row: row["v2_event_log_contains_write"], novel_rows),
        "replay_v2_anchor_read_expected_rate": rate(
            lambda row: bool(set(row["expected_anchor_ids"]) & set(row["v2_anchor_read_ids"])),
            replay_rows,
        ),
    }
    return {"rows": rows, "summary": summary, "final_state": field.state_summary()}


def run_sensitivity(dataset: dict[str, Any], memory_vectors: np.ndarray, query_vectors: dict[str, np.ndarray], base_config: dict[str, Any]) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for threshold in (0.03, 0.06, 0.12):
        config = base_config.copy()
        config["propagation_threshold"] = threshold
        comparison = run_v2_only_with_vectors(dataset, memory_vectors, query_vectors, config)
        rows.append({"parameter":"propagation_threshold", "value":threshold, "average":comparison["average"], "final_state":comparison["final_state"]})
    for budget in (32, 96, 192, 512, 1024, 2048, 4096):
        config = base_config.copy()
        config["sample_budget"] = budget
        comparison = run_v2_only_with_vectors(dataset, memory_vectors, query_vectors, config)
        rows.append({
            "parameter": "sample_budget",
            "value": budget,
            "sample_coverage": min(budget, len(dataset["memories"])) / len(dataset["memories"]) if dataset["memories"] else 0.0,
            "average": comparison["average"],
            "final_state": comparison["final_state"],
        })
    return rows


def make_llm_contexts(dataset: dict[str, Any], comparison: dict[str, Any]) -> dict[str, Any]:
    query_by_id = {query["id"]: query for query in dataset["queries"]}
    output: dict[str, Any] = {}
    for system_name in ("traditional", "v2_warm_sequential"):
        output[system_name] = []
        rows = comparison[system_name]["rows"]
        rows_by_id = {row["query_id"]: row for row in rows}
        for query_id in dataset["llm_eval_queries"]:
            if query_id not in rows_by_id or query_id not in query_by_id:
                continue
            row = rows_by_id[query_id]
            output[system_name].append({
                "query_id": query_id,
                "question": query_by_id[query_id]["text"],
                "retrieved_context": [{"id": item["id"], "text": item["text"]} for item in row["top"]],
            })
    return output


def call_llm_review(client: HttpJsonClient, contexts: list[dict[str, Any]], endpoint: str, api_key: str, model: str) -> dict[str, Any]:
    prompt = (
        "Answer the three questions below using only the supplied retrieved context. "
        "This is an integration smoke test, not a score or a system evaluation. "
        "Keep each answer to 1-3 sentences and cite memory ids in square brackets.\n\n"
        + json.dumps(contexts, ensure_ascii=False, indent=2)
    )
    payload = {
        "model": model,
        "temperature": 0,
        "max_tokens": 700,
        "messages": [
            {"role": "system", "content": "You are a precise memory-context consumer."},
            {"role": "user", "content": prompt},
        ],
    }
    response = client.post(endpoint, payload, {"Authorization": f"Bearer {api_key}"})
    choice = response.get("choices", [{}])[0]
    message = choice.get("message", {})
    content = message.get("content", "")
    if isinstance(content, list):
        content = "".join(part.get("text", "") for part in content if isinstance(part, dict))
    return {"content": content, "raw": response}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--dataset", type=Path, default=DATASET_PATH)
    parser.add_argument("--llm", action="store_true", help="Run two real chat-completion integration calls, one per context pipeline.")
    parser.add_argument("--output", type=Path, default=OUTPUT_PATH)
    parser.add_argument("--cache", type=Path, help="Embedding cache (.npz); defaults to a hidden file beside the dataset.")
    parser.add_argument("--embedding-batch-size", type=int, default=32)
    parser.add_argument("--limit-memories", type=int, help="Short probe only: keep the first N memories and compatible queries.")
    parser.add_argument("--limit-queries", type=int, help="Short probe only: keep the first N queries.")
    parser.add_argument("--limit-writes", type=int, help="Short probe only: keep the first N write cases.")
    parser.add_argument("--skip-sensitivity", action="store_true", help="Skip the six parameter sensitivity runs for a short probe.")
    parser.add_argument("--sample-budget", type=int, help="Override the observational v2 sample budget for a run.")
    parser.add_argument("--propagation-threshold", type=float, help="Override the observational v2 chain stop threshold for a run.")
    args = parser.parse_args()

    dataset = read_json(args.dataset)
    dataset = copy.deepcopy(dataset)
    if args.limit_memories:
        dataset["memories"] = dataset["memories"][:args.limit_memories]
        memory_ids = {memory["id"] for memory in dataset["memories"]}
        dataset["queries"] = [
            query for query in dataset["queries"]
            if all(relevant_id in memory_ids for relevant_id in query.get("relevant_ids", []))
        ]
    if args.limit_queries:
        dataset["queries"] = dataset["queries"][:args.limit_queries]
    if args.limit_writes:
        dataset["write_tests"] = dataset["write_tests"][:args.limit_writes]
    run_config = CONFIG.copy()
    if args.sample_budget is not None:
        run_config["sample_budget"] = args.sample_budget
    if args.propagation_threshold is not None:
        run_config["propagation_threshold"] = args.propagation_threshold
    client = HttpJsonClient()
    embedding_url = os.environ.get("OLLAMA_EMBED_URL", "http://127.0.0.1:11434/api/embed")
    embedding_model = os.environ.get("OLLAMA_EMBED_MODEL", dataset["embedding"]["model"])
    dimension = int(os.environ.get("OLLAMA_EMBED_DIM", dataset["embedding"]["dimension"]))
    embedder = OllamaEmbedder(client, embedding_url, embedding_model, dimension, args.embedding_batch_size)
    cache_path = args.cache or args.dataset.with_name(f".{args.dataset.stem}.embeddings.npz")
    cache = EmbeddingCache(cache_path, embedding_model, dimension)
    started = time.monotonic()
    memory_texts = [memory["text"] for memory in dataset["memories"]]
    print(f"[v2] embedding memories: {len(memory_texts)} texts (batch={args.embedding_batch_size})", flush=True)
    memory_vectors, memory_missing = cache.embed(memory_texts, embedder)
    query_texts = [query["text"] for query in dataset["queries"]]
    print(f"[v2] embedding queries: {len(query_texts)} texts", flush=True)
    query_vectors_matrix, query_missing = cache.embed(query_texts, embedder)
    query_vectors = {query["id"]: vector for query, vector in zip(dataset["queries"], query_vectors_matrix)}
    write_texts = [write["text"] for write in dataset["write_tests"]]
    print(f"[v2] embedding writes: {len(write_texts)} texts", flush=True)
    write_vectors, write_missing = cache.embed(write_texts, embedder)
    embedding_elapsed_ms = round((time.monotonic() - started) * 1000, 2)

    comparison_started = time.monotonic()
    print(f"[v2] comparing retrieval: {len(dataset['queries'])} queries against {len(dataset['memories'])} memories", flush=True)
    comparison = run_query_comparison(dataset, embedder, memory_vectors, query_vectors, run_config)
    comparison_elapsed_ms = round((time.monotonic() - comparison_started) * 1000, 2)
    write_started = time.monotonic()
    writes = run_write_comparison(dataset, embedder, memory_vectors, run_config, write_vectors)
    write_elapsed_ms = round((time.monotonic() - write_started) * 1000, 2)
    sensitivity_started = time.monotonic()
    sensitivity = [] if args.skip_sensitivity else run_sensitivity(dataset, memory_vectors, query_vectors, run_config)
    sensitivity_elapsed_ms = round((time.monotonic() - sensitivity_started) * 1000, 2)
    result: dict[str, Any] = {
        "metadata": {
            "dataset_id": dataset["dataset_id"],
            "generated_at": dt.datetime.now(dt.timezone.utc).isoformat(),
            "embedding_url": embedding_url,
            "embedding_model": embedding_model,
            "embedding_dimension": dimension,
            "llm_used_for_scoring": False,
            "config": run_config,
            "dataset_path": str(args.dataset),
            "memory_count": len(dataset["memories"]),
            "query_count": len(dataset["queries"]),
            "write_count": len(dataset["write_tests"]),
            "embedding_cache": {
                "path": str(cache_path),
                "cached_vector_count_after_run": len(cache.vectors),
                "memory_cache_misses": memory_missing,
                "query_cache_misses": query_missing,
                "write_cache_misses": write_missing,
            },
            "timings_ms": {
                "embedding": embedding_elapsed_ms,
                "query_comparison": comparison_elapsed_ms,
                "write_comparison": write_elapsed_ms,
                "sensitivity": sensitivity_elapsed_ms,
            },
            "write_read_semantics": "traditional write adds a retrievable chunk; v2 write and read both call perturb, but the observational skeleton does not create a new anchor for a novel event",
        },
        "comparison": comparison,
        "write_comparison": writes,
        "sensitivity": sensitivity,
        "llm_contexts": make_llm_contexts(dataset, comparison),
        "http_calls": client.calls,
    }

    if args.llm:
        endpoint = os.environ.get("LLM_ENDPOINT", "http://127.0.0.1:4000/v1/chat/completions")
        api_key = os.environ.get("LLM_API_KEY")
        model = os.environ.get("LLM_MODEL", "test-model")
        if not api_key:
            raise RuntimeError("--llm requires LLM_API_KEY in the environment; the key is intentionally not stored in source files")
        result["llm_integration"] = {
            "endpoint": endpoint,
            "model": model,
            "traditional": call_llm_review(client, result["llm_contexts"]["traditional"], endpoint, api_key, model),
            "v2_warm_sequential": call_llm_review(client, result["llm_contexts"]["v2_warm_sequential"], endpoint, api_key, model),
        }

    write_json(args.output, result)
    print(json.dumps({
        "output": str(args.output),
        "dataset": str(args.dataset),
        "memory_count": len(dataset["memories"]),
        "query_count": len(dataset["queries"]),
        "write_count": len(dataset["write_tests"]),
        "traditional_average": comparison["traditional"]["average"],
        "v2_average": comparison["v2_warm_sequential"]["average"],
        "http_call_count": len(client.calls),
        "llm_enabled": args.llm,
    }, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
