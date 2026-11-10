# Skill marketplaces are independent of Git source discovery

Status: accepted

The Skill marketplace offers skills before the user has installed anything from their Marketplace source. Source discovery remains the inspection of Git sources associated with existing installations, with the catalog and notification semantics defined by ADR-0019. We keep separate marketplace and discovery entries and catalogs, while skills installed through either flow enter the same scope-specific managed inventory. SkillHub is the first Marketplace source; the marketplace concept is not identified with that service.

Merging marketplace results into the Discovery catalog would give public market entries installation-dependent eligibility and unread/ignore semantics that belong to watched Git sources. Keeping those boundaries separate preserves their identities and lifecycle as more Marketplace sources are introduced. Marketplace reads remain available when the SkillHub CLI is missing; dependency failures belong to the affected source and operation rather than disabling Git installation globally.

These product boundaries were confirmed during the 2026-11-10 design interview. Marketplace version policy and mutation execution are described separately in [ADR-0030](0030-marketplace-installation-records-and-execution.md) and the [marketplace specification](../../.scratch/skill-marketplace/spec.md).
