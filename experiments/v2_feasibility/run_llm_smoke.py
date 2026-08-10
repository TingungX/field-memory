#!/usr/bin/env python3
"""Run exactly one real-model context smoke test for a saved v2 result.

This is deliberately separate from ``run_experiment.py``: retrieval evaluation
must remain deterministic and local, while this script makes one bounded remote
call solely to verify that a retrieved context can be consumed by the configured
model endpoint.  Its output is not a quality score.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parent
RESULTS_PATH = ROOT / "results_scale.json"


def read_json(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def write_json(path: Path, value: dict[str, Any]) -> None:
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def make_prompt(contexts: list[dict[str, Any]]) -> str:
    return (
        "Answer the supplied questions using only the retrieved context. "
        "This is an integration smoke test, not a score or a system evaluation. "
        "For each question, give at most one sentence and cite the supporting "
        "memory id in square brackets. If context is insufficient, say so.\n\n"
        + json.dumps(contexts, ensure_ascii=False, indent=2)
    )


def extract_output_text(response: dict[str, Any]) -> str:
    text_parts: list[str] = []
    for item in response.get("output", []):
        if not isinstance(item, dict) or item.get("type") != "message":
            continue
        for part in item.get("content", []):
            if isinstance(part, dict) and part.get("type") in {"output_text", "text"}:
                value = part.get("text", "")
                if isinstance(value, str):
                    text_parts.append(value)
    return "".join(text_parts)


def parse_sse(raw: str) -> list[dict[str, Any]]:
    """Decode a complete, non-interactive Responses SSE body for audit output."""

    events: list[dict[str, Any]] = []
    for block in raw.replace("\r\n", "\n").split("\n\n"):
        event_type: str | None = None
        data_lines: list[str] = []
        for line in block.splitlines():
            if line.startswith("event:"):
                event_type = line.partition(":")[2].strip()
            elif line.startswith("data:"):
                data_lines.append(line.partition(":")[2].lstrip())
        if not data_lines:
            continue
        data = "\n".join(data_lines)
        if data == "[DONE]":
            events.append({"event": event_type or "done", "data": "[DONE]"})
            continue
        try:
            parsed: Any = json.loads(data)
        except json.JSONDecodeError:
            parsed = {"raw": data[:2_000]}
        events.append({"event": event_type or "message", "data": parsed})
    return events


def extract_stream_output(events: list[dict[str, Any]]) -> tuple[str, dict[str, Any] | None]:
    text_parts: list[str] = []
    error: dict[str, Any] | None = None
    for item in events:
        data = item.get("data")
        if not isinstance(data, dict):
            continue
        if "error" in data:
            candidate = data["error"]
            error = candidate if isinstance(candidate, dict) else {"message": str(candidate)}
        if item.get("event") == "response.output_text.delta" and isinstance(data.get("delta"), str):
            text_parts.append(data["delta"])
        response = data.get("response")
        if isinstance(response, dict):
            completed_text = extract_output_text(response)
            if completed_text:
                return completed_text, error
    return "".join(text_parts), error


def post_once(
    url: str,
    payload: dict[str, Any],
    api_key: str,
    timeout: int,
    *,
    stream: bool,
) -> tuple[int, dict[str, Any], float, str | None]:
    body = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        url,
        data=body,
        method="POST",
        headers={
            "Authorization": f"Bearer {api_key}",
            "Content-Type": "application/json",
        },
    )
    started = time.monotonic()
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            raw = response.read().decode("utf-8", errors="replace")
            parsed = {"sse_events": parse_sse(raw)} if stream else json.loads(raw)
            return response.status, parsed, round((time.monotonic() - started) * 1000, 2), response.headers.get("x-request-id")
    except urllib.error.HTTPError as error:
        raw = error.read().decode("utf-8", errors="replace")
        try:
            parsed: dict[str, Any] = json.loads(raw)
        except json.JSONDecodeError:
            parsed = {"raw_error": raw[:2_000]}
        return error.code, parsed, round((time.monotonic() - started) * 1000, 2), error.headers.get("x-request-id")
    except urllib.error.URLError as error:
        return 0, {"error": {"type": "connection_error", "message": str(error.reason)}}, round((time.monotonic() - started) * 1000, 2), None


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Make exactly one bounded Responses API smoke-test call for one saved retrieval pipeline."
    )
    parser.add_argument("--results", type=Path, default=RESULTS_PATH)
    parser.add_argument("--pipeline", choices=("traditional", "v2_warm_sequential"), required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--max-questions",
        type=int,
        help="Use only the first N saved query contexts. This bounds a real-model smoke test; it never changes retrieval scoring.",
    )
    parser.add_argument(
        "--reasoning-effort",
        choices=("none", "auto", "minimal", "low", "medium", "high", "xhigh", "max"),
        help="Optional Responses reasoning.effort value sent to the configured model endpoint.",
    )
    parser.add_argument("--max-output-tokens", type=int, default=250)
    parser.add_argument("--timeout", type=int, default=180)
    parser.add_argument("--stream", action="store_true", help="Use the Responses SSE path for this one request.")
    args = parser.parse_args()

    if args.max_output_tokens < 1:
        raise ValueError("--max-output-tokens must be positive")
    if args.max_questions is not None and args.max_questions < 1:
        raise ValueError("--max-questions must be positive")
    if args.timeout < 1:
        raise ValueError("--timeout must be positive")

    api_key = os.environ.get("LLM_API_KEY")
    if not api_key:
        raise RuntimeError("LLM_API_KEY must be provided through the process environment")
    endpoint = os.environ.get("LLM_ENDPOINT", "http://127.0.0.1:4000/v1/responses")
    model = os.environ.get("LLM_MODEL", "test-model")

    results = read_json(args.results)
    saved_contexts = results.get("llm_contexts", {}).get(args.pipeline)
    if not isinstance(saved_contexts, list) or not saved_contexts:
        raise ValueError(f"no saved LLM contexts for pipeline {args.pipeline!r} in {args.results}")
    contexts = saved_contexts[:args.max_questions] if args.max_questions is not None else saved_contexts

    prompt = make_prompt(contexts)
    payload = {
        "model": model,
        "instructions": "You are a precise memory-context consumer.",
        "input": prompt,
        "max_output_tokens": args.max_output_tokens,
        "temperature": 0,
    }
    if args.reasoning_effort is not None:
        payload["reasoning"] = {"effort": args.reasoning_effort}
    if args.stream:
        payload["stream"] = True
    status, response, elapsed_ms, request_id = post_once(
        endpoint,
        payload,
        api_key,
        args.timeout,
        stream=args.stream,
    )
    output_text = ""
    stream_error: dict[str, Any] | None = None
    if args.stream:
        output_text, stream_error = extract_stream_output(response.get("sse_events", []))
    else:
        output_text = extract_output_text(response)
    succeeded = 200 <= status < 300 and stream_error is None
    outcome: dict[str, Any] = {
        "purpose": "one real-model context-consumption smoke test; not used as a score",
        "timestamp_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
        "protocol": "openai/responses",
        "stream": args.stream,
        "endpoint": endpoint,
        "model": model,
        "pipeline": args.pipeline,
        "request_count": 1,
        "source_context_query_count": len(saved_contexts),
        "context_query_count": len(contexts),
        "reasoning_effort": args.reasoning_effort,
        "max_output_tokens": args.max_output_tokens,
        "timeout_seconds": args.timeout,
        "context_query_ids": [item.get("query_id") for item in contexts],
        "context_memory_ids": [
            item.get("id")
            for context in contexts
            for item in context.get("retrieved_context", [])
        ],
        "prompt_sha256": hashlib.sha256(prompt.encode("utf-8")).hexdigest(),
        "http_status": status,
        "request_id": request_id,
        "elapsed_ms": elapsed_ms,
        "status": "succeeded" if succeeded else "failed",
        "response": response,
    }
    if stream_error is not None:
        outcome["stream_error"] = stream_error
    if succeeded:
        outcome["output_text"] = output_text

    write_json(args.output, outcome)
    print(json.dumps({
        "pipeline": args.pipeline,
        "status": outcome["status"],
        "http_status": status,
        "elapsed_ms": elapsed_ms,
        "output": str(args.output),
    }, ensure_ascii=False))


if __name__ == "__main__":
    main()
