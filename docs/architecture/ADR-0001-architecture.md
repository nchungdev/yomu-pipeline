# ADR-0001: Project Base Architecture, Security Gates & Multi-Agent Standards

- **Date**: 2026-11-09 | **Status**: Accepted | **Project**: Yomu Pipeline | **Stack**: Rust (single crate / headless library + CLI worker)

## Decision
- **Architecture Style**: `lightweight` | **Flow Pattern**: `headless-pipeline`
- **Reference Architecture Anchor**: [`LukeMathWalker/zero-to-production`](https://github.com/LukeMathWalker/zero-to-production)
- **Boundary Enforcement**: Module boundaries inside one crate: pure stage modules (contour, inpaint, layout, typography, mask, types) never import the CLI/worker (`src/bin`); zero `egui`/`eframe`/`yomu` dependencies
- **File Size Policy**: `1 File = 1 Primary Responsibility`, warn at `300 LOC`, hard block at `450 LOC`.
- **Security & Git Gates**: Staged secret scanning, OWASP heuristics, Conventional Commits, and pre-push test/CVE verification via `.githooks/`.

### Selected Base Capabilities
- `solid-dry`
- `typed-errors`
- `arch-lint`
- `testing-ci`
- `di-core`
