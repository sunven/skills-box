# Native plugin state belongs to the hosting Agent

Status: accepted

Skills Box manages Codex and Claude Code Plugins as complete native extensions, including Plugins installed outside Skills Box. The hosting Agent's installation records and configuration remain authoritative; Skills Box does not create a competing Plugin installation ledger or convert these installations into managed Skills or Expert packs.

Using the existing Skill installation records would make a Plugin appear to share the Canonical store, attachment and update lifecycle of independent Skills, and would split ownership with the native tools that also manage it. Keeping native ownership preserves interoperability with those tools while requiring Skills Box to reflect their distinct capabilities and observed state. This is a separate domain boundary from the app-owned Skill marketplace installations in ADR-0030.

The ownership decision was confirmed as Q5, together with the native Plugin boundary in Q1, during the 2026-11-19 [design interview](../../.scratch/native-plugin-management/spec.md). The initial product scope was user-level management; the subsequently confirmed execution and native-effect boundaries are recorded in [ADR-0038](0038-native-plugin-execution-boundary.md). [ADR-0040](0040-native-plugin-project-context.md) extends the product design to registered Projects and native configuration layers while retaining this ownership boundary.
