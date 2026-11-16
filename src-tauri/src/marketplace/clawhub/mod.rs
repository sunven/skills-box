//! Anonymous ClawHub catalog reads. ClawHub is the third Marketplace source;
//! installations (tickets 07+) use versioned registry records (ADR-0034).
//!
//! `/api/v1/skills` exposes no `install.kind`, so a list item cannot be
//! classified as a skills.sh alias. Per spec R7-Q4 such entries are shown
//! normally here; ticket 06 identifies aliases at detail time and redirects to
//! the skills.sh source. Do not call the per-item install resolver while
//! listing, and do not filter list entries by guessing. ADR-0034 records the
//! same search-only exclusion / list-time fallback split.
//!
//! `/api/v1/search` does expose `install.kind`. Live samples (2026-11-16)
//! returned only skills: native rows carried `install.kind: "clawhub"`,
//! `source: "clawhub"` and a `native.skill` record; indexed skills.sh rows
//! carried `install.kind: "skills-sh"`, `source: "skills-sh"`, a
//! `sourceIdentity` and `native: null`. Search is therefore filtered on that
//! kind without resolver calls; any other kind is excluded and counted as a
//! non-skill/unsupported entity. A row with a missing or invalid kind never
//! fails the whole search: it is kept when it carries `native.skill`, and
//! counted as filtered otherwise.
mod client;
mod detail;
mod list;
mod search;

pub use detail::{
    AliasDetail, Detail, NativeDetail, ResolvedReference, VersionFiles, VersionRef, detail,
    read_file, version_files,
};
pub use list::{Listing, Metrics, Skill, Sort, list};
pub use search::{Search, search};
