# Marketplace catalog identity does not replace Git installation identity

Status: accepted

skills.sh catalogs skills whose installable upstream is a GitHub repository. For these entries, Skills Box treats skills.sh as the Marketplace source used for browsing while retaining the Git source as the installation Source. Installing through the marketplace and installing the same skill directly from the same repository therefore produce the same scope-specific installation identity, with the existing Git update and Source discovery behavior. Market details identify both the catalog service and the actual repository.

Creating separate skills.sh installation records would split one upstream skill into competing identities, duplicate existing Git lifecycle rules, and imply a marketplace release policy that has not been established. The complete skills.sh catalog remains independent of the installation-dependent Source catalog in ADR-0029; only the repository associated with an actual Git installation becomes eligible for Source discovery under ADR-0019.

Marketplace snapshots provide browsing content, not installation versions. A skills.sh installation retains the existing Git default-branch behavior rather than pinning the snapshot or a commit; confirmation states that installed content can differ from the displayed snapshot. Missing snapshot content alone does not block installation when the Git preview uniquely resolves the selected skill. This preserves one Git lifecycle while accepting that browsing and installation content are not guaranteed to match.

This clarifies the scope of [ADR-0030](0030-marketplace-installation-records-and-execution.md): its app-owned release records and platform-version comparisons remain necessary for SkillHub-distributed releases, but are not imposed on a marketplace that directs installation to a Git source. The identity decision was confirmed as Q2, and the content and selection boundaries as Q8–Q10, during the 2026-11-14 [skills.sh design interview](../../.scratch/skills-sh-marketplace/spec.md). The user confirmed the complete specification on the same date; all product questions are resolved.
