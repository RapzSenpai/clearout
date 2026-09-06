import type { AppInfo, ScanResult } from '../types'

// Snapshot of a finished leftover scan the user hasn't cleaned up yet.
// Lets them leave the Review page and resume later without re-scanning.
export interface PendingCleanup {
  primary: AppInfo
  apps: AppInfo[]
  scan: ScanResult
  // True when the scan ran while the app was still installed (removal, not leftovers).
  removalScan: boolean
  notes: string[]
  // App ids whose native uninstaller ran (or was skipped) — drives install-dir locking.
  confirmedIds: string[]
  savedAt: number
}

let pending: PendingCleanup | null = $state(null)

export function getPendingCleanup() { return pending }
export function setPendingCleanup(p: PendingCleanup) { pending = p }
export function clearPendingCleanup() { pending = null }
