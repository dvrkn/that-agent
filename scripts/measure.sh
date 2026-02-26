#!/usr/bin/env bash
# measure.sh — workspace metrics dashboard
# Outputs: lines of code per crate, preamble token budget, and context window utilization.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CRATES_DIR="$REPO_ROOT/crates"

echo "=============================="
echo "  that-agent Metrics"
echo "=============================="
echo ""

# ─── 1. Lines of Code ─────────────────────────────────────────────────────────

echo "## Lines of Code"
echo ""

if command -v tokei &>/dev/null; then
    tokei "$CRATES_DIR"
else
    echo "(tokei not found — using wc -l fallback)"
    echo ""
    total=0
    for crate_dir in "$CRATES_DIR"/*/; do
        crate_name="$(basename "$crate_dir")"
        count=$(find "$crate_dir/src" -name '*.rs' 2>/dev/null | xargs wc -l 2>/dev/null | tail -1 | awk '{print $1}')
        count=${count:-0}
        printf "  %-28s %6d lines\n" "$crate_name" "$count"
        total=$((total + count))
    done
    echo ""
    printf "  %-28s %6d lines\n" "TOTAL" "$total"
fi

echo ""

# ─── 2. Preamble Token Budget ─────────────────────────────────────────────────

echo "## Preamble Token Budget"
echo ""

REPO_ROOT_FOR_PY="$REPO_ROOT" python3 - <<'PYEOF'
import sys, re, os

# Token counter — use tiktoken when available, else len//4
try:
    import tiktoken
    enc = tiktoken.get_encoding("cl100k_base")
    def count_tokens(text):
        return len(enc.encode(text))
    token_method = "tiktoken cl100k_base"
except ImportError:
    def count_tokens(text):
        return len(text) // 4
    token_method = "fallback (len/4)"

print(f"  Token counting: {token_method}")
print()

repo_root = os.environ["REPO_ROOT_FOR_PY"]
crates_dir = os.path.join(repo_root, "crates")

# ── 2a. Always-true skills ─────────────────────────────────────────────────────
# Skills with `always: true` in frontmatter are injected into every preamble.
skill_rows = []
for dirpath, dirnames, filenames in os.walk(crates_dir):
    for fname in filenames:
        if fname != "SKILL.md":
            continue
        fpath = os.path.join(dirpath, fname)
        with open(fpath, encoding="utf-8") as f:
            content = f.read()
        # Check for always: true in YAML frontmatter
        fm_match = re.match(r'^---\s*\n(.*?)\n---', content, re.DOTALL)
        if not fm_match:
            continue
        fm = fm_match.group(1)
        if not re.search(r'^\s*always\s*:\s*true', fm, re.MULTILINE):
            continue
        name_match = re.search(r'^\s*name\s*:\s*(.+)', fm, re.MULTILINE)
        skill_name = name_match.group(1).strip() if name_match else fpath
        toks = count_tokens(content)
        skill_rows.append((skill_name, toks))

print("  Always-injected skills:")
skill_total = 0
for name, toks in sorted(skill_rows):
    print(f"    {name:<30} {toks:>5} tokens")
    skill_total += toks
print(f"    {'SUBTOTAL':<30} {skill_total:>5} tokens")
print()

# ── 2b. Default workspace markdown functions ───────────────────────────────────
workspace_path = os.path.join(crates_dir, "that-core", "src", "workspace", "mod.rs")
preamble_path  = os.path.join(crates_dir, "that-core", "src", "orchestration", "preamble.rs")

def extract_raw_string(source, fn_name):
    """Extract the r#"..."# content from a pub fn that returns &'static str."""
    pattern = rf'pub fn {re.escape(fn_name)}\(\)'
    m = re.search(pattern, source)
    if not m:
        return None
    start = source.find('r#"', m.start())
    if start == -1:
        return None
    end = source.find('"#', start + 3)
    if end == -1:
        return None
    return source[start + 3:end]

with open(workspace_path, encoding="utf-8") as f:
    workspace_src = f.read()

ws_fns = [
    ("default_agents_md",    "Agents.md default"),
    ("default_soul_md",      "Soul.md default"),
    ("default_identity_md",  "Identity.md default"),
    ("default_bootstrap_md", "Bootstrap.md default"),
]

print("  Workspace default templates:")
ws_total = 0
for fn_name, label in ws_fns:
    content = extract_raw_string(workspace_src, fn_name)
    if content is None:
        print(f"    {label:<30}  (not found)")
        continue
    toks = count_tokens(content)
    print(f"    {label:<30} {toks:>5} tokens")
    ws_total += toks
print(f"    {'SUBTOTAL':<30} {ws_total:>5} tokens")
print()

# ── 2c. Static preamble sections ──────────────────────────────────────────────
# Extract all string literals pushed to `preamble` in build_preamble().
with open(preamble_path, encoding="utf-8") as f:
    preamble_src = f.read()

# Collect all push_str literal content in build_preamble fn
static_strings = re.findall(r'preamble\.push_str\(\s*"((?:[^"\\]|\\.)*)"\s*\)', preamble_src)
static_total = sum(count_tokens(s) for s in static_strings)
# Also count format! string templates (just the literal portions)
fmt_strings = re.findall(r'preamble\.push_str\(&format!\(\s*"((?:[^"\\]|\\.)*)"\s*,', preamble_src)
fmt_total = sum(count_tokens(s) for s in fmt_strings)

print("  Static preamble sections (preamble.rs literals):")
print(f"    {'push_str literals':<30} {static_total:>5} tokens (approx)")
print(f"    {'format! templates':<30} {fmt_total:>5} tokens (approx)")
static_sub = static_total + fmt_total
print(f"    {'SUBTOTAL':<30} {static_sub:>5} tokens")
print()

# ── Summary ───────────────────────────────────────────────────────────────────
baseline = skill_total + ws_total + static_sub
print(f"  {'─'*44}")
print(f"  {'ESTIMATED SYSTEM PROMPT BASELINE':<34} {baseline:>5} tokens")
print(f"  (soul+identity+agents = 3 workspace files injected per session)")
print()

# ── 3. Context window utilization ─────────────────────────────────────────────
# Context sizes in descending order (tokens). Labels are tier descriptions,
# not provider names — any model at that tier maps to the same row.
context_tiers = [
    (1_000_000, "1M  (current max tier)"),
    (  400_000, "400K"),
    (  200_000, "200K"),
    (  128_000, "128K"),
]

print("## Context Window Utilization")
print()
print(f"  {'Context':<24} {'Window':>8}  {'Used':>6}  {'Remaining':>10}  {'% used':>7}")
print(f"  {'─'*24}  {'─'*8}  {'─'*6}  {'─'*10}  {'─'*7}")
for window, label in context_tiers:
    remaining = window - baseline
    pct = baseline / window * 100
    bar_full = 30
    bar_used = max(1, round(pct / 100 * bar_full)) if pct < 100 else bar_full
    bar = "█" * bar_used + "░" * (bar_full - bar_used)
    print(f"  {label:<24} {window:>8,}  {baseline:>6,}  {remaining:>10,}  {pct:>6.2f}%")
    print(f"  {bar}")
    print()
PYEOF

echo ""
echo "Done."
