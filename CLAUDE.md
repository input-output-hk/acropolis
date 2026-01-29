# Claude Code Instructions for Acropolis

## Lessons Learned Integration

When working on speckit phases (specify, plan, implement), incorporate lessons learned from past PR reviews.

### Before Starting Work

1. Check if `docs/feedback/lessons.md` exists
2. If it exists, read and filter lessons by relevance to current task:
   - **For specifications**: prioritize architecture, documentation lessons
   - **For plans**: prioritize architecture, testing, performance lessons
   - **For implementation**: prioritize code-quality, security, testing lessons
3. Apply relevant lessons to avoid repeating past mistakes
4. Reference applied lessons in output where appropriate (e.g., "Per L007: checking CLI dependencies")

### Purpose

The lessons database at `docs/feedback/lessons.md` contains accumulated insights from PR reviews. Using it helps:
- Avoid repeating past mistakes
- Apply proven patterns
- Maintain code quality standards

### Lesson Categories

| Category | When to Apply |
|----------|---------------|
| code-quality | Implementation, code reviews |
| architecture | Specifications, plans |
| testing | Plans, implementation |
| documentation | Specifications, documentation tasks |
| security | Implementation, security-sensitive features |
| performance | Plans, optimization tasks |

## Speckit Workflow

See `.claude/skills/speckit/SKILL.md` for complete speckit workflow documentation.
