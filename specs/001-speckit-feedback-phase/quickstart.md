# Quickstart: /speckit.feedback

**Feature**: 001-speckit-feedback-phase  
**Date**: 2026-01-21  
**Status**: ✅ Implemented

## Overview

The `/speckit.feedback` command captures lessons learned from PR reviews and stores them for future reference by other speckit phases.

## Prerequisites

- VS Code with GitHub Copilot Chat extension
- GitHub CLI (`gh`) installed and authenticated
- Git repository with GitHub remote configured

## Basic Usage

### Capture Feedback from Current Branch's PR

```
/speckit.feedback
```

This will:
1. Detect the current branch
2. Find the open PR for that branch (or most recently merged if none open)
3. Extract review comments and PR description
4. Categorize lessons and save to `docs/feedback/pr-<number>-lessons.md`
5. Update the central `docs/feedback/lessons.md` database
6. Prompt you to commit the changes to your branch

### Capture Feedback from Specific PR

```
/speckit.feedback --pr 123
```

Explicitly specify which PR to extract feedback from.

### Add Manual Lesson

```
/speckit.feedback "Always validate user input before database operations"
```

Add a lesson directly without PR context. The lesson will be added to the central database with a "manual" source tag.

### Add Manual Lesson with Category

```
/speckit.feedback --category security "Sanitize all user inputs"
```

Add a manual lesson with explicit category assignment.

## Output Files

| File | Description |
|------|-------------|
| `docs/feedback/pr-<number>-lessons.md` | Lessons from a specific PR |
| `docs/feedback/lessons.md` | Central database of all lessons |

## Categories

Lessons are categorized into one of:

- **code-quality**: Code style, idioms, best practices
- **architecture**: System design, patterns, structure
- **testing**: Test strategies, coverage, edge cases
- **documentation**: Comments, READMEs, API docs
- **security**: Auth, input validation, secrets
- **performance**: Optimization, efficiency
- **other**: Miscellaneous

## Integration with Other Phases

The lessons database is automatically consulted by:

- `/speckit.specify` - Avoid repeating past specification mistakes
- `/speckit.plan` - Incorporate known patterns and anti-patterns
- `/speckit.implement` - Follow established code quality lessons

## Examples

### Before Merging a Feature PR

After reviews are complete but before merging:

```
$ git checkout feature/add-rest-api
$ /speckit.feedback

✅ Extracted 5 lessons from PR #142 "Add REST API endpoints"
   - 2 code-quality lessons
   - 2 documentation lessons  
   - 1 testing lesson

📝 Created: docs/feedback/pr-142-lessons.md
📊 Updated: docs/feedback/lessons.md (now contains 47 total lessons)

📝 Ready to commit! Run:
   git add docs/feedback/
   git commit -m "chore(feedback): capture lessons from PR #142"
   git push

Then merge your PR as usual - lessons will be included!
```

### Running Again After New Comments

If reviewers add more comments after your first feedback run:

```
$ /speckit.feedback

✅ Merged 2 new lessons into PR #142 lessons
   - 5 existing lessons preserved
   - 2 new lessons added
   - 1 duplicate skipped

📝 Updated: docs/feedback/pr-142-lessons.md (now contains 7 lessons)
📊 Updated: docs/feedback/lessons.md (now contains 49 total lessons)
```

### Adding Manual Lessons Before Merge

You can also add manual lessons alongside PR-extracted ones:

```
$ /speckit.feedback --category architecture "Consider using the repository pattern for data access"

✅ Added manual lesson to database
📊 Updated: docs/feedback/lessons.md (now contains 50 total lessons)
```

### Quick Manual Entry After Pair Session

```
/speckit.feedback --category architecture "Prefer composition over inheritance for service dependencies"

✅ Added manual lesson to database
📊 Updated: docs/feedback/lessons.md (now contains 48 total lessons)
```
