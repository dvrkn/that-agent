//! Token budget engine for Anvil.
//!
//! Every command output passes through this pipeline:
//! 1. Tool produces a typed result struct
//! 2. `ToolContext::emit()` serializes to the requested format
//! 3. If over budget, the *data* is reduced (array truncation, field elision)
//!    **before** serialization — guaranteeing valid output at every budget
//! 4. Final serialized output is measured and returned
//!
//! Key invariant: output is ALWAYS valid JSON when format is JSON.
//! Budget is enforced on the FINAL emitted payload, including envelope.

use serde::Serialize;
use std::sync::LazyLock;
use tiktoken_rs::CoreBPE;

/// Cached tokenizer instance — initialized once, used everywhere.
static TOKENIZER: LazyLock<CoreBPE> =
    LazyLock::new(|| tiktoken_rs::cl100k_base().expect("tokenizer initialization should not fail"));

/// Approximate token count for a string using cl100k_base (GPT-4/Claude tokenizer).
pub fn count_tokens(text: &str) -> usize {
    TOKENIZER.encode_ordinary(text).len()
}

/// The result of applying a token budget to output.
#[derive(Debug, Clone, Serialize)]
pub struct BudgetedOutput {
    /// The final output content — always valid in the requested format.
    pub content: String,
    /// Actual token count of the returned content.
    pub tokens: usize,
    /// Whether the output was truncated or compacted.
    pub truncated: bool,
    /// Original token count before compaction (0 if not applicable).
    pub original_tokens: usize,
}

/// An envelope that wraps tool output with budget metadata for agents.
///
/// Only emitted in JSON mode. Human-facing formats (markdown, raw, compact) are unaffected.
/// `flush_recommended` is set when the output was heavily truncated
/// (`original_tokens > tokens * 2`), signalling the agent to consider compaction.
#[derive(Debug, Clone, Serialize)]
pub struct OutputEnvelope {
    /// The actual tool result data.
    pub data: serde_json::Value,
    /// Actual token count of the returned content.
    pub tokens: usize,
    /// Whether the output was truncated to fit the budget.
    pub truncated: bool,
    /// Original token count before compaction (0 when not truncated).
    pub original_tokens: usize,
    /// True when truncation was severe (original > 2x returned), suggesting compaction.
    pub flush_recommended: bool,
}

impl OutputEnvelope {
    pub fn from_budgeted(budgeted: &BudgetedOutput) -> Self {
        let data = serde_json::from_str(&budgeted.content)
            .unwrap_or(serde_json::Value::String(budgeted.content.clone()));
        let flush_recommended =
            budgeted.truncated && budgeted.original_tokens > budgeted.tokens * 2;
        Self {
            data,
            tokens: budgeted.tokens,
            truncated: budgeted.truncated,
            original_tokens: budgeted.original_tokens,
            flush_recommended,
        }
    }
}

/// Strategy for compacting output that exceeds the token budget.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
#[derive(Default)]
pub enum CompactionStrategy {
    /// Keep the first and last portions, elide the middle.
    #[default]
    HeadTail,
    /// Keep only the first N tokens worth of content.
    HeadOnly,
    /// Rule-based extraction of key information.
    RuleBased,
}

/// Applies a token budget to plain text (non-JSON).
///
/// Used for internal content fields (file content, code snippets) before
/// they are placed into a JSON envelope.
pub fn apply_budget_to_text(
    text: &str,
    max_tokens: usize,
    strategy: CompactionStrategy,
) -> BudgetedOutput {
    let original_tokens = count_tokens(text);

    if original_tokens <= max_tokens {
        return BudgetedOutput {
            content: text.to_string(),
            tokens: original_tokens,
            truncated: false,
            original_tokens,
        };
    }

    let compacted = match strategy {
        CompactionStrategy::HeadTail => compact_head_tail(text, max_tokens),
        CompactionStrategy::HeadOnly => compact_head_only(text, max_tokens),
        CompactionStrategy::RuleBased => compact_rule_based(text, max_tokens),
    };

    let tokens = count_tokens(&compacted);
    BudgetedOutput {
        content: compacted,
        tokens,
        truncated: true,
        original_tokens,
    }
}

