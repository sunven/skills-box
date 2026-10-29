# The read model owns the catalog and per-skill views

Status: accepted

`inventory`, `detail` and `content` each rebuilt the lock and agent-directory catalog per request; `detail` re-projected one skill, `content` resolved a root through detail, and `InventorySkill.update` / `SkillDetail.update` always serialized `unknown` while the UI overlaid a separate update report. We introduce a top-level `ReadModel` that owns a cached catalog, one `SkillView` per skill, and root resolution. The catalog (lock entries plus canonical/agent directory topology and attachments) is cached; `get_inventory` always rebuilds it and writes it back, while `detail` and `resolve_root` reuse it until a fingerprint of the lock, `agents.json`, canonical directory and agent directories changes. Per-skill views read `SKILL.md` and the baseline fresh on every call, so local-modification detection never goes stale. `SKILL.md` is read through detail's strict reader (1 MiB cap, symlink containment, UTF-8). `content` functions become root-relative; commands keep the 30s timeout and blocking-pool policy. The dead `update` field is removed from both Rust read models and the TypeScript types; `applyUpdateReport` derives update state solely from the report. `EnvironmentService` and agent detection remain separate, and baseline write-in-read remains for a later decision.

## Considered options

- **Full snapshot cache**: rejected because local modification and `SKILL.md` content would go stale.
- **No cross-request cache**: rejected because detail and content would keep rebuilding the catalog; it preserves freshness but not the repeated work.
- **Cache only the catalog** (chosen): views stay fresh, content/detail root lookups become cheap.
- **Populate `update` in Rust from a persisted report**: rejected because it couples the read model to the network update pipeline.
- **Merge detail into inventory and remove `get_skill_detail`**: rejected because it would enlarge the inventory payload and change the on-demand detail contract.
- **Move agent detection into `EnvironmentService`**: rejected for scope; environment unification is a separate decision.

## Consequences

- `AppState` holds one `Arc<ReadModel>`; read commands go through it. `get_environment(refresh=true)` invalidates the catalog.
- `get_inventory` is the catalog refresh point; every mutation already invalidates the frontend inventory query.
- `TaskManager` and native actions are not coupled to the read model; mutation visibility comes from the inventory refresh and the catalog fingerprint.
- `inventory::read` and `detail::read` remain one-shot entry points for tests and integration fixtures; production reads use the shared `ReadModel`.
- `content` is root-relative and testable with a temporary root; copy-only ambiguity and path security live in `ReadModel::resolve_root`.
- The `update` field disappears from the read-model contracts; the frontend `UpdateViewSkill` type owns it.
- Detail still uses the strict `SKILL.md` reader; inventory now shares that behavior, so oversized or escaping `SKILL.md` files show a warning instead of a description.
