---
name: agent-orchestrator
description: Deploy, scope, and manage child agents for parallel task execution. Covers spawning, workspace sharing, memory-based team evolution, and result aggregation.
triggers:
  - orchestrate agents
  - spawn subagent
  - deploy child agent
  - team of agents
  - multi-agent
  - delegate task
  - agent hierarchy
metadata:
  bootstrap: true
  version: 1.0.0
---

# Multi-Agent Orchestration

This skill describes how to deploy, scope, and manage child agents as a root (parent)
agent. Use it when you need to parallelize work, delegate specialized tasks, or
build a team of agents that collaborate on a shared goal.

## Spawning Subagents

When deploying a child agent, always establish the hierarchy relationship:

1. **Set the parent flag** so the child knows who spawned it and can report back
2. **Assign a role** that scopes the child's responsibility clearly
3. **Choose a workspace strategy** based on whether the child needs access to shared data

The child agent receives hierarchy context in its preamble and adjusts its behavior
accordingly — focusing on its assigned scope rather than acting as a general-purpose agent.

### Deployment Modes

- **Local Docker sandbox**: Spawn a new agent process with hierarchy flags. The sandbox
  container is created automatically with labels for the parent and role, enabling
  filtering via container inspection.
- **Kubernetes**: Deploy an overlay that patches hierarchy labels and configmap values.
  Use label selectors to query all agents in a hierarchy tree.

## Workspace Strategies

Choose the right workspace isolation level for each child agent:

### Isolated Workspace (Default)
Each child gets its own workspace directory. Best for:
- Independent tasks with no shared state
- Exploratory work that should not affect other agents
- Tasks where the child produces artifacts to be collected later

### Inherited Workspace
The child uses the parent's workspace directory. Best for:
- Shared data access (databases, config files, shared state)
- Tasks that need to read the parent's artifacts directly
- Tightly coupled workflows where agents operate on the same data

### Worktree Isolation
The child works on a separate git branch via worktrees. Best for:
- Parallel code changes on the same repository
- Feature development where each agent owns a branch
- Code review workflows (developer writes, reviewer checks)

Use the `agent-worktree` skill for detailed worktree orchestration patterns.

## Communication Patterns

### Synchronous Query
Send a task to a child agent and wait for the result. Use the remote query tool
to communicate with agents running HTTP gateway channels. Best for short,
well-defined tasks with clear deliverables.

### Asynchronous Delegation
Deploy a child agent with a task in its preamble or bootstrap instructions.
Check on progress periodically via remote query or by reviewing worktree diffs.
Best for longer-running tasks where blocking is not acceptable.

### Result Collection
After a child completes work:
1. Review changes via worktree diff or by reading artifacts from the child's workspace
2. Merge completed branches or collect output files
3. Clean up the child's resources when done

## Scoping Principles

Effective orchestration depends on clear boundaries:

- **One role per agent**: Each child should have a single, clear responsibility
- **Minimal tool access**: Scope the child's capabilities to what its role requires
- **Bounded turn budget**: Set appropriate max-turns so children do not run indefinitely
- **Clear deliverables**: Define what "done" looks like for each child's task

### Common Roles

| Role | Purpose |
|------|---------|
| explorer | Research, codebase analysis, information gathering |
| developer | Implementation, code changes, feature development |
| reviewer | Code review, testing, quality assurance |
| deployer | Build, push, and deploy artifacts |
| researcher | Web search, documentation review, knowledge synthesis |

## Memory-Based Team Evolution

After orchestration completes, store learnings in persistent memory to improve
future team compositions:

- **What worked**: Which role assignments produced good results for which task types
- **Configuration insights**: Agent settings that worked well (model, turn budget, tools)
- **Failure patterns**: Common issues and how to prevent them in future runs
- **Team compositions**: Effective agent team structures for recurring task patterns

Use `mem_add` to store these observations and `mem_recall` to retrieve them when
planning the next orchestration. Over time, your memory builds a knowledge base
of effective multi-agent strategies.

## Lifecycle Management

### Monitoring
- Query running children for status updates
- Check worktree diffs to see work in progress
- Use container/pod labels to list all agents in a hierarchy

### Cleanup
- Merge or collect results from completed children
- Discard worktrees after merging
- Remove sandbox containers when no longer needed
- Update task status to reflect completed delegations

### Error Handling
- If a child agent fails, inspect its output and determine whether to retry or reassign
- Consider splitting failed tasks into smaller, more focused subtasks
- Store failure patterns in memory to avoid repeating them
