# 01: Source-aware marketplace identity and native-source classification

**What to build:** The marketplace identity model represents ClawHub as a third source without forking records or lifecycle rules. Existing SkillHub installations keep their behavior and wire format. New ClawHub records carry source identity, canonical listing URL, the resolved installation reference (semver version or commit) and optional diagnostics, and every read/update path asks the record's source instead of assuming SkillHub. ClawHub is classified as a native marketplace source so uninstall, stale-record cleanup, restore blocking, copy-to-project blocking and batch-update guidance follow the same rules as SkillHub.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] Existing SkillHub market records load, display, update-check, uninstall and clean up identically; old records remain readable without migration.
- [ ] New ClawHub records round-trip source, canonical URL, resolved reference and optional diagnostics; corrupt or unknown schema states are rejected without overwriting valid records.
- [ ] The read model reports the recorded market version/reference for both sources.
- [ ] Update-check dispatch selects the provider for each record's source; a ClawHub record is never queried as SkillHub and vice versa.
- [ ] Native-source classification includes ClawHub in uninstall, stale cleanup, restore blocking, copy-to-project blocking and batch-update guidance.
- [ ] Tests cover backward compatibility, source separation, invalid records and native-source behavior.
