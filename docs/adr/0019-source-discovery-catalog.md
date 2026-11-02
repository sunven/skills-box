# Source discovery owns an application-wide source catalog

Status: accepted

Source discovery follows the default branches of sources associated with existing Global or registered Project installation records. Its Source catalog is shared across scopes, and a Discovered skill is identified by normalized source and case-insensitive skill name. Installed Skill identity remains `(scope, name)` as defined by ADR 0011; installation state and conflicts belong to the selected Scope, and Update checks continue to use their recorded refs.

A scope-owned catalog would repeat source reads and fragment discovery history when the same source is used by several Projects. A path-based discovery identity would report folder moves as new skills and distinguish same-name entries that the current Install selector cannot reliably select separately. Name-based identity instead treats folder moves as the same discovery and a name change as a new item. Different sources can still publish distinct discoveries with the same name.

This adds an upstream discovery view without merging installed inventories or weakening Project access revocation. Discovery history and acknowledgements survive source inactivity; reminder counts are shared across scopes, and ignored discoveries are restored as acknowledged. Live installation records and Project revisions determine source eligibility, so old catalog state cannot restore access to a forgotten Project. The complete design was confirmed on 2026-11-02.


The catalog contents come from the shared pinned CLI read-only preview seam.
Native authenticated GitHub commit queries and HTTPS Git HEAD queries decide
when to reuse a verified catalog. Catalog publication checks the active source
generation again and merges the current user state; it never publishes the
read/ignore decisions captured when a scan started. This auxiliary store does
not grant filesystem authority or change installation locks.
