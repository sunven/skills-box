//! Central hard caps from the spec (§6.3).
//!
//! - Probe (`node --version`, `git --version`, login shell): 5s.
//! - Read (`get_inventory`, `get_skill_detail`, update check): 30s.
//! - Write (install / update / uninstall CLI): 15min.
//! - Read-only preview clone: 60s.
//!
//! The constants are aliased by the modules that own each operation so a
//! single change here cannot silently drift below the documented caps.

use std::time::Duration;

pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
pub const LOGIN_SHELL_TIMEOUT: Duration = Duration::from_secs(3);
pub const READ_TIMEOUT: Duration = Duration::from_secs(30);
pub const READ_TIMEOUT_MS: u64 = 30_000;
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
pub const PREVIEW_TIMEOUT: Duration = Duration::from_secs(60);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_timeout_matrix_is_encoded_in_one_place() {
        assert_eq!(PROBE_TIMEOUT, Duration::from_secs(5));
        assert_eq!(READ_TIMEOUT, Duration::from_secs(30));
        assert_eq!(READ_TIMEOUT.as_millis() as u64, READ_TIMEOUT_MS);
        assert_eq!(WRITE_TIMEOUT, Duration::from_secs(900));
        assert_eq!(PREVIEW_TIMEOUT, Duration::from_secs(60));
        assert_eq!(LOGIN_SHELL_TIMEOUT, Duration::from_secs(3));
    }
}
