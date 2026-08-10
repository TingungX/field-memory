#!/usr/bin/env python3
"""Validate the saved v2 feasibility dataset and deterministic result artifact.

This intentionally checks artifact integrity and metric accounting only.  It
does not re-embed, re-run the field, or issue an LLM request, so it is safe to
use as a quick audit after a completed scale run.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import math
from pathlib import Path
from typing import Any, Iterable


ROOT = Path(__file__).resolve().parent
DATASET_PATH = ROOT / "dataset_scale.json"
RESULTS_PATH = ROOT / "results_scale.json"

METRICS = (
    "precision_at_k",
    "precision_of_returned",
    "recall_at_k",
    "mrr",
    "ndcg_at_k",
)


class AuditError(ValueError):
    """Raised for an artifact integrity or accounting mismatch."""


def read_json(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def unique_ids(rows: Iterable[dict[str, Any]], label: str) -> set[str]:
    values = [row.get("id") for row in rows]
    if any(not isinstance(value, str) or not value for value in values):
        raise AuditError(f"{label} has an empty or non-string id")
    unique = set(values)
    if len(unique) != len(values):
        raise AuditError(f"{label} has duplicate ids")
    return unique


def require_close(actual: float, expected: float, label: str) -> None:
    if not math.isclose(actual, expected, rel_tol=0.0, abs_tol=1e-12):
        raise AuditError(f"{label}: expected {expected}, got {actual}")


def validate_dataset(dataset: dict[str, Any]) -> dict[str, Any]:
    memories = dataset.get("memories")
    queries = dataset.get("queries")
    writes = dataset.get("write_tests")
    diagnostics = dataset.get("diagnostics", {})
    if not all(isinstance(items, list) for items in (memories, queries, writes)):
        raise AuditError("dataset memories, queries, and write_tests must be lists")

    memory_ids = unique_ids(memories, "memories")
    query_ids = unique_ids(queries, "queries")
    write_ids = unique_ids(writes, "write_tests")
    for memory in memories:
        try:
            dt.date.fromisoformat(memory["date"])
        except (KeyError, TypeError, ValueError) as error:
            raise AuditError(f"invalid memory date for {memory.get('id')!r}") from error
    for query in queries:
        relevant_ids = query.get("relevant_ids")
        if not isinstance(relevant_ids, list) or not relevant_ids:
            raise AuditError(f"query {query.get('id')!r} has no relevant_ids")
        missing = set(relevant_ids) - memory_ids
        if missing:
            raise AuditError(f"query {query.get('id')!r} references missing memory ids: {sorted(missing)[:3]}")
    for write in writes:
        try:
            dt.date.fromisoformat(write["date"])
        except (KeyError, TypeError, ValueError) as error:
            raise AuditError(f"invalid write date for {write.get('id')!r}") from error
        missing = set(write.get("expected_anchor_ids", [])) - memory_ids
        if missing:
            raise AuditError(f"write {write.get('id')!r} references missing anchors: {sorted(missing)[:3]}")

    locomo_memories = [row for row in memories if row.get("source") == "locomo"]
    project_memories = [row for row in memories if row.get("source") == "project"]
    locomo_queries = [row for row in queries if row.get("source") == "locomo"]
    project_queries = [row for row in queries if row.get("source") == "project"]
    category_counts: dict[str, int] = {}
    for query in locomo_queries:
        category = str(query.get("category"))
        category_counts[category] = category_counts.get(category, 0) + 1

    expected_counts = {
        "turn_memory_count": len(locomo_memories),
        "project_chunk_count": len(project_memories),
        "usable_query_count": len(locomo_queries),
        "project_query_count": len(project_queries),
        "total_memory_count": len(memories),
        "total_query_count": len(queries),
        "write_count": len(writes),
    }
    for key, expected in expected_counts.items():
        if diagnostics.get(key) != expected:
            raise AuditError(f"diagnostics.{key}: expected {expected}, got {diagnostics.get(key)!r}")
    if {str(key): value for key, value in diagnostics.get("query_category_counts", {}).items()} != category_counts:
        raise AuditError("diagnostics.query_category_counts does not match locomo queries")

    return {
        "memory_count": len(memories),
        "query_count": len(queries),
        "write_count": len(writes),
        "locomo_query_count": len(locomo_queries),
        "project_query_count": len(project_queries),
        "memory_ids": memory_ids,
        "query_ids": query_ids,
        "write_ids": write_ids,
    }


def validate_pipeline(rows: Any, expected_query_ids: set[str], memory_ids: set[str], name: str) -> dict[str, float]:
    if not isinstance(rows, list):
        raise AuditError(f"{name}.rows must be a list")
    row_ids = [row.get("query_id") for row in rows]
    if len(row_ids) != len(set(row_ids)):
        raise AuditError(f"{name}.rows has duplicate query ids")
    if set(row_ids) != expected_query_ids:
        missing = expected_query_ids - set(row_ids)
        extra = set(row_ids) - expected_query_ids
        raise AuditError(f"{name}.rows query coverage mismatch: missing={len(missing)}, extra={len(extra)}")
    for row in rows:
        top_ids = row.get("top_ids", [])
        if row.get("returned_count") != len(top_ids):
            raise AuditError(f"{name} returned_count mismatch for {row.get('query_id')!r}")
        unknown = set(top_ids) - memory_ids
        if unknown:
            raise AuditError(f"{name} returned unknown memory ids for {row.get('query_id')!r}")
        for metric in METRICS:
            value = row.get(metric)
            if not isinstance(value, (int, float)) or not 0.0 <= value <= 1.0:
                raise AuditError(f"{name}.{metric} out of range for {row.get('query_id')!r}")
    return {metric: sum(float(row[metric]) for row in rows) / len(rows) for metric in METRICS}


def validate_results(results: dict[str, Any], dataset_check: dict[str, Any]) -> dict[str, Any]:
    metadata = results.get("metadata", {})
    if metadata.get("llm_used_for_scoring") is not False:
        raise AuditError("results must state that LLM was not used for scoring")
    for key, dataset_key in (("memory_count", "memory_count"), ("query_count", "query_count"), ("write_count", "write_count")):
        if metadata.get(key) != dataset_check[dataset_key]:
            raise AuditError(f"metadata.{key} does not match dataset")

    comparison = results.get("comparison", {})
    recomputed: dict[str, dict[str, float]] = {}
    for name in ("traditional", "v2_warm_sequential"):
        pipeline = comparison.get(name, {})
        average = pipeline.get("average", {})
        recomputed[name] = validate_pipeline(
            pipeline.get("rows"),
            dataset_check["query_ids"],
            dataset_check["memory_ids"],
            name,
        )
        for metric, expected in recomputed[name].items():
            require_close(float(average.get(metric)), expected, f"{name}.average.{metric}")

    write_comparison = results.get("write_comparison", {})
    write_rows = write_comparison.get("rows")
    if not isinstance(write_rows, list):
        raise AuditError("write_comparison.rows must be a list")
    write_row_ids = [row.get("write_id") for row in write_rows]
    if len(write_row_ids) != len(set(write_row_ids)) or set(write_row_ids) != dataset_check["write_ids"]:
        raise AuditError("write_comparison.rows does not cover write_tests exactly")
    summary = write_comparison.get("summary", {})
    if summary.get("total") != len(write_rows):
        raise AuditError("write summary total does not match rows")
    for key, expression in {
        "traditional_exact_self_hit_rate": lambda row: bool(row.get("traditional_exact_self_hit")),
        "v2_exact_event_text_returned_rate": lambda row: bool(row.get("v2_exact_event_text_returned")),
        "v2_event_log_contains_write_rate": lambda row: bool(row.get("v2_event_log_contains_write")),
    }.items():
        expected = sum(expression(row) for row in write_rows) / len(write_rows)
        require_close(float(summary.get(key)), expected, f"write_comparison.summary.{key}")

    sensitivity = results.get("sensitivity")
    if not isinstance(sensitivity, list) or len(sensitivity) < 2:
        raise AuditError("sensitivity must contain threshold and capacity observations")
    budget_rows = [row for row in sensitivity if row.get("parameter") == "sample_budget"]
    if len(budget_rows) < 2:
        raise AuditError("sensitivity is missing sample-budget observations")
    previous_budget = 0
    previous_coverage = 0.0
    for row in budget_rows:
        budget = row.get("value")
        coverage = row.get("sample_coverage")
        if not isinstance(budget, int) or not isinstance(coverage, (int, float)):
            raise AuditError("sample-budget sensitivity row is malformed")
        if budget <= previous_budget or coverage <= previous_coverage:
            raise AuditError("sample-budget sensitivity is not strictly increasing")
        require_close(float(coverage), budget / dataset_check["memory_count"], f"sample coverage for budget {budget}")
        previous_budget, previous_coverage = budget, float(coverage)

    contexts = results.get("llm_contexts", {})
    for name in ("traditional", "v2_warm_sequential"):
        context_ids = [row.get("query_id") for row in contexts.get(name, [])]
        if not context_ids or not set(context_ids) <= dataset_check["query_ids"]:
            raise AuditError(f"llm_contexts.{name} is missing or contains unknown query ids")

    return {
        "traditional": recomputed["traditional"],
        "v2_warm_sequential": recomputed["v2_warm_sequential"],
        "sample_budget_points": len(budget_rows),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description="Audit the v2 scale dataset and saved deterministic result JSON.")
    parser.add_argument("--dataset", type=Path, default=DATASET_PATH)
    parser.add_argument("--results", type=Path, default=RESULTS_PATH)
    args = parser.parse_args()

    dataset_check = validate_dataset(read_json(args.dataset))
    result_check = validate_results(read_json(args.results), dataset_check)
    print(json.dumps({
        "status": "passed",
        "dataset": str(args.dataset),
        "results": str(args.results),
        "memory_count": dataset_check["memory_count"],
        "query_count": dataset_check["query_count"],
        "write_count": dataset_check["write_count"],
        "locomo_query_count": dataset_check["locomo_query_count"],
        "project_query_count": dataset_check["project_query_count"],
        "sample_budget_points": result_check["sample_budget_points"],
        "traditional_recall_at_k": result_check["traditional"]["recall_at_k"],
        "v2_recall_at_k": result_check["v2_warm_sequential"]["recall_at_k"],
    }, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