/// Serializes a value to JSON and enforces a hard token budget on the final output.
///
/// If the serialized JSON exceeds the budget, the value is re-serialized with
/// structural reduction (array truncation, compact formatting) to fit.
///
/// **Invariant**: The returned `content` is ALWAYS valid JSON.
pub fn emit_json<T: Serialize>(value: &T, max_tokens: Option<usize>) -> BudgetedOutput {
    match max_tokens {
        None => {
            let pretty_json =
                serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string());
            let original_tokens = count_tokens(&pretty_json);
            BudgetedOutput {
                content: pretty_json,
                tokens: original_tokens,
                truncated: false,
                original_tokens,
            }
        }
        Some(budget) => {
            // Compact JSON is the cheapest stable baseline for budget checks.
            let compact_json = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
            let compact_tokens = count_tokens(&compact_json);

            // If compact already exceeds budget, pretty definitely won't fit.
            if compact_tokens > budget {
                let truncated_json = truncate_json_value(value, budget);
                let final_tokens = count_tokens(&truncated_json);
                BudgetedOutput {
                    content: truncated_json,
                    tokens: final_tokens,
                    truncated: true,
                    original_tokens: compact_tokens,
                }
            } else {
                // Compact fits; try pretty and keep it when within budget for readability.
                let pretty_json =
                    serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string());
                let pretty_tokens = count_tokens(&pretty_json);

                if pretty_tokens <= budget {
                    BudgetedOutput {
                        content: pretty_json,
                        tokens: pretty_tokens,
                        truncated: false,
                        original_tokens: pretty_tokens,
                    }
                } else {
                    BudgetedOutput {
                        content: compact_json,
                        tokens: compact_tokens,
                        truncated: false,
                        original_tokens: pretty_tokens,
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct ValueStats {
    arrays: usize,
    strings: usize,
    max_array_len: usize,
    max_string_len: usize,
}

fn collect_value_stats(value: &serde_json::Value, stats: &mut ValueStats) {
    match value {
        serde_json::Value::String(s) => {
            stats.strings += 1;
            stats.max_string_len = stats.max_string_len.max(s.len());
        }
        serde_json::Value::Array(arr) => {
            stats.arrays += 1;
            stats.max_array_len = stats.max_array_len.max(arr.len());
            for v in arr {
                collect_value_stats(v, stats);
            }
        }
        serde_json::Value::Object(map) => {
            for v in map.values() {
                collect_value_stats(v, stats);
            }
        }
        _ => {}
    }
}

fn reduction_plan(stats: ValueStats) -> &'static [(usize, usize)] {
    // (max_string_len, max_array_len), ordered from least to most aggressive.
    const ARRAY_HEAVY: &[(usize, usize)] = &[
        (2000, 30),
        (1200, 20),
        (800, 15),
        (500, 10),
        (300, 7),
        (200, 5),
        (150, 3),
        (100, 2),
        (80, 1),
        (50, 1),
        (30, 1),
    ];
    const STRING_HEAVY: &[(usize, usize)] = &[
        (1200, 50),
        (800, 30),
        (500, 20),
        (300, 15),
        (220, 10),
        (180, 7),
        (140, 5),
        (100, 3),
        (80, 2),
        (50, 1),
        (30, 1),
        (20, 1),
    ];
    const BALANCED: &[(usize, usize)] = &[
        (1500, 30),
        (900, 20),
        (600, 15),
        (400, 10),
        (280, 7),
        (220, 5),
        (170, 3),
        (120, 2),
        (90, 1),
        (60, 1),
        (40, 1),
        (30, 1),
    ];

    if stats.max_array_len >= 100 || stats.arrays > stats.strings.saturating_mul(2) {
        ARRAY_HEAVY
    } else if stats.max_string_len >= 2000 || stats.strings > stats.arrays.saturating_mul(2) {
        STRING_HEAVY
    } else {
        BALANCED
    }
}

/// Structurally truncate a JSON value to fit within a token budget.
///
/// Strategy: parse to serde_json::Value, then progressively reduce with a
/// shape-aware reduction plan to avoid excessive trial serializations.
///
/// Always produces valid JSON.
fn truncate_json_value<T: Serialize>(value: &T, budget: usize) -> String {
    let mut json_value: serde_json::Value =
        serde_json::to_value(value).unwrap_or(serde_json::Value::Null);

    let mut stats = ValueStats::default();
    collect_value_stats(&json_value, &mut stats);

    for &(max_string_len, max_array_len) in reduction_plan(stats) {
        let reduced = reduce_value(&json_value, max_string_len, max_array_len);
        let serialized = serde_json::to_string(&reduced).unwrap_or_else(|_| "{}".to_string());
        let tokens = count_tokens(&serialized);
        if tokens <= budget {
            return serialized;
        }
    }

    // Last resort: aggressively reduce to skeleton.
    json_value = reduce_value(&json_value, 20, 1);
    let serialized = serde_json::to_string(&json_value).unwrap_or_else(|_| "{}".to_string());

    if count_tokens(&serialized) > budget {
        let skeleton = extract_skeleton(&json_value);
        let skeleton_str = serde_json::to_string(&skeleton).unwrap_or_else(|_| "{}".to_string());
        if count_tokens(&skeleton_str) <= budget {
            return skeleton_str;
        }
        return r#"{"budget_exhausted":true}"#.to_string();
    }

    serialized
}

/// Recursively reduce a JSON value by shortening strings and truncating arrays.
fn reduce_value(
    value: &serde_json::Value,
    max_string_len: usize,
    max_array_len: usize,
) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => {
            if s.len() > max_string_len {
                // Truncate at a valid UTF-8 boundary without scanning chars one by one.
                let mut end = max_string_len.min(s.len());
                while end > 0 && !s.is_char_boundary(end) {
                    end -= 1;
                }
                let truncated = &s[..end];
                serde_json::Value::String(format!("{}...[truncated]", truncated))
            } else {
                value.clone()
            }
        }
        serde_json::Value::Array(arr) => {
            let mut reduced: Vec<serde_json::Value> = arr
                .iter()
                .take(max_array_len)
                .map(|v| reduce_value(v, max_string_len, max_array_len))
                .collect();
            if arr.len() > max_array_len {
                reduced.push(
                    serde_json::json!({"_truncated": true, "remaining": arr.len() - max_array_len}),
                );
            }
            serde_json::Value::Array(reduced)
        }
        serde_json::Value::Object(map) => {
            let reduced = map
                .iter()
                .map(|(k, v)| (k.clone(), reduce_value(v, max_string_len, max_array_len)))
                .collect();
            serde_json::Value::Object(reduced)
        }
        _ => value.clone(),
    }
}

/// Extract a skeleton from a JSON value: keep only top-level scalars
/// (numbers, bools, strings <=50 chars), drop arrays and nested objects,
/// and add a "truncated" marker.
fn extract_skeleton(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut skeleton = serde_json::Map::new();
            for (k, v) in map {
                match v {
                    serde_json::Value::Number(_) | serde_json::Value::Bool(_) => {
                        skeleton.insert(k.clone(), v.clone());
                    }
                    serde_json::Value::String(s) if s.len() <= 50 => {
                        skeleton.insert(k.clone(), v.clone());
                    }
                    _ => {}
                }
            }
            skeleton.insert("truncated".to_string(), serde_json::Value::Bool(true));
            serde_json::Value::Object(skeleton)
        }
        _ => serde_json::json!({"truncated": true}),
    }
}

/// Keeps the first ~60% and last ~40% of the usable budget, with an elision marker.
/// Reserves tokens for the marker, then redistributes unused head tokens to tail.
fn compact_head_tail(text: &str, max_tokens: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= 3 {
        return compact_head_only(text, max_tokens);
    }

    // Reserve ~5 tokens for the elision marker before splitting
    let marker_reserve = 5;
    let usable = max_tokens.saturating_sub(marker_reserve);
    let head_budget = (usable as f64 * 0.6) as usize;

    let mut head_lines = Vec::new();
    let mut head_tokens = 0;
    for line in &lines {
        let line_tokens = count_tokens(line);
        if head_tokens + line_tokens > head_budget {
            break;
        }
        head_lines.push(*line);
        head_tokens += line_tokens;
    }

    // Redistribute unused head tokens to the tail budget
    let unused_head = head_budget.saturating_sub(head_tokens);
    let base_tail_budget = (usable as f64 * 0.4) as usize;
    let tail_budget = base_tail_budget + unused_head;

    let mut tail_lines = Vec::new();
    let mut tail_tokens = 0;
    for line in lines.iter().rev() {
        let line_tokens = count_tokens(line);
        if tail_tokens + line_tokens > tail_budget {
            break;
        }
        tail_lines.push(*line);
        tail_tokens += line_tokens;
    }
    tail_lines.reverse();

    let omitted = lines
        .len()
        .saturating_sub(head_lines.len() + tail_lines.len());
    let mut result = head_lines.join("\n");
    if omitted > 0 {
        result.push_str(&format!("\n... ({} lines omitted) ...\n", omitted));
    }
    result.push_str(&tail_lines.join("\n"));
    result
}

/// Keeps only the beginning of the text up to the budget.
fn compact_head_only(text: &str, max_tokens: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut result_lines = Vec::new();
    let mut token_count = 0;

    for line in &lines {
        let line_tokens = count_tokens(line);
        if token_count + line_tokens > max_tokens.saturating_sub(5) {
            break;
        }
        result_lines.push(*line);
        token_count += line_tokens;
    }

    let omitted = lines.len().saturating_sub(result_lines.len());
    let mut result = result_lines.join("\n");
    if omitted > 0 {
        result.push_str(&format!("\n... ({} more lines truncated)", omitted));
    }
    result
}

