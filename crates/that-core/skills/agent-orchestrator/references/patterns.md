# Orchestration Patterns Reference

Detailed patterns for structuring multi-agent workflows. Each pattern describes
when to use it, how to set it up, and what to watch out for.

## Fan-Out / Fan-In

**When**: You have a large task that can be decomposed into independent subtasks.

**Structure**:
1. Parent analyzes the task and identifies independent work units
2. Spawn N child agents, each assigned one work unit with a clear role
3. Children work in parallel on isolated branches or workspaces
4. Parent collects results from all children
5. Parent synthesizes the combined output

**Best practices**:
- Keep subtasks genuinely independent — shared state creates coordination overhead
- Set consistent turn budgets across children to avoid stragglers
- Use worktrees for code tasks, isolated workspaces for non-code tasks
- Have a timeout strategy for children that take too long

## Pipeline (Sequential Delegation)

**When**: Work flows through stages where each stage depends on the previous one.

**Structure**:
1. Parent spawns Agent A with the first stage task
2. When A completes, parent reviews output and spawns Agent B with stage two
3. Continue until all stages are complete
4. Parent synthesizes the final result

**Best practices**:
- Each stage should produce clear, well-defined artifacts
- The parent reviews between stages to catch issues early
- Use inherited workspaces so each stage can build on the previous one
- Keep pipeline stages focused — if a stage is too large, decompose it

## Explorer / Developer Split

**When**: A task requires both research and implementation.

**Structure**:
1. Parent spawns an explorer agent to research the problem space
2. Explorer investigates, reads code, searches the web, and reports findings
3. Parent reviews the research and formulates an implementation plan
4. Parent spawns a developer agent with the plan and research context
5. Developer implements the solution
6. Parent reviews and merges the result

**Best practices**:
- Give the explorer a clear research question, not a vague directive
- Summarize the explorer's findings before passing to the developer
- The developer should receive a concrete plan, not raw research output
- Consider adding a reviewer step after the developer completes

## Review Pattern

**When**: Code changes need independent verification before merging.

**Structure**:
1. Developer agent commits changes to a worktree branch
2. Parent spawns a reviewer agent and points it at the worktree diff
3. Reviewer examines the changes, runs tests, and reports findings
4. If changes pass review, parent merges the branch
5. If changes need revision, parent sends feedback to the developer

**Best practices**:
- Reviewers should have access to the full codebase context
- Define clear review criteria (tests pass, no security issues, style compliance)
- Reviewer and developer should use inherited workspace for shared context
- Keep review cycles bounded — set a max number of revision rounds

## Specialist Team

**When**: A complex project requires diverse expertise.

**Structure**:
1. Parent assembles a team based on the project requirements
2. Each specialist handles their domain (frontend, backend, testing, docs)
3. Specialists work on separate worktree branches
4. Parent coordinates integration points and resolves conflicts
5. Parent merges branches in dependency order

**Best practices**:
- Define clear interface contracts between specialists
- Merge frequently to catch integration issues early
- The parent should maintain a project-level view and coordinate timing
- Store team composition in memory for reuse on similar projects
