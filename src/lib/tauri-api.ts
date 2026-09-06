import { invoke } from '@tauri-apps/api/core'
import type { AppInfo, ScanResult, DeleteResult, LockInfo, AiResponse, VerifyResult, LeftoverItem, RegistryBackupEntry } from './types'

export async function getInstalledApps(): Promise<AppInfo[]> {
  return invoke<AppInfo[]>('get_installed_apps')
}

export async function runUninstaller(uninstallString: string): Promise<number> {
  // Tauri v2 matches command args by camelCase key; snake_case kept for safety.
  return invoke<number>('run_uninstaller', {
    uninstallString,
    uninstall_string: uninstallString,
  } as any)
}

export async function scanLeftovers(app: AppInfo): Promise<ScanResult> {
  // Strip heavy icon payload — not needed for scan, bloats IPC and can hit limits
  const { icon: _icon, ...cleanApp } = app as AppInfo & { icon?: string }
  const settings = (await import('./stores/settings.svelte')).getSettings()
  return invoke<ScanResult>('scan_leftovers', {
    app_info: cleanApp,
    appInfo: cleanApp,
    excluded_paths: settings.excludedPaths ?? [],
    excludedPaths: settings.excludedPaths ?? [],
    excluded_hosts: settings.excludedHosts ?? [],
    excludedHosts: settings.excludedHosts ?? [],
    scan_depth: settings.scanDepth,
    scanDepth: settings.scanDepth,
  } as any)
}

export async function listRegistryBackups(): Promise<RegistryBackupEntry[]> {
  return invoke<RegistryBackupEntry[]>('list_registry_backups')
}

export async function restoreRegistryBackup(id: string): Promise<void> {
  return invoke('restore_registry_backup', { id })
}

export async function deleteRegistryBackup(id: string): Promise<void> {
  return invoke('delete_registry_backup', { id })
}

export async function deleteItems(items: LeftoverItem[], createRestorePoint: boolean, forceKillAllowed?: boolean): Promise<DeleteResult> {
  // Tauri v2 matches command args by camelCase key; snake_case kept for safety.
  return invoke<DeleteResult>('delete_items', {
    items,
    createRestorePoint,
    create_restore_point: createRestorePoint,
    forceKillAllowed,
    force_kill_allowed: forceKillAllowed,
  } as any)
}

export async function checkLocks(path: string): Promise<LockInfo[]> {
  return invoke<LockInfo[]>('check_locks', { path })
}

export async function askAi(
  path: string,
  name: string,
  itemType: string,
  associatedApp: string,
  apiKey: string,
  provider: string
): Promise<AiResponse> {
  return invoke<AiResponse>('ask_ai', {
    path,
    name,
    item_type: itemType,
    itemType: itemType,
    associated_app: associatedApp,
    associatedApp: associatedApp,
    api_key: apiKey,
    apiKey: apiKey,
    provider,
  } as any)
}

export async function verifyScan(app: AppInfo, deletedIds: string[], scanDepth?: string): Promise<VerifyResult> {
  const { icon: _icon, ...cleanApp } = app as AppInfo & { icon?: string }
  return invoke<VerifyResult>('verify_scan', {
    app: cleanApp,
    appInfo: cleanApp,
    deleted_ids: deletedIds,
    deletedIds: deletedIds,
    scan_depth: scanDepth,
    scanDepth: scanDepth,
  } as any)
}

export async function exportReportJson(
  appName: string,
  appVersion: string | undefined,
  scanResult: ScanResult,
  deleteResult: DeleteResult | null
): Promise<string> {
  return invoke<string>('export_report_json', {
    appName,
    app_name: appName,
    appVersion,
    app_version: appVersion,
    scanResult,
    scan_result: scanResult,
    deleteResult,
    delete_result: deleteResult,
  } as any)
}

export async function exportReportTxt(
  appName: string,
  appVersion: string | undefined,
  scanResult: ScanResult,
  deleteResult: DeleteResult | null
): Promise<string> {
  return invoke<string>('export_report_txt', {
    appName,
    app_name: appName,
    appVersion,
    app_version: appVersion,
    scanResult,
    scan_result: scanResult,
    deleteResult,
    delete_result: deleteResult,
  } as any)
}

export async function openLocation(path: string): Promise<void> {
  return invoke('open_location', { path })
}
