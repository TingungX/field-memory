# DSE-Memory

**Dynamic Semantic Evolutionary Memory** — a unified potential-field memory engine in Rust.

> 概念的意义由其关联事件的空间密度分布动态赋予。
> 记忆不是数据库，是松弛系统。状态不是"写入"，是连续平衡被扰动后的收敛结果。
> 没有判断，没有分类，只有扰动与收敛。

## What is this?

DSE-Memory is a memory engine where every concept (`Anchor`) and every event (`Event`) lives as a vector in a shared potential field. Recall is a **relaxation process**: a query broadcasts its vector to all anchors, anchors whose direction has high cosine similarity to the query and high accumulated density exert pull, and the field relaxes toward the query's direction. Memory is the equilibrium of this system, not a lookup table.

The core equation is a single line:

```
impact(anchor, event) = cos_sim(anchor.direction, event.direction) × √anchor.density
```

That's it. Everything else — paradigm shifts, ECG-style field tension diagnostics, the recall engine, persistence — derives from this formula.

## Architecture

Single Cargo workspace, two crates:

```
MEMORY/
├── crates/
│   ├── dse-core/                  # the engine (library)
│   │   └── src/
│   │       ├── types.rs           # Event, AnchorKey, ImpactTrace, SeedConcept, Anisotropy
│   │       ├── math.rs            # cosine_sim, normalize, l2, dot, add, scale, moving_avg
│   │       ├── embed.rs           # EmbedProvider trait + DummyEmbedProvider
│   │       ├── physics.rs         # impact(), effective_direction(), relax_step()
│   │       ├── cycle.rs           # RelaxationCycle (the only field evolution mechanism)
│   │       ├── recall.rs          # value_init / associate / recall / consolidate_from_recall
│   │       ├── init.rs            # concept descriptions → anchors
│   │       ├── paradigm.rs        # detect / orthogonalize / revive (seed → anchor)
│   │       ├── ecg.rs             # CognitiveEcg (tension, convergence, anisotropy)
│   │       ├── engine.rs          # DseEngine orchestrator (public API)
│   │       └── persist.rs         # sled save / load
│   └── dse-cli/                   # end-to-end demo (binary)
│       └── src/main.rs            # init → input → relax → recall → ecg → persist
└── docs/
    └── design.md                  # v2 architecture (clean-slate potential-field design)
```

Configuration is exactly **5 parameters**:

```rust
pub struct DseCoreParams {
    pub vector_dim: usize,            // embedding dimension (default: 32)
    pub event_window_secs: u64,       // sliding window for cycle (default: 3600)
    pub damping_base: f32,            // base damping (default: 0.5)
    pub stiffness_base: f32,          // base stiffness (default: 1.0)
    pub convergence_threshold: f32,   // relaxation stop threshold (default: 0.001)
}
```

## Quick Start

```bash
# Run the CLI demo end-to-end
cargo run -p dse-cli

# Run all unit + integration tests
cargo test

# Type-check the whole workspace
cargo check
```

The CLI demo will:

1. Initialize 7 anchors from Chinese concept descriptions (your coding-style preferences)
2. Simulate 3 user inputs ("add webfetch", "rewrite in Rust", "be careful with apply_patch")
3. Print value-init (top anchors by density), associative recall, active recall, ECG report
4. Persist state to `/tmp/dse-demo-data` (sled embedded KV)
5. Reload state into a fresh engine for verification

## Status

**Phase 1 MVP complete.** 14 tasks done, all 27 unit tests + 1 integration test passing, `cargo run -p dse-cli` runs the full demo cycle without panic.

Two tests are marked `#[ignore]` because the plan's expectations depend on the hash-based `DummyEmbedProvider` producing semantically-similar directions, which it does not. These will be re-enabled when a real embedding model is plugged in via the `EmbedProvider` trait.

## License

Copyright (C) 2026 Tingung \<TingungX@outlook.com\>

Licensed under the **GNU Affero General Public License v3.0** (AGPL-3.0). See [`LICENSE`](LICENSE) for the full text, or visit <https://www.gnu.org/licenses/agpl-3.0.txt>.

AGPL-3.0 means: if you run a modified version of this engine as a network service, you must provide the source code of your modifications to the users of that service. This is the standard copyleft enforcement for memory-as-a-service use cases.

## See also

- [`docs/design.md`](docs/design.md) — the v2 clean-slate architecture document (potential-field design rationale)
- [`docs/superpowers/plans/2026-06-13-dse-phase1.md`](docs/superpowers/plans/2026-06-13-dse-phase1.md) — the 14-task Phase 1 implementation plan
- [`AGENTS.md`](AGENTS.md) — project-local agent instructions

