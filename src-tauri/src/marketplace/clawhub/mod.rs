//! Anonymous ClawHub catalog reads. ClawHub is the third Marketplace source;
//! installations (tickets 07+) use versioned registry records (ADR-0034).
//!
//! `/api/v1/skills` exposes no `install.kind`, so a list item cannot be
//! classified as a skills.sh alias. Per spec R7-Q4 such entries are shown
//! normally here; ticket 06 identifies aliases at detail time and redirects to
//! the skills.sh source. Do not call the per-item install resolver while
//! listing, and do not filter list entries by guessing.
mod client;
mod list;

pub use list::{Listing, Metrics, Skill, Sort, list};
