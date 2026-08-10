#!/usr/bin/env python3
"""Prepare the scaled v2 feasibility corpus.

The small ``dataset.json`` fixture remains a regression/control set.  This
script builds a larger, auditable dataset from two sources:

* LoCoMo's public ten-conversation release (turn-level memories and its
  evidence-linked QA annotations); and
* the current repository's Markdown/Rust/TypeScript source as a project-context
  pressure set.

No model is used here.  All records, queries, and write cases are derived by
 deterministic transformations so the ground truth can be inspected.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import re
import urllib.request
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parent
REPO_ROOT = ROOT.parents[1]
DEFAULT_SOURCE_URL = (
    "https://raw.githubusercontent.com/snap-research/locomo/"
    "3eb6f2c585f5e1699204e3c3bdf7adc5c28cb376/data/locomo10.json"
)
DEFAULT_OUTPUT = ROOT / "dataset_scale.json"
EXPECTED_SOURCE_SHA256 = "79fa87e90f04081343b8c8debecb80a9a6842b76a7aa537dc9fdf651ea698ff4"
TODAY = "2026-08-09"


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def parse_session_date(value: str) -> str:
    match = re.search(r"on\s+(\d{1,2}\s+[A-Za-z]+,\s+\d{4})", value)
    if not match:
        match = re.search(r"(\d{1,2}\s+[A-Za-z]+,\s+\d{4})", value)
    if not match:
        raise ValueError(f"cannot parse LoCoMo session date: {value!r}")
    return dt.datetime.strptime(match.group(1), "%d %B, %Y").date().isoformat()


def load_source(source: Path | None, url: str) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    if source is None:
        with urllib.request.urlopen(url, timeout=120) as response:
            raw = response.read()
        source_name = url
    else:
        raw = source.read_bytes()
        source_name = str(source)
    digest = hashlib.sha256(raw).hexdigest()
    if url == DEFAULT_SOURCE_URL and digest != EXPECTED_SOURCE_SHA256:
        raise ValueError(f"LoCoMo source checksum mismatch: expected {EXPECTED_SOURCE_SHA256}, got {digest}")
    data = json.loads(raw)
    if not isinstance(data, list) or not data:
        raise ValueError("LoCoMo source must be a non-empty JSON list")
    return data, {"url": source_name, "sha256": digest, "license": "CC BY-NC 4.0"}


def memory_id(sample_id: str, dia_id: str) -> str:
    return f"locomo_{sample_id}_{dia_id.replace(':', '_')}"


def build_locomo(source: list[dict[str, Any]]) -> tuple[list[dict[str, Any]], list[dict[str, Any]], list[str], dict[str, int]]:
    memories: list[dict[str, Any]] = []
    memory_by_evidence: dict[tuple[str, str], str] = {}
    for sample in source:
        sample_id = sample["sample_id"]
        conversation = sample["conversation"]
        for session_number in range(1, 100):
            session_key = f"session_{session_number}"
            if session_key not in conversation:
                break
            session_date = parse_session_date(conversation[f"{session_key}_date_time"])
            for turn in conversation[session_key]:
                dia_id = turn["dia_id"]
                record_id = memory_id(sample_id, dia_id)
                text = f"{turn['speaker']}: {turn['text']}"
                if turn.get("blip_caption"):
                    text += f" [Image caption: {turn['blip_caption']}]"
                if turn.get("query"):
                    text += f" [Image query: {turn['query']}]"
                memories.append({
                    "id": record_id,
                    "topic": f"locomo/{sample_id}/session_{session_number}",
                    "date": session_date,
                    # A neutral starting density prevents QA annotations from
                    # leaking into field mechanics.
                    "seed_hits": 1,
                    "text": text,
                    "source": "locomo",
                    "source_sample": sample_id,
                    "source_evidence": dia_id,
                })
                memory_by_evidence[(sample_id, dia_id)] = record_id

    queries: list[dict[str, Any]] = []
    unresolved_evidence = 0
    unresolved_questions = 0
    category_counts: dict[str, int] = {}
    for sample in source:
        sample_id = sample["sample_id"]
        for index, question in enumerate(sample["qa"], start=1):
            relevant_ids = [
                memory_by_evidence[(sample_id, evidence)]
                for evidence in question.get("evidence", [])
                if (sample_id, evidence) in memory_by_evidence
            ]
            unresolved_evidence += len(question.get("evidence", [])) - len(relevant_ids)
            if not relevant_ids:
                unresolved_questions += 1
                continue
            category = str(question.get("category", "unknown"))
            category_counts[category] = category_counts.get(category, 0) + 1
            queries.append({
                "id": f"locomo_q_{sample_id}_{index:04d}",
                "text": question["question"],
                "relevant_ids": relevant_ids,
                "answer": question.get("answer"),
                "answer_keywords": re.findall(r"[A-Za-z0-9]+", str(question.get("answer", "")).lower()),
                "category": category,
                "source": "locomo",
                "source_sample": sample_id,
                "evidence": question.get("evidence", []),
            })
    diagnostics = {
        "conversation_count": len(source),
        "turn_memory_count": len(memories),
        "qa_count": sum(len(item["qa"]) for item in source),
        "usable_query_count": len(queries),
        "unresolved_evidence_count": unresolved_evidence,
        "unresolved_question_count": unresolved_questions,
        "query_category_counts": category_counts,
    }
    return memories, queries, list(memory_by_evidence.values()), diagnostics


def iter_project_files() -> list[Path]:
    allowed_suffixes = {".md", ".rs", ".tsx", ".ts", ".css", ".toml"}
    excluded_parts = {".git", "target", "node_modules", "experiments", ".next"}
    paths: list[Path] = []
    for path in REPO_ROOT.rglob("*"):
        if not path.is_file() or path.suffix not in allowed_suffixes:
            continue
        if any(part in excluded_parts for part in path.relative_to(REPO_ROOT).parts):
            continue
        paths.append(path)
    return sorted(paths)


def chunk_text(text: str, size: int = 850, overlap: int = 120) -> list[str]:
    text = text.replace("\r\n", "\n").strip()
    if not text:
        return []
    chunks: list[str] = []
    start = 0
    while start < len(text):
        end = min(len(text), start + size)
        if end < len(text):
            boundary = max(text.rfind("\n\n", start, end), text.rfind("\n", start, end))
            if boundary > start + size // 2:
                end = boundary
        chunk = text[start:end].strip()
        if chunk:
            chunks.append(chunk)
        if end == len(text):
            break
        start = max(start + 1, end - overlap)
    return chunks


def project_query_text(chunk: str, relative_path: str, index: int) -> str:
    headings = [line.lstrip("# ").strip() for line in chunk.splitlines() if line.startswith("#")]
    heading = headings[0] if headings else ""
    sentence = re.split(r"(?<=[.!?])\s+", re.sub(r"[`*_]", "", chunk), maxsplit=1)[0]
    if heading and sentence:
        return f"In {relative_path}, what does the {heading} section say about {sentence[:180]}?"
    return f"What does {relative_path} document near chunk {index} about {sentence[:220]}?"


def build_project_context() -> tuple[list[dict[str, Any]], list[dict[str, Any]], dict[str, int]]:
    memories: list[dict[str, Any]] = []
    queries: list[dict[str, Any]] = []
    for path in iter_project_files():
        relative_path = path.relative_to(REPO_ROOT).as_posix()
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        chunks = chunk_text(text)
        for index, chunk in enumerate(chunks):
            record_id = f"repo_{hashlib.sha1(relative_path.encode()).hexdigest()[:10]}_{index:03d}"
            memory = {
                "id": record_id,
                "topic": f"repo/{relative_path}",
                "date": TODAY,
                "seed_hits": 1,
                "text": f"[{relative_path}#{index}]\n{chunk}",
                "source": "project",
                "source_path": relative_path,
                "source_chunk": index,
            }
            memories.append(memory)
            if len(queries) < 120 or index == 0:
                queries.append({
                    "id": f"repo_q_{len(queries):04d}",
                    "text": project_query_text(chunk, relative_path, index),
                    "relevant_ids": [record_id],
                    "answer_keywords": re.findall(r"[A-Za-z0-9_:-]+", chunk.lower())[:12],
                    "category": "project-context",
                    "source": "project",
                    "source_path": relative_path,
                })
    return memories, queries, {"project_file_count": len(iter_project_files()), "project_chunk_count": len(memories), "project_query_count": len(queries)}


def build_writes(memories: list[dict[str, Any]]) -> list[dict[str, Any]]:
    locomo = [memory for memory in memories if memory.get("source") == "locomo"]
    if not locomo:
        raise ValueError("scaled dataset has no LoCoMo memories")
    writes: list[dict[str, Any]] = []
    replay_count = min(32, len(locomo))
    step = max(1, len(locomo) // replay_count)
    for index, memory in enumerate(locomo[::step][:replay_count]):
        writes.append({
            "id": f"w_replay_{index:03d}",
            "date": TODAY,
            "text": f"Follow-up memory: {memory['text']}",
            "expected_anchor_ids": [memory["id"]],
            "novel": False,
            "source": "locomo-replay",
        })
    for index in range(32):
        writes.append({
            "id": f"w_novel_{index:03d}",
            "date": TODAY,
            "text": (
                f"Experiment write marker {index}: a new v2 event must be observable in the event log, "
                "while anchor creation remains a separate design decision."
            ),
            "expected_anchor_ids": [],
            "novel": True,
            "source": "synthetic-write",
        })
    return writes


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--locomo-source", type=Path, help="Use a local locomo10.json instead of downloading the pinned source.")
    parser.add_argument("--locomo-url", default=DEFAULT_SOURCE_URL)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    args = parser.parse_args()

    locomo_source, provenance = load_source(args.locomo_source, args.locomo_url)
    locomo_memories, locomo_queries, _, locomo_diag = build_locomo(locomo_source)
    project_memories, project_queries, project_diag = build_project_context()
    memories = locomo_memories + project_memories
    queries = locomo_queries + project_queries
    writes = build_writes(memories)
    dataset = {
        "dataset_id": "field-memory-v2-feasibility-scaled-2026-08-09",
        "description": (
            "Scaled observational comparison: 10 public LoCoMo conversations with evidence-linked QA, "
            "plus a deterministic repository document/code pressure set."
        ),
        "language": "English",
        "embedding": {
            "provider": "ollama",
            "url": "http://127.0.0.1:11434/api/embed",
            "model": "bge-m3",
            "dimension": 1024,
        },
        "provenance": {
            "locomo": provenance,
            "locomo_license": "CC BY-NC 4.0; see https://github.com/snap-research/locomo/blob/main/LICENSE.txt",
            "project_root": str(REPO_ROOT),
            "project_file_selection": "Markdown, Rust, TypeScript/TSX, CSS, and TOML; excludes generated/dependency/experiment paths",
            "generated_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        },
        "diagnostics": {**locomo_diag, **project_diag, "total_memory_count": len(memories), "total_query_count": len(queries), "write_count": len(writes)},
        "memories": memories,
        "queries": queries,
        "write_tests": writes,
        "llm_eval_queries": [query["id"] for query in locomo_queries[:3]],
    }
    write_json(args.output, dataset)
    print(json.dumps(dataset["diagnostics"], ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
