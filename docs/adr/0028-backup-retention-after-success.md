# Backup retention is independent of pre-operation protection

Status: accepted

The app-data setting controls whether operation Backup snapshots are retained after success; it defaults to enabled and is captured when the operation starts. Disabling it still creates required pre-operation snapshots, keeps snapshots after failure or interruption, and does not delete older backups. Skipping snapshot creation was rejected because the existing mutation and last-copy removal protocols rely on backup-before-write protection (ADR-0003, ADR-0025 and ADR-0027); reclaiming space after success meets the user's storage-management goal without changing those preconditions.

Retention applies to existing snapshot-producing operations, including Uninstall and Update. Standalone cancellation retains snapshots; settled successful Batch targets may release their own snapshots even when the batch stops, while snapshots shared with failed targets remain. Task-level snapshots require complete success. Runtime or interrupted references prevent deletion, and a corrupt interruption record protects all snapshots. Older snapshots are managed manually without inferring success from incomplete logs; automatic age-based retention and snapshot restoration remain outside this feature. See the [app-data specification](../../.scratch/app-data-management/spec.md) for cleanup and compatibility behavior.