/// Rule-based compaction: extract key patterns (errors, headings, summaries).
fn compact_rule_based(text: &str, max_tokens: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();

    let priority_patterns = [
        "error", "Error", "ERROR", "warn", "Warn", "WARN", "panic", "fail", "FAIL",
    ];
    let mut priority_lines: Vec<&str> = Vec::new();
    let mut other_lines: Vec<&str> = Vec::new();

    for line in &lines {
        if priority_patterns.iter().any(|p| line.contains(p)) {
            priority_lines.push(line);
        } else {
            other_lines.push(line);
        }
    }

    let mut result_lines = Vec::new();
    let mut token_count = 0;
    let budget = max_tokens.saturating_sub(5);

    for line in priority_lines.iter().chain(other_lines.iter()) {
        let line_tokens = count_tokens(line);
        if token_count + line_tokens > budget {
            break;
        }
        result_lines.push(*line);
        token_count += line_tokens;
    }

    let total = lines.len();
    let included = result_lines.len();
    let mut result = result_lines.join("\n");
    if included < total {
        result.push_str(&format!("\n... ({} of {} lines shown)", included, total));
    }
    result
}

/// Strip non-essential fields from JSON output for compact format.
///
/// Detects the command type by key signatures and removes fields that
/// add tokens without adding useful information for agents:
/// - Tree entries: remove `depth`
/// - Symbols: remove `byte_start`, `byte_end`
/// - Grep/ast-grep matches: remove `context_before`, `context_after`, empty `captures`
pub fn compact_json_value(value: &serde_json::Value) -> serde_json::Value {
    if let Some(obj) = value.as_object() {
        // Tree result: strip depth from entries
        if obj.contains_key("root")
            && obj.contains_key("entries")
            && obj.contains_key("total_files")
        {
            let mut out = obj.clone();
            if let Some(entries) = out.get_mut("entries").and_then(|v| v.as_array_mut()) {
                for entry in entries.iter_mut() {
                    if let Some(o) = entry.as_object_mut() {
                        o.remove("depth");
                    }
                }
            }
            return serde_json::Value::Object(out);
        }
        // Grep/ast-grep result: strip context and deprecated fields in compact mode
        if obj.contains_key("pattern")
            && obj.contains_key("matches")
            && obj.contains_key("total_matches")
        {
            let mut out = obj.clone();
            // Strip context from file_matches groups
            if let Some(groups) = out.get_mut("file_matches").and_then(|v| v.as_array_mut()) {
                for group in groups.iter_mut() {
                    if let Some(group_matches) =
                        group.get_mut("matches").and_then(|v| v.as_array_mut())
                    {
                        for m in group_matches.iter_mut() {
                            if let Some(o) = m.as_object_mut() {
                                o.remove("context_before");
                                o.remove("context_after");
                            }
                        }
                    }
                }
                // In compact mode with file_matches, strip the deprecated flat matches field
                out.remove("matches");
            } else {
                // Legacy format without file_matches — strip context from flat matches
                if let Some(matches) = out.get_mut("matches").and_then(|v| v.as_array_mut()) {
                    for m in matches.iter_mut() {
                        if let Some(o) = m.as_object_mut() {
                            o.remove("context_before");
                            o.remove("context_after");
                            if let Some(caps) = o.get("captures") {
                                if caps.as_object().is_some_and(|c| c.is_empty()) {
                                    o.remove("captures");
                                }
                            }
                        }
                    }
                }
            }
            return serde_json::Value::Object(out);
        }
    }
    // Search results: strip score, source from individual results
    if let Some(obj) = value.as_object() {
        if obj.contains_key("query")
            && obj.contains_key("results")
            && obj.contains_key("total_results")
        {
            let mut out = obj.clone();
            if let Some(results) = out.get_mut("results").and_then(|v| v.as_array_mut()) {
                for r in results.iter_mut() {
                    if let Some(o) = r.as_object_mut() {
                        o.remove("score");
                        o.remove("source");
                    }
                }
            }
            return serde_json::Value::Object(out);
        }
    }
    // Symbol arrays: strip byte_start, byte_end
    if let Some(arr) = value.as_array() {
        if arr
            .first()
            .is_some_and(|v| v.get("kind").is_some() && v.get("line_start").is_some())
        {
            let compacted: Vec<serde_json::Value> = arr
                .iter()
                .map(|v| {
                    if let Some(o) = v.as_object() {
                        let mut out = o.clone();
                        out.remove("byte_start");
                        out.remove("byte_end");
                        serde_json::Value::Object(out)
                    } else {
                        v.clone()
                    }
                })
                .collect();
            return serde_json::Value::Array(compacted);
        }
    }
    value.clone()
}

