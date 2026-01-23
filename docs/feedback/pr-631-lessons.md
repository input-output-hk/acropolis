---
pr_number: 631
pr_title: "feat: Add /speckit.feedback phase for capturing PR lessons learned"
pr_url: "https://github.com/input-output-hk/acropolis/pull/631"
extracted_date: 2026-01-22
lesson_count: 5
---

# Lessons from PR #631: Add /speckit.feedback phase for capturing PR lessons learned

## Lessons Extracted

### L005 - Match Implementation Format to Data Model Spec

```yaml
category: documentation
tags: [data-model, consistency, format]
```

When implementing file formats, ensure the actual output matches the examples in the data-model specification. All lesson files should use fenced YAML code blocks for metadata to maintain consistency.


### L006 - Avoid Ambiguous YAML Separators

```yaml
category: documentation
tags: [yaml, parsing, format]
```

Avoid using `---` separators between lessons as it creates ambiguous YAML parsing. Use blank lines between lessons and fenced YAML code blocks for metadata within each lesson.


### L007 - Check CLI Dependencies Before Use

```yaml
category: code-quality
tags: [bash, dependencies, validation]
```

When a script depends on external CLI tools (like `jq`, `gh`, etc.), always verify they are installed before attempting to use them. Add checks like: `command -v jq >/dev/null 2>&1 || error "jq is required but not installed."`


### L008 - Keep Specs in Sync with Implementation

```yaml
category: documentation
tags: [specification, implementation, consistency]
```

When implementation behavior changes (e.g., from "never updated" to "incremental updates"), update the corresponding specification documents (data-model.md, spec.md) to reflect the new behavior. Spec-implementation drift causes confusion.


### L009 - Escape Values in Shell Eval Statements

```yaml
category: security
tags: [bash, command-injection, eval]
```

When using `eval $(function_that_outputs_assignments)`, ensure all interpolated values are escaped to prevent command injection. A malicious branch name or environment variable containing single quotes can break out of assignments and execute arbitrary commands. Either escape single quotes in values or avoid `eval` entirely by sourcing a generated file.
