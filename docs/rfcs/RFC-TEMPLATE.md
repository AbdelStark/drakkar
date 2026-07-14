<!--
Copy this file to docs/rfcs/RFC-NNNN-short-title.md and fill it in. Keep the
section order; it matches the shipped RFCs (RFC-0010 through RFC-0012).

Mint a per-document requirement prefix for any normative statement a reasonable
engineer could implement differently (see SPEC.md "Conventions"), e.g. `XY1`,
`XY2`, and cite locked decisions as `LDn`. Implementation PRs cite these IDs.
-->

# RFC-NNNN: <Title>

- Status: Draft <!-- Draft | Proposed | Accepted | Superseded by RFC-XXXX -->
- Authors: <github-handle>
- Created: <YYYY-MM-DD>
- Target milestone: <v0.1 | v0.2 | v0.3 | v1.0>

## Summary

One paragraph: what this decides and why it matters.

## Motivation

The problem, the forces at play, and why a decision record (not just code) is
warranted. Link to the PRD or spec sections this serves.

## Goals

- What this RFC commits to achieving.

## Non-Goals

- What is explicitly out of scope, so reviewers do not expect it.

## Proposed Design

The normative content. Assign requirement IDs (`XY1`, `XY2`, …) to each
load-bearing statement; use RFC 2119 keywords (MUST, SHOULD, MAY). Reference the
architecture invariants I1–I5
([../spec/01-architecture.md §10](../spec/01-architecture.md#10-invariants-the-review-contract))
where they apply.

## Alternatives Considered

Each alternative with a one-line verdict on why it was rejected. A scoring
matrix is welcome for multi-way choices.

## Drawbacks

The costs and risks of the proposed design, stated honestly.

## Migration / Rollout

How this ships across milestones, what changes for existing consumers, and any
deprecation window.

## Testing Strategy

The named test classes (unit, property, golden-fixture, conformance, soak) that
verify each requirement, and the CI gate that runs them.

## Open Questions

| ID | Question | Owner | Resolution path |
| --- | --- | --- | --- |
| OQ-XY-1 | … | <handle> | … |

## References

- Links to related RFCs, spec sections, PRD sections, and external sources.
