# Native plugin operations use verified official interfaces

Status: accepted

Skills Box uses the hosting Agent's official CLI for Plugin and Plugin marketplace operations, and first verifies Codex's official configuration write interface for Plugin enablement. The reference application's direct TOML editing is not inherited as a fallback: if an operation cannot be performed reliably through the selected official interface, that operation remains unavailable with a specific explanation.

Direct configuration editing could bypass CLI gaps, but would make Skills Box maintain a second interpretation of native configuration precedence and write semantics. Official execution retains the Agent's behavior while accepting capability gaps and version-specific verification. Compatibility therefore uses verified operation contracts and observed capabilities rather than copying the reference application's exact-version lock; recognizable command names or parseable responses alone do not prove mutation compatibility.

Native side effects remain explicit product behavior: market synchronization is separate from rereading state, Codex reinstall is not presented as a guaranteed upgrade, and market removal requires checking its actual impact across installation scopes even though Skills Box only manages user-level state. An affected project installation or an indeterminate impact blocks removal. These choices were confirmed as Q10–Q14 in the 2026-11-19 [design interview](../../.scratch/native-plugin-management/spec.md), extending [ADR-0037](0037-native-plugin-state-ownership.md) without changing the existing Skill mutation executor.
