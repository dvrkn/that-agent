# Contributing to that-agent

## What this project is

The agent manages its own home. Its capabilities, deployed services, and environment are expressed as plugins it authors, ships, and upgrades at runtime. The foundation — orchestration, memory, sandboxing, channels, eval — is deliberately stable. It exists to make that self-management safe and testable.

Contributions fall into two categories:

**Foundation contributions** — improvements to the core substrate: orchestration correctness, tool reliability, memory integrity, sandbox safety, eval coverage, performance. These are always in scope. The foundation needs to be rock solid for the agent to build confidently on top of it.

**Capability contributions** — new integrations, protocols, channel adapters, or runtime behaviors. Before adding these to the core, ask: can the agent build and maintain this itself as a plugin? If yes, it belongs as a plugin, not as a crate. The project's long-term goal is that the agent expands its own capabilities without requiring PRs into this repository.

If you are unsure which category your contribution falls into, open an issue before writing code.

See [`docs/self-knowledge-map.md`](./docs/self-knowledge-map.md) for a breakdown of how well the agent currently understands each aspect of itself, and where the highest-leverage gaps are.

---

## Development setup

1. Install stable Rust via [rustup](https://rustup.rs/).
2. Clone the repo and enter the workspace.
3. Copy the env template:
   ```bash
   cp .env.example .env
   ```
4. Provide at least one provider key (`ANTHROPIC_API_KEY` or `OPENAI_API_KEY`).

## Validation

Before opening a PR, run:

```bash
cargo fmt --all
cargo test --workspace
```

If your change affects observable behavior, update the relevant docs (`README.md`, `ARCHITECTURE.md`, `OPERATORS.md`) and add or update eval scenarios under `evals/scenarios/`.

## Pull requests

- Keep changes focused. One concern per PR.
- Explain what changed and why — not just what.
- Describe how you validated the behavior.
- Do not include secrets, `.env` files, or machine-specific config.

## Prompt and eval guidelines

- Write prompts as natural human requests. Never name internal tools, skill identifiers, or workflow steps in user-facing prompts.
- Test outcomes, not implementation details.
- Scenarios that require destructive operations must set `sandbox = true`.
