---
name: design-code
description: Generate or translate accessible, token-driven UI components and screens for Tailwind, SwiftUI, Jetpack Compose, and other supported UI frameworks. Use when a UI request needs shared design tokens or framework-specific production code; defer platform API correctness to the relevant native skill.
---

# Design Code

Use the vendored upstream design-code skill without changing its design or output rules.

1. Read [the upstream skill](../../vendor/ux-ui-agent-skills/.claude/skills/design-code/SKILL.md) completely before acting.
2. Resolve the upstream root-relative resource paths against `../../vendor/ux-ui-agent-skills/`. This includes `frameworks/`, `tokens/`, `components/`, `accessibility/`, `workflows/`, `scripts/`, and `taste/`.
3. Treat the upstream `invocation` frontmatter field as packaging metadata only. It does not change the workflow.
4. For Android platform APIs, combine the upstream token and rendering rules with the relevant Google Android skill. Project architecture and platform-specific skills take precedence when an adapter example conflicts with them.
5. Do not edit files below `../../vendor/ux-ui-agent-skills/`; update them only by replacing the vendored upstream snapshot.
