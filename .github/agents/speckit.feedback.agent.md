---
description: Extract lessons learned from PR review comments and discussions, storing them in per-PR lesson files and a central lessons database.
handoffs:
  - label: Specify Feature
    agent: speckit.specify
    prompt: Create a new feature specification
  - label: Plan Feature
    agent: speckit.plan
    prompt: Create a technical plan for the specification
---

## User Input

```text
$ARGUMENTS
```

You **MUST** consider the user input before proceeding (if not empty).

## Outline

Goal: Capture lessons learned from PR feedback and store them for future reference by other speckit phases.

## Execution Steps

### Step 1: Parse Arguments

Analyze the `$ARGUMENTS` input to determine the operation mode:

**Mode A - Specific PR**: If `--pr <number>` is present, use that PR number.

**Mode B - Auto-detect PR**: If no `--pr` flag but no inline text, find the most recently merged PR for the current branch.

**Mode C - Manual Entry**: If inline text is provided (not a flag), treat it as a manual lesson entry.

Parse these optional flags:
- `--pr <number>`: Specific PR to extract feedback from
- `--category <category>`: Category for manual lesson (code-quality, architecture, testing, documentation, security, performance, other)

### Step 2: Ensure Directory Structure

Check if `docs/feedback/` directory exists. If not, create it with:
- `docs/feedback/lessons.md` - Central lessons database (use template from data-model)
- `docs/feedback/AGENTS.md` - Agent instructions for GitHub Copilot
- `docs/feedback/CLAUDE.md` - Agent instructions for Claude Code

### Step 3: Execute Based on Mode

#### Mode A/B: PR Feedback Extraction

1. **Get PR Number**:
   - Mode A: Use the `--pr <number>` value
   - Mode B: Run `.specify/scripts/bash/fetch-pr-feedback.sh --find-recent` to get the most recently merged PR

2. **Check for Duplicate PR Processing**:
   - If `docs/feedback/pr-<number>-lessons.md` exists, ask user: "PR #<number> has already been processed. Overwrite existing lessons file? (yes/no)"
   - If no, skip to database update step or exit

3. **Fetch PR Data**:
   - Run `.specify/scripts/bash/fetch-pr-feedback.sh <pr_number>`
   - Parse the JSON output containing:
     - PR metadata (title, body, author, URL)
     - Review comments and suggestions
     - Discussion thread comments

4. **Handle Edge Cases**:
   - If no PR found: Prompt user "No merged PR found for current branch. Enter a PR number or type a lesson for manual entry."
   - If PR has no review comments: Report "No review feedback found in PR #<number>. Would you like to add a manual lesson instead?"

5. **Extract and Categorize Lessons**:
   - Analyze review comments, suggestions, and PR description
   - For each substantive feedback item, create a lesson:
     - Determine the most appropriate category (code-quality, architecture, testing, documentation, security, performance, other)
     - Extract the core lesson content
     - Add relevant tags based on file paths, technologies mentioned
   - Limit to top 10 lessons if PR discussion is very long (summarize themes)

6. **Generate PR Lessons File**:
   - Create `docs/feedback/pr-<number>-lessons.md` with YAML frontmatter:
     ```yaml
     ---
     pr_number: <number>
     pr_title: "<title>"
     pr_url: "<url>"
     extracted_date: <today's date YYYY-MM-DD>
     lesson_count: <count>
     ---
     ```
   - Add each lesson in the format from data-model.md

7. **Update Central Database** (proceed to Step 4)

#### Mode C: Manual Lesson Entry

1. **Get Lesson Content**: Extract the inline text from `$ARGUMENTS`

2. **Determine Category**:
   - If `--category <category>` provided, use that category
   - Otherwise, prompt user: "Select a category for this lesson: 1) code-quality, 2) architecture, 3) testing, 4) documentation, 5) security, 6) performance, 7) other"

3. **Create Lesson Object**:
   ```yaml
   lesson_id: <next available L### ID>
   category: <selected category>
   tags: []
   source: manual
   source_ref: "Manual entry <today's date>"
   date: <today's date YYYY-MM-DD>
   frequency: 1
   ```

4. **Proceed to Step 4** to add to database

### Step 4: Update Central Lessons Database

1. **Read Existing Database**:
   - Load `docs/feedback/lessons.md`
   - Parse YAML frontmatter to get `total_lessons` and `last_updated`
   - Parse existing lesson entries to build lesson ID list

2. **Duplicate Detection**:
   For each new lesson:
   - Compare content with existing lessons (fuzzy match - look for similar phrasing, same core concept)
   - If duplicate found (>80% similarity in meaning):
     - Increment the `frequency` count on the existing lesson
     - Do NOT add as new entry
   - If not duplicate:
     - Generate next lesson_id (L001, L002, etc. based on existing IDs)
     - Append to database

3. **Update Database Metadata**:
   - Update `last_updated` to today's date
   - Update `total_lessons` count

4. **Write Updated Database**:
   - Preserve the header and category documentation
   - Append new lessons below the `<!-- Lessons are appended below this line -->` marker

### Step 5: Report Summary

Output a summary to the user:

**For PR Extraction:**
```
✅ Extracted <N> lessons from PR #<number> "<title>"
   - <count> code-quality lessons
   - <count> architecture lessons
   - <count> testing lessons
   - ... (other categories with lessons)

📝 Created: docs/feedback/pr-<number>-lessons.md
📊 Updated: docs/feedback/lessons.md (now contains <total> total lessons)

<If any duplicates found>
ℹ️ <M> lessons matched existing entries (frequency incremented)
```

**For Manual Entry:**
```
✅ Added manual lesson to database
   Category: <category>

📊 Updated: docs/feedback/lessons.md (now contains <total> total lessons)
```

## Lesson Entry Format

Each lesson in the database follows this format:

```markdown
---
lesson_id: L###
category: <category>
tags: [tag1, tag2]
source: pr | manual
source_ref: "PR #123" | "Manual entry YYYY-MM-DD"
date: YYYY-MM-DD
frequency: 1
---

<Lesson content in plain text or markdown>
```

## Categories Reference

| Category | Use For |
|----------|---------|
| code-quality | Code style, idioms, language-specific best practices |
| architecture | System design, patterns, module structure |
| testing | Test strategies, coverage, edge case handling |
| documentation | Comments, READMEs, API documentation |
| security | Authentication, authorization, input validation, secrets handling |
| performance | Optimization, efficiency, resource usage |
| other | Miscellaneous lessons not fitting other categories |

## Error Handling

- **No `gh` CLI**: "GitHub CLI (gh) is required. Install with: brew install gh"
- **Not authenticated**: "Please authenticate with GitHub: gh auth login"
- **Not in git repo**: "This command must be run from within a git repository"
- **PR not found**: "PR #<number> not found. Please verify the PR number exists."
- **Network error**: "Failed to fetch PR data. Check your network connection and try again."

## Stop Conditions

- Successfully created/updated lesson files
- User cancels duplicate overwrite prompt
- Fatal error (missing dependencies, authentication failure)
