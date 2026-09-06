export interface AppInfo {
  id: string
  name: string
  version?: string
  publisher?: string
  install_location?: string
  estimated_size?: number
  uninstall_string?: string
  install_date?: string
  icon?: string
}

export interface LeftoverItem {
  id: string
  path: string
  item_type: LeftoverType
  confidence: number
  confidence_tier: ConfidenceTier
  associated_app: string
  size?: number
}

export type LeftoverType = 'File' | 'Folder' | 'Registry' | 'Service' | 'Startup' | 'Hosts' | 'Task'
export type ConfidenceTier = 'High' | 'Medium' | 'Low'

export interface ScanResult {
  files: LeftoverItem[]
  registry: LeftoverItem[]
  services: LeftoverItem[]
  startup: LeftoverItem[]
  hosts: LeftoverItem[]
  tasks: LeftoverItem[]
}

/** One skipped item plus the rule that caused the skip. */
export interface SkippedItem {
  path: string
  reason: string
}

export interface DeleteResult {
  deleted: number
  skipped: number
  /** Every rule-skipped item with its reason (same count as `skipped`). */
  skipped_items: SkippedItem[]
  /** Selected items that no longer existed at delete time — nothing to do. */
  already_gone: number
  already_gone_paths: string[]
  /** Paths of items moved to the internal trash (restorable from History). */
  trashed: string[]
  /** Ids of items successfully removed — used by verify_scan. */
  deleted_ids: string[]
  errors: string[]
  /** Restore point outcome — separate from item errors on purpose. */
  restore_point_ok: boolean
  restore_point_error: string | null
}

export interface LockInfo {
  pid: number
  process_name: string
  path: string
}

export interface AiResponse {
  assessment: string
  confidence: string
  recommendation: string
}

export interface VerifyResult {
  remaining: ScanResult
  remaining_count: number
  deleted_count: number
  failed_items: LeftoverItem[]
}

export interface Settings {
  aiEnabled: boolean
  aiProvider: string
  apiKey: string
  forceKillAllowed: boolean
  restorePointDefault: boolean
  scanDepth: 'fast' | 'thorough'
  excludedPaths: string[]
  excludedHosts: string[]
  theme: 'light' | 'dark' | 'system'
  /** Accent family — colored themes keep console identity, mono drops color. */
  accent: 'green' | 'cyan' | 'purple' | 'amber' | 'mono'
}

export interface RegistryBackupEntry {
  id: string
  created: string
  root_path: string
  key_count: number
  value_count: number
}