/// Render a JSON value as plain text suitable for piping and simple consumption.
///
/// Detects the command type from the JSON structure and renders as:
/// - Tree: newline-separated paths (dirs suffixed with `/`)
/// - Symbols: `name (kind) L:start-end` per line
/// - Grep/ast-grep: `file:line: content` per line
/// - fs ls: `name\ttype\tsize` per line
/// - Code read/fs cat: extract `content` field
/// - Index/edit: `key: value` per line
/// - Generic: fallback key-value rendering
pub fn render_raw(value: &serde_json::Value) -> String {
    if let Some(obj) = value.as_object() {
        // Tree result
        if obj.contains_key("root")
            && obj.contains_key("entries")
            && obj.contains_key("total_files")
        {
            return render_raw_tree(obj);
        }
        // Code read: extract content
        if obj.contains_key("path") && obj.contains_key("language") && obj.contains_key("content") {
            return obj
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
        }
        // Grep/ast-grep result (detect by file_matches or flat matches)
        if obj.contains_key("pattern")
            && (obj.contains_key("file_matches") || obj.contains_key("matches"))
            && obj.contains_key("total_matches")
        {
            return render_raw_grep(obj);
        }
        // fs ls
        if obj.contains_key("entries") && obj.contains_key("total") {
            return render_raw_fs_ls(obj);
        }
        // Search results
        if obj.contains_key("query")
            && obj.contains_key("results")
            && obj.contains_key("total_results")
        {
            return render_raw_search(obj);
        }
        // Fetch result
        if obj.contains_key("url") && obj.contains_key("content") && obj.contains_key("word_count")
        {
            return obj
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
        }
        // fs cat
        if obj.contains_key("path") && obj.contains_key("content") {
            return obj
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
        }
        // Edit result
        if obj.contains_key("applied") && obj.contains_key("validated") {
            return render_raw_kv(obj);
        }
        // Index build/status and other objects: key-value
        return render_raw_kv(obj);
    }
    // Symbol arrays
    if let Some(arr) = value.as_array() {
        if arr
            .first()
            .is_some_and(|v| v.get("kind").is_some() && v.get("line_start").is_some())
        {
            return render_raw_symbols(arr);
        }
        // Generic array
        return arr
            .iter()
            .map(|v| match v {
                serde_json::Value::String(s) => s.clone(),
                _ => v.to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    value.to_string()
}

fn render_raw_tree(obj: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut lines = Vec::new();
    if let Some(entries) = obj.get("entries").and_then(|v| v.as_array()) {
        for entry in entries {
            let path = entry.get("path").and_then(|v| v.as_str()).unwrap_or("?");
            let etype = entry.get("type").and_then(|v| v.as_str()).unwrap_or("file");
            let suffix = if etype == "dir" { "/" } else { "" };
            lines.push(format!("{}{}", path, suffix));
        }
    }
    lines.join("\n")
}

fn render_raw_symbols(arr: &[serde_json::Value]) -> String {
    let mut lines = Vec::new();
    for sym in arr {
        if is_truncation_sentinel(sym) {
            lines.push(format_truncation_sentinel(sym));
            continue;
        }
        let name = sym.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let kind = sym.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
        let ls = sym.get("line_start").and_then(|v| v.as_u64()).unwrap_or(0);
        let le = sym.get("line_end").and_then(|v| v.as_u64()).unwrap_or(0);
        let range = if le > ls {
            format!("{}-{}", ls, le)
        } else {
            format!("{}", ls)
        };
        lines.push(format!("{} ({}) L:{}", name, kind, range));
    }
    lines.join("\n")
}

fn render_raw_grep(obj: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut lines = Vec::new();
    // Prefer file_matches (grouped format), fall back to flat matches
    if let Some(groups) = obj.get("file_matches").and_then(|v| v.as_array()) {
        for group in groups {
            if is_truncation_sentinel(group) {
                lines.push(format_truncation_sentinel(group));
                continue;
            }
            let file = group.get("file").and_then(|v| v.as_str()).unwrap_or("?");
            if let Some(matches) = group.get("matches").and_then(|v| v.as_array()) {
                for m in matches {
                    if is_truncation_sentinel(m) {
                        lines.push(format_truncation_sentinel(m));
                        continue;
                    }
                    let line = m.get("line_number").and_then(|v| v.as_u64()).unwrap_or(0);
                    let content = m.get("content").and_then(|v| v.as_str()).unwrap_or("");
                    lines.push(format!("{}:{}: {}", file, line, content.trim()));
                }
            }
        }
    } else if let Some(matches) = obj.get("matches").and_then(|v| v.as_array()) {
        for m in matches {
            if is_truncation_sentinel(m) {
                lines.push(format_truncation_sentinel(m));
                continue;
            }
            let file = m.get("file").and_then(|v| v.as_str()).unwrap_or("?");
            let line = m.get("line_number").and_then(|v| v.as_u64()).unwrap_or(0);
            let content = m.get("content").and_then(|v| v.as_str()).unwrap_or("");
            lines.push(format!("{}:{}: {}", file, line, content.trim()));
        }
    }
    lines.join("\n")
}

fn render_raw_fs_ls(obj: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut lines = Vec::new();
    if let Some(entries) = obj.get("entries").and_then(|v| v.as_array()) {
        for entry in entries {
            if is_truncation_sentinel(entry) {
                lines.push(format_truncation_sentinel(entry));
                continue;
            }
            let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let etype = entry.get("type").and_then(|v| v.as_str()).unwrap_or("?");
            let size = entry
                .get("size")
                .and_then(|v| v.as_u64())
                .map(|s| s.to_string())
                .unwrap_or_else(|| "-".to_string());
            lines.push(format!("{}\t{}\t{}", name, etype, size));
        }
    }
    lines.join("\n")
}

fn render_raw_search(obj: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut lines = Vec::new();
    let query = obj.get("query").and_then(|v| v.as_str()).unwrap_or("?");
    let engine = obj.get("engine").and_then(|v| v.as_str()).unwrap_or("?");
    lines.push(format!("Search: {} ({})", query, engine));
    lines.push(String::new());
    if let Some(results) = obj.get("results").and_then(|v| v.as_array()) {
        for (i, r) in results.iter().enumerate() {
            let title = r.get("title").and_then(|v| v.as_str()).unwrap_or("?");
            let url = r.get("url").and_then(|v| v.as_str()).unwrap_or("");
            let snippet = r.get("snippet").and_then(|v| v.as_str()).unwrap_or("");
            lines.push(format!("{}. {}", i + 1, title));
            if !url.is_empty() {
                lines.push(format!("   {}", url));
            }
            if !snippet.is_empty() {
                lines.push(format!("   {}", snippet));
            }
            lines.push(String::new());
        }
    }
    lines.join("\n")
}

fn render_raw_kv(obj: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut lines = Vec::new();
    for (key, value) in obj {
        lines.push(format!("{}: {}", key, format_json_value(value)));
    }
    lines.join("\n")
}

/// Render a JSON value as human-readable markdown.
///
/// Detects the command type from the JSON structure and renders accordingly:
/// - Tree results → indented file listing
/// - Symbol lists → table with name/kind/lines
/// - Code read → file header + fenced code block
/// - Grep/ast-grep → matches grouped by file
/// - Directory listing → table with name/type/size
/// - Index results → key-value list
/// - Edit results → summary list with optional diff
/// - Generic objects/arrays → fallback rendering
pub fn render_markdown(value: &serde_json::Value) -> String {
    if let Some(obj) = value.as_object() {
        // Detect by key signatures
        if obj.contains_key("root")
            && obj.contains_key("entries")
            && obj.contains_key("total_files")
        {
            return render_tree_md(obj);
        }
        if obj.contains_key("path") && obj.contains_key("language") && obj.contains_key("content") {
            return render_code_read_md(obj);
        }
        if obj.contains_key("pattern")
            && (obj.contains_key("file_matches") || obj.contains_key("matches"))
            && obj.contains_key("total_matches")
        {
            return render_grep_md(obj);
        }
        if obj.contains_key("entries") && obj.contains_key("total") {
            return render_fs_ls_md(obj);
        }
        if obj.contains_key("files_indexed") && obj.contains_key("symbols_added") {
            return render_index_build_md(obj);
        }
        if obj.contains_key("total_symbols") && obj.contains_key("schema_version") {
            return render_index_status_md(obj);
        }
        if obj.contains_key("applied") && obj.contains_key("validated") {
            return render_edit_md(obj);
        }
        // Search results
        if obj.contains_key("query")
            && obj.contains_key("results")
            && obj.contains_key("total_results")
        {
            return render_search_md(obj);
        }
        // Fetch result
        if obj.contains_key("url") && obj.contains_key("content") && obj.contains_key("word_count")
        {
            return render_fetch_md(obj);
        }
        if obj.contains_key("path") && obj.contains_key("content") {
            return render_fs_cat_md(obj);
        }
        // Fallback: render as key-value list
        return render_object_md(obj);
    }
    if let Some(arr) = value.as_array() {
        // Check if it's a symbol array
        if arr
            .first()
            .is_some_and(|v| v.get("kind").is_some() && v.get("line_start").is_some())
        {
            return render_symbols_md(arr);
        }
        return render_array_md(arr);
    }
    value.to_string()
}

fn render_tree_md(obj: &serde_json::Map<String, serde_json::Value>) -> String {
    let root = obj.get("root").and_then(|v| v.as_str()).unwrap_or(".");
    let total_files = obj.get("total_files").and_then(|v| v.as_u64()).unwrap_or(0);
    let total_dirs = obj.get("total_dirs").and_then(|v| v.as_u64()).unwrap_or(0);
    let mut out = format!(
        "## Tree: {}\n\n{} files, {} directories\n\n```\n",
        root, total_files, total_dirs
    );

    if let Some(entries) = obj.get("entries").and_then(|v| v.as_array()) {
        for entry in entries {
            let path = entry.get("path").and_then(|v| v.as_str()).unwrap_or("?");
            let etype = entry.get("type").and_then(|v| v.as_str()).unwrap_or("?");
            let depth = entry.get("depth").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
            let indent = "  ".repeat(depth.saturating_sub(1));
            let suffix = if etype == "dir" { "/" } else { "" };
            // Extract just the file/dir name from the path
            let name = path.rsplit('/').next().unwrap_or(path);
            out.push_str(&format!("{}{}{}\n", indent, name, suffix));
        }
    }
    out.push_str("```\n");
    out
}

fn render_code_read_md(obj: &serde_json::Map<String, serde_json::Value>) -> String {
    let path = obj.get("path").and_then(|v| v.as_str()).unwrap_or("?");
    let lang = obj.get("language").and_then(|v| v.as_str()).unwrap_or("");
    let lines = obj.get("lines").and_then(|v| v.as_u64()).unwrap_or(0);
    let tokens = obj.get("tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let truncated = obj
        .get("truncated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let trunc_str = if truncated { " (truncated)" } else { "" };
    let mut out = format!(
        "## {}\n\n**{}** | {} lines | {} tokens{}\n\n",
        path, lang, lines, tokens, trunc_str
    );

    // Symbols table if present and non-empty
    if let Some(symbols) = obj.get("symbols").and_then(|v| v.as_array()) {
        if !symbols.is_empty() {
            out.push_str("### Symbols\n\n| Name | Kind | Lines |\n|------|------|-------|\n");
            for sym in symbols {
                // Skip truncation sentinel objects
                if is_truncation_sentinel(sym) {
                    out.push_str(&format!("\n_{}_\n", format_truncation_sentinel(sym)));
                    continue;
                }
                let name = sym.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                let kind = sym.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
                let ls = sym
                    .get("line")
                    .or_else(|| sym.get("line_start"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let le = sym.get("line_end").and_then(|v| v.as_u64());
                let line_range = match le {
                    Some(end) if end != ls => format!("{}-{}", ls, end),
                    _ => format!("{}", ls),
                };
                out.push_str(&format!("| `{}` | {} | {} |\n", name, kind, line_range));
            }
            out.push('\n');
        }
    }

    // Code content
    if let Some(content) = obj.get("content").and_then(|v| v.as_str()) {
        out.push_str(&format!("```{}\n{}\n```\n", lang, content));
    }
    out
}

fn render_grep_md(obj: &serde_json::Map<String, serde_json::Value>) -> String {
    let pattern = obj.get("pattern").and_then(|v| v.as_str()).unwrap_or("?");
    let total = obj
        .get("total_matches")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let files_searched = obj
        .get("files_searched")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let mut out = format!(
        "## Grep: `{}`\n\n{} matches across {} files\n\n",
        pattern, total, files_searched
    );

    // Prefer file_matches (grouped format)
    if let Some(groups) = obj.get("file_matches").and_then(|v| v.as_array()) {
        for group in groups {
            if is_truncation_sentinel(group) {
                out.push_str(&format!("_{}_\n\n", format_truncation_sentinel(group)));
                continue;
            }
            let file = group.get("file").and_then(|v| v.as_str()).unwrap_or("?");
            out.push_str(&format!("### {}\n\n", file));

            if let Some(matches) = group.get("matches").and_then(|v| v.as_array()) {
                for m in matches {
                    if is_truncation_sentinel(m) {
                        out.push_str(&format!("_{}_\n\n", format_truncation_sentinel(m)));
                        continue;
                    }
                    render_grep_match_md(&mut out, m);
                }
            }
        }
    } else if let Some(matches) = obj.get("matches").and_then(|v| v.as_array()) {
        // Fall back to flat matches (legacy or ast-grep)
        let mut current_file = String::new();
        for m in matches {
            if is_truncation_sentinel(m) {
                out.push_str(&format!("_{}_\n\n", format_truncation_sentinel(m)));
                continue;
            }
            let file = m.get("file").and_then(|v| v.as_str()).unwrap_or("?");
            if file != current_file {
                out.push_str(&format!("### {}\n\n", file));
                current_file = file.to_string();
            }
            render_grep_match_md(&mut out, m);
        }
    }
    out
}

/// Render a single grep match in markdown (shared between grouped and flat formats).
fn render_grep_match_md(out: &mut String, m: &serde_json::Value) {
    let line = m.get("line_number").and_then(|v| v.as_u64()).unwrap_or(0);
    let content = m.get("content").and_then(|v| v.as_str()).unwrap_or("");

    if let Some(before) = m.get("context_before").and_then(|v| v.as_array()) {
        for ctx_line in before {
            if let Some(s) = ctx_line.as_str() {
                out.push_str(&format!("    {}\n", s));
            }
        }
    }
    out.push_str(&format!("**{}:** `{}`\n", line, content.trim()));
    if let Some(after) = m.get("context_after").and_then(|v| v.as_array()) {
        for ctx_line in after {
            if let Some(s) = ctx_line.as_str() {
                out.push_str(&format!("    {}\n", s));
            }
        }
    }

    // Show captures for ast-grep results
    if let Some(captures) = m.get("captures").and_then(|v| v.as_object()) {
        if !captures.is_empty() {
            let caps: Vec<String> = captures
                .iter()
                .map(|(k, v)| format!("{}=`{}`", k, v.as_str().unwrap_or("?")))
                .collect();
            out.push_str(&format!("  Captures: {}\n", caps.join(", ")));
        }
    }
    out.push('\n');
}

fn render_symbols_md(arr: &[serde_json::Value]) -> String {
    let mut out = String::from("## Symbols\n\n| Name | Kind | Lines |\n|------|------|-------|\n");
    for sym in arr {
        if is_truncation_sentinel(sym) {
            out.push_str(&format!("\n_{}_\n", format_truncation_sentinel(sym)));
            continue;
        }
        let name = sym.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let kind = sym.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
        let ls = sym.get("line_start").and_then(|v| v.as_u64()).unwrap_or(0);
        let le = sym.get("line_end").and_then(|v| v.as_u64()).unwrap_or(0);
        let line_range = if le > ls {
            format!("{}-{}", ls, le)
        } else {
            format!("{}", ls)
        };
        out.push_str(&format!("| `{}` | {} | {} |\n", name, kind, line_range));
    }
    out
}

fn render_fs_ls_md(obj: &serde_json::Map<String, serde_json::Value>) -> String {
    let total = obj.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
    let mut out = format!(
        "## Directory Listing\n\n{} entries\n\n| Name | Type | Size |\n|------|------|------|\n",
        total
    );

    if let Some(entries) = obj.get("entries").and_then(|v| v.as_array()) {
        for entry in entries {
            if is_truncation_sentinel(entry) {
                out.push_str(&format!("\n_{}_\n", format_truncation_sentinel(entry)));
                continue;
            }
            let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("?");
            let etype = entry.get("type").and_then(|v| v.as_str()).unwrap_or("?");
            let size = entry
                .get("size")
                .and_then(|v| v.as_u64())
                .map(format_size)
                .unwrap_or_else(|| "-".to_string());
            out.push_str(&format!("| `{}` | {} | {} |\n", name, etype, size));
        }
    }
    out
}

fn render_fs_cat_md(obj: &serde_json::Map<String, serde_json::Value>) -> String {
    let path = obj.get("path").and_then(|v| v.as_str()).unwrap_or("?");
    let mut out = format!("## {}\n\n", path);
    if let Some(content) = obj.get("content").and_then(|v| v.as_str()) {
        out.push_str(&format!("```\n{}\n```\n", content));
    }
    out
}

fn render_index_build_md(obj: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut out = String::from("## Index Built\n\n");
    for (key, value) in obj {
        let label = key.replace('_', " ");
        out.push_str(&format!("- **{}:** {}\n", label, format_json_value(value)));
    }
    out
}

fn render_index_status_md(obj: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut out = String::from("## Index Status\n\n");
    for (key, value) in obj {
        let label = key.replace('_', " ");
        out.push_str(&format!("- **{}:** {}\n", label, format_json_value(value)));
    }
    out
}

fn render_edit_md(obj: &serde_json::Map<String, serde_json::Value>) -> String {
    let path = obj.get("path").and_then(|v| v.as_str()).unwrap_or("?");
    let applied = obj
        .get("applied")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let validated = obj
        .get("validated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let lines_changed = obj
        .get("lines_changed")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let format = obj.get("format").and_then(|v| v.as_str()).unwrap_or("?");

    let mut out = format!("## Edit: {}\n\n", path);
    out.push_str(&format!("- **Format:** {}\n", format));
    out.push_str(&format!("- **Applied:** {}\n", applied));
    out.push_str(&format!("- **Validated:** {}\n", validated));
    out.push_str(&format!("- **Lines changed:** {}\n", lines_changed));

    if let Some(diff) = obj.get("diff").and_then(|v| v.as_str()) {
        if !diff.is_empty() {
            out.push_str(&format!("\n```diff\n{}\n```\n", diff));
        }
    }
    out
}

fn render_search_md(obj: &serde_json::Map<String, serde_json::Value>) -> String {
    let query = obj.get("query").and_then(|v| v.as_str()).unwrap_or("?");
    let engine = obj.get("engine").and_then(|v| v.as_str()).unwrap_or("?");
    let total = obj
        .get("total_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cached = obj.get("cached").and_then(|v| v.as_bool()).unwrap_or(false);

    let cache_str = if cached { " (cached)" } else { "" };
    let mut out = format!(
        "## Search: `{}`\n\n{} results via {}{}\n\n",
        query, total, engine, cache_str
    );

    if let Some(results) = obj.get("results").and_then(|v| v.as_array()) {
        for (i, r) in results.iter().enumerate() {
            let title = r.get("title").and_then(|v| v.as_str()).unwrap_or("?");
            let url = r.get("url").and_then(|v| v.as_str()).unwrap_or("");
            let snippet = r.get("snippet").and_then(|v| v.as_str()).unwrap_or("");

            if url.is_empty() {
                out.push_str(&format!("### {}. {}\n\n", i + 1, title));
            } else {
                out.push_str(&format!("### {}. [{}]({})\n\n", i + 1, title, url));
            }
            if !snippet.is_empty() {
                out.push_str(&format!("{}\n\n", snippet));
            }
        }
    }
    out
}

fn render_fetch_md(obj: &serde_json::Map<String, serde_json::Value>) -> String {
    let url = obj.get("url").and_then(|v| v.as_str()).unwrap_or("?");
    let title = obj.get("title").and_then(|v| v.as_str()).unwrap_or("");
    let word_count = obj.get("word_count").and_then(|v| v.as_u64()).unwrap_or(0);
    let content = obj.get("content").and_then(|v| v.as_str()).unwrap_or("");

    let mut out = format!("## {}\n\n", if title.is_empty() { url } else { title });
    out.push_str(&format!(
        "**Source:** {}\n**Words:** {}\n\n",
        url, word_count
    ));
    out.push_str(content);
    out.push('\n');
    out
}

fn render_object_md(obj: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut out = String::new();
    for (key, value) in obj {
        let label = key.replace('_', " ");
        out.push_str(&format!("- **{}:** {}\n", label, format_json_value(value)));
    }
    out
}

fn render_array_md(arr: &[serde_json::Value]) -> String {
    let mut out = String::new();
    for (i, value) in arr.iter().enumerate() {
        out.push_str(&format!("{}. {}\n", i + 1, format_json_value(value)));
    }
    out
}

/// Check if a JSON value is the array truncation sentinel.
fn is_truncation_sentinel(value: &serde_json::Value) -> bool {
    value
        .get("_truncated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Format the truncation sentinel for display.
fn format_truncation_sentinel(value: &serde_json::Value) -> String {
    let remaining = value.get("remaining").and_then(|v| v.as_u64()).unwrap_or(0);
    format!("...({} more items)", remaining)
}

fn format_json_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".to_string(),
        _ => value.to_string(),
    }
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_tokens_empty() {
        assert_eq!(count_tokens(""), 0);
    }

    #[test]
    fn test_count_tokens_simple() {
        let count = count_tokens("Hello, world!");
        assert!(count > 0);
        assert!(count < 10);
    }

    #[test]
    fn test_tokenizer_is_cached() {
        // Calling count_tokens multiple times should not panic or slow down
        for _ in 0..100 {
            count_tokens("test string");
        }
    }

    #[test]
    fn test_apply_budget_to_text_within_budget() {
        let text = "Hello, world!";
        let result = apply_budget_to_text(text, 100, CompactionStrategy::HeadTail);
        assert!(!result.truncated);
        assert_eq!(result.content, text);
    }

    #[test]
    fn test_apply_budget_to_text_exceeds_budget() {
        let text = (0..100)
            .map(|i| format!("Line {}: some content here", i))
            .collect::<Vec<_>>()
            .join("\n");

        let result = apply_budget_to_text(&text, 50, CompactionStrategy::HeadTail);
        assert!(result.truncated);
        assert!(result.content.contains("lines omitted"));
    }

    #[test]
    fn test_emit_json_within_budget() {
        let data = vec!["file1.rs", "file2.rs"];
        let result = emit_json(&data, Some(100));
        assert!(!result.truncated);
        // Must be valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert!(parsed.is_array());
    }

    #[test]
    fn test_emit_json_no_budget() {
        let data = vec!["a", "b", "c"];
        let result = emit_json(&data, None);
        assert!(!result.truncated);
        let _: serde_json::Value = serde_json::from_str(&result.content).unwrap();
    }

    #[test]
    fn test_emit_json_over_budget_produces_valid_json() {
        let data: Vec<String> = (0..500).map(|i| format!("file_{}.rs", i)).collect();
        let result = emit_json(&data, Some(30));
        assert!(result.truncated);
        // CRITICAL: Must still be valid JSON
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&result.content);
        assert!(
            parsed.is_ok(),
            "truncated output must be valid JSON: {}",
            result.content
        );
    }

    #[test]
    fn test_emit_json_over_budget_object_produces_valid_json() {
        #[derive(Serialize)]
        struct BigResult {
            entries: Vec<String>,
            total: usize,
            content: String,
        }
        let big = BigResult {
            entries: (0..200).map(|i| format!("entry_{}", i)).collect(),
            total: 200,
            content: "x".repeat(5000),
        };
        let result = emit_json(&big, Some(40));
        assert!(result.truncated);
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&result.content);
        assert!(
            parsed.is_ok(),
            "truncated object must be valid JSON: {}",
            result.content
        );
    }

    #[test]
    fn test_emit_json_very_small_budget() {
        let data: Vec<String> = (0..100).map(|i| format!("item_{}", i)).collect();
        let result = emit_json(&data, Some(5));
        // Must still be valid JSON, even with extreme budget
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&result.content);
        assert!(
            parsed.is_ok(),
            "extreme budget must produce valid JSON: {}",
            result.content
        );
        // Should use skeleton or budget_exhausted, not {"t":1}
        assert!(
            !result.content.contains(r#""t":1"#),
            "should not use old {{\"t\":1}} fallback"
        );
    }

    #[test]
    fn test_extract_skeleton_keeps_scalars() {
        let value = serde_json::json!({
            "path": "src/main.rs",
            "lines": 100,
            "truncated": false,
            "content": "x".repeat(200),
            "entries": [1, 2, 3]
        });
        let skeleton = extract_skeleton(&value);
        let obj = skeleton.as_object().unwrap();
        assert_eq!(obj.get("path").unwrap(), "src/main.rs");
        assert_eq!(obj.get("lines").unwrap(), 100);
        assert_eq!(obj.get("truncated").unwrap(), true); // overwritten by skeleton marker
        assert!(
            obj.get("content").is_none(),
            "long strings should be dropped"
        );
        assert!(obj.get("entries").is_none(), "arrays should be dropped");
    }

    #[test]
    fn test_apply_budget_to_text_head_only() {
        let text = (0..100)
            .map(|i| format!("Line {}: content", i))
            .collect::<Vec<_>>()
            .join("\n");

        let result = apply_budget_to_text(&text, 30, CompactionStrategy::HeadOnly);
        assert!(result.truncated);
        assert!(result.content.contains("more lines truncated"));
    }

    #[test]
    fn test_apply_budget_to_text_rule_based_prioritizes_errors() {
        let lines = vec![
            "INFO: Starting up",
            "DEBUG: Loading config",
            "ERROR: Failed to connect to database",
            "INFO: Retrying",
            "WARN: Connection timeout",
            "DEBUG: Some debug info",
        ];
        let text = lines.join("\n");

        let result = apply_budget_to_text(&text, 20, CompactionStrategy::RuleBased);
        assert!(result.truncated);
        assert!(result.content.contains("ERROR"));
    }

    #[test]
    fn test_budgeted_output_original_tokens_preserved() {
        let text = (0..50)
            .map(|i| format!("Line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let original_count = count_tokens(&text);

        let result = apply_budget_to_text(&text, 20, CompactionStrategy::HeadOnly);
        assert!(result.truncated);
        assert_eq!(result.original_tokens, original_count);
        assert!(result.tokens < result.original_tokens);
    }

    #[test]
    fn test_reduce_value_truncates_strings() {
        let value = serde_json::json!({"content": "a".repeat(1000)});
        let reduced = reduce_value(&value, 50, 100);
        let s = reduced["content"].as_str().unwrap();
        assert!(s.len() < 200);
        assert!(s.contains("[truncated]"));
    }

    #[test]
    fn test_reduce_value_truncates_arrays() {
        let value = serde_json::json!([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        let reduced = reduce_value(&value, 1000, 3);
        let arr = reduced.as_array().unwrap();
        assert_eq!(arr.len(), 4); // 3 items + truncation sentinel
        let sentinel = &arr[3];
        assert_eq!(sentinel["_truncated"], true);
        assert_eq!(sentinel["remaining"], 7);
    }

    #[test]
    fn test_render_markdown_tree() {
        let value = serde_json::json!({
            "root": "src",
            "entries": [
                {"path": "cli", "type": "dir", "depth": 1},
                {"path": "cli/mod.rs", "type": "file", "depth": 2},
                {"path": "main.rs", "type": "file", "depth": 1}
            ],
            "total_files": 2,
            "total_dirs": 1
        });
        let md = render_markdown(&value);
        assert!(md.contains("## Tree: src"));
        assert!(md.contains("2 files, 1 directories"));
        assert!(md.contains("cli/"));
        assert!(md.contains("mod.rs"));
        assert!(md.contains("main.rs"));
    }

    #[test]
    fn test_render_markdown_code_read() {
        let value = serde_json::json!({
            "path": "src/main.rs",
            "language": "rust",
            "lines": 100,
            "symbols": [
                {"name": "main", "kind": "function", "line_start": 1, "line_end": 10}
            ],
            "content": "fn main() {\n    println!(\"hello\");\n}",
            "tokens": 20,
            "truncated": false
        });
        let md = render_markdown(&value);
        assert!(md.contains("## src/main.rs"));
        assert!(md.contains("**rust**"));
        assert!(md.contains("100 lines"));
        assert!(md.contains("| `main` | function | 1-10 |"));
        assert!(md.contains("```rust"));
        assert!(md.contains("fn main()"));
    }

    #[test]
    fn test_render_markdown_grep() {
        let value = serde_json::json!({
            "pattern": "TODO",
            "matches": [
                {
                    "file": "main.rs",
                    "line_number": 42,
                    "content": "// TODO: fix this",
                    "context_before": [],
                    "context_after": []
                }
            ],
            "total_matches": 1,
            "files_searched": 5
        });
        let md = render_markdown(&value);
        assert!(md.contains("## Grep: `TODO`"));
        assert!(md.contains("1 matches across 5 files"));
        assert!(md.contains("### main.rs"));
        assert!(md.contains("**42:** `// TODO: fix this`"));
    }

    #[test]
    fn test_render_markdown_symbols() {
        let value = serde_json::json!([
            {"name": "Config", "kind": "struct", "line_start": 10, "line_end": 20, "byte_start": 0, "byte_end": 100},
            {"name": "new", "kind": "function", "line_start": 22, "line_end": 30, "byte_start": 0, "byte_end": 100}
        ]);
        let md = render_markdown(&value);
        assert!(md.contains("## Symbols"));
        assert!(md.contains("| `Config` | struct | 10-20 |"));
        assert!(md.contains("| `new` | function | 22-30 |"));
    }

    #[test]
    fn test_render_markdown_fs_ls() {
        let value = serde_json::json!({
            "entries": [
                {"name": "src", "path": "src", "type": "dir", "size": 96},
                {"name": "main.rs", "path": "main.rs", "type": "file", "size": 1500}
            ],
            "total": 2
        });
        let md = render_markdown(&value);
        assert!(md.contains("## Directory Listing"));
        assert!(md.contains("2 entries"));
        assert!(md.contains("| `src` | dir |"));
        assert!(md.contains("| `main.rs` | file |"));
    }

    #[test]
    fn test_render_markdown_edit() {
        let value = serde_json::json!({
            "path": "file.rs",
            "format": "search-replace",
            "applied": true,
            "validated": true,
            "diff": "- old\n+ new",
            "lines_changed": 1
        });
        let md = render_markdown(&value);
        assert!(md.contains("## Edit: file.rs"));
        assert!(md.contains("**Applied:** true"));
        assert!(md.contains("```diff"));
        assert!(md.contains("- old"));
    }

    #[test]
    fn test_render_markdown_index_build() {
        let value = serde_json::json!({
            "files_indexed": 19,
            "files_skipped": 0,
            "symbols_added": 429,
            "refs_added": 780
        });
        let md = render_markdown(&value);
        assert!(md.contains("## Index Built"));
        assert!(md.contains("**files indexed:** 19"));
        assert!(md.contains("**symbols added:** 429"));
    }

    #[test]
    fn test_render_markdown_index_status() {
        let value = serde_json::json!({
            "path": ".anvil/index.db",
            "total_files": 19,
            "total_symbols": 429,
            "total_refs": 780,
            "stale_files": 0,
            "schema_version": "1"
        });
        let md = render_markdown(&value);
        assert!(md.contains("## Index Status"));
        assert!(md.contains("**total symbols:** 429"));
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1_572_864), "1.5 MB");
    }

    #[test]
    fn test_compact_json_value_tree_strips_depth() {
        let value = serde_json::json!({
            "root": "src",
            "entries": [
                {"path": "main.rs", "type": "file", "depth": 1},
                {"path": "lib.rs", "type": "file", "depth": 1}
            ],
            "total_files": 2,
            "total_dirs": 0
        });
        let compacted = compact_json_value(&value);
        let entries = compacted["entries"].as_array().unwrap();
        for entry in entries {
            assert!(
                entry.get("depth").is_none(),
                "depth should be stripped in compact"
            );
            assert!(entry.get("path").is_some(), "path should be preserved");
        }
    }

    #[test]
    fn test_compact_json_value_symbols_strips_bytes() {
        let value = serde_json::json!([
            {"name": "Config", "kind": "struct", "line_start": 10, "line_end": 20, "byte_start": 0, "byte_end": 100}
        ]);
        let compacted = compact_json_value(&value);
        let arr = compacted.as_array().unwrap();
        let sym = &arr[0];
        assert!(
            sym.get("byte_start").is_none(),
            "byte_start should be stripped"
        );
        assert!(sym.get("byte_end").is_none(), "byte_end should be stripped");
        assert!(sym.get("name").is_some(), "name should be preserved");
    }

    #[test]
    fn test_compact_json_value_grep_strips_context() {
        let value = serde_json::json!({
            "pattern": "TODO",
            "matches": [
                {
                    "file": "main.rs",
                    "line_number": 42,
                    "content": "// TODO: fix",
                    "context_before": ["line before"],
                    "context_after": ["line after"],
                    "captures": {}
                }
            ],
            "total_matches": 1,
            "files_searched": 5
        });
        let compacted = compact_json_value(&value);
        let m = &compacted["matches"][0];
        assert!(
            m.get("context_before").is_none(),
            "context_before should be stripped"
        );
        assert!(
            m.get("context_after").is_none(),
            "context_after should be stripped"
        );
        assert!(
            m.get("captures").is_none(),
            "empty captures should be stripped"
        );
        assert!(m.get("file").is_some(), "file should be preserved");
    }

    #[test]
    fn test_render_raw_tree() {
        let value = serde_json::json!({
            "root": "src",
            "entries": [
                {"path": "cli", "type": "dir", "depth": 1},
                {"path": "cli/mod.rs", "type": "file", "depth": 2},
                {"path": "main.rs", "type": "file", "depth": 1}
            ],
            "total_files": 2,
            "total_dirs": 1
        });
        let raw = render_raw(&value);
        assert!(raw.contains("cli/"));
        assert!(raw.contains("cli/mod.rs"));
        assert!(raw.contains("main.rs"));
        // Should not contain JSON characters
        assert!(!raw.contains('{'));
    }

    #[test]
    fn test_render_raw_symbols() {
        let value = serde_json::json!([
            {"name": "Config", "kind": "struct", "line_start": 10, "line_end": 20, "byte_start": 0, "byte_end": 100},
            {"name": "new", "kind": "function", "line_start": 22, "line_end": 30, "byte_start": 0, "byte_end": 100}
        ]);
        let raw = render_raw(&value);
        assert!(raw.contains("Config (struct) L:10-20"));
        assert!(raw.contains("new (function) L:22-30"));
    }

    #[test]
    fn test_render_raw_grep() {
        let value = serde_json::json!({
            "pattern": "TODO",
            "matches": [
                {
                    "file": "main.rs",
                    "line_number": 42,
                    "content": "// TODO: fix this",
                    "context_before": [],
                    "context_after": []
                }
            ],
            "total_matches": 1,
            "files_searched": 5
        });
        let raw = render_raw(&value);
        assert!(raw.contains("main.rs:42: // TODO: fix this"));
    }

    #[test]
    fn test_render_raw_fs_ls() {
        let value = serde_json::json!({
            "entries": [
                {"name": "src", "path": "src", "type": "dir", "size": 96},
                {"name": "main.rs", "path": "main.rs", "type": "file", "size": 1500}
            ],
            "total": 2
        });
        let raw = render_raw(&value);
        assert!(raw.contains("src\tdir\t96"));
        assert!(raw.contains("main.rs\tfile\t1500"));
    }

    #[test]
    fn test_render_raw_code_read() {
        let value = serde_json::json!({
            "path": "src/main.rs",
            "language": "rust",
            "lines": 10,
            "symbols": [],
            "content": "fn main() {\n    println!(\"hello\");\n}",
            "tokens": 20,
            "truncated": false
        });
        let raw = render_raw(&value);
        assert_eq!(raw, "fn main() {\n    println!(\"hello\");\n}");
    }

    #[test]
    fn test_render_raw_index() {
        let value = serde_json::json!({
            "files_indexed": 19,
            "files_skipped": 0,
            "symbols_added": 429,
            "refs_added": 780
        });
        let raw = render_raw(&value);
        assert!(raw.contains("files_indexed: 19"));
        assert!(raw.contains("symbols_added: 429"));
    }
}
