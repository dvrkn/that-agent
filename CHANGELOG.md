# Changelog

All notable changes to this project will be documented here.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning follows [Semantic Versioning](https://semver.org/).

---

## [Unreleased]

## [0.1.0] — 2026-02-27

### Added
- Core runtime: single orchestration loop across run, chat, TUI, listen, and eval modes
- Tool engine with policy gate (Allow / Prompt / Deny) for all capability categories
- SQLite-backed persistent memory with FTS5 semantic search
- JSONL session transcripts with history reconstruction
- Docker and Kubernetes sandbox backends for elevated-autonomy isolation
- Heartbeat system: autonomous listen mode with configurable wakeup schedules
- Plugin system: runtime extensions via commands, activations, and routines
- Channel routing: Telegram, HTTP, and TUI adapters through a unified interface
- Eval harness: TOML scenario runner with LLM judge and structured reports
- Workspace identity files: Soul, Identity, Agents, User, Tools, Heartbeat, Tasks
- Skill system: markdown-based capability extensions with hot-reload
- Kubernetes deployment manifests with BuildKit sidecar and in-cluster registry
- VPS one-liner installer (`scripts/install.sh`) for k3s deployments
- File attachment support in channel messages
