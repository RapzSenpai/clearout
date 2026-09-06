<script lang="ts">
  import { onMount, onDestroy } from 'svelte'
  import { scanLeftovers, deleteItems, askAi, runUninstaller, verifyScan, exportReportJson, exportReportTxt } from '../lib/tauri-api'
  import type { AppInfo, ScanResult, LeftoverItem, AiResponse, DeleteResult, SkippedItem, VerifyResult } from '../lib/types'
  import { formatSize } from '../lib/utils'
  import { getSettings } from '../lib/stores/settings.svelte'
  import { setPendingCleanup, clearPendingCleanup, type PendingCleanup } from '../lib/stores/pending.svelte'
  import LeftoverTable from '../lib/components/LeftoverTable.svelte'
  import { Trash2, Scan, ChevronRight, MessageCircle, Loader2, Globe, Clock, Check, ShieldCheck, ShieldAlert, XCircle, X, Info } from '@lucide/svelte'
  import { Checkbox, Dialog, Progress } from 'bits-ui'
  import { listen } from '@tauri-apps/api/event'
  import { invoke } from '@tauri-apps/api/core'

  let {
    app,
    apps = [],
    onBack,
    onScanComplete,
    restore = null
  }: {
    app: AppInfo
    apps?: AppInfo[]
    onBack: () => void
    onScanComplete: (result: ScanResult) => void
    restore?: PendingCleanup | null
  } = $props()

  let displayApps = $derived(apps.length > 0 ? apps : [app])
  let isMulti = $derived(displayApps.length > 1)

  type Phase = 'gate' | 'scanning' | 'review' | 'summary'
  let phase = $state<Phase>('gate')
  let scanResult: ScanResult | null = $state(null)
  let selectedItems: Set<string> = $state(new Set())
  let createRestorePoint = $state(getSettings().restorePointDefault)
  let scanning = $state(false)
  let scanError = $state(false)
  let scanErrorMsg = $state('')
  // Elevation awareness — restore points need admin; surface it before delete
  // instead of failing silently mid-operation.
  let isAdmin = $state(true)
  let adminKnown = $state(false)
  let deleting = $state(false)
  let activeTab: 'files' | 'registry' | 'services' | 'startup' | 'hosts' | 'tasks' = $state('files')
  let showDeleteDialog = $state(false)
  let scanProgress = $state(0)
  let scanStage = $state('')
  let deleteProgress = $state(0)
  let deleteStage = $state('')
  let deleteCurrent = $state(0)
  let deleteTotal = $state(0)
  let iconError = $state(false)
  let unsubscribe: (() => void) | null = null
  let deleteUnsub: (() => void) | null = null

  // --- Uninstall gate -------------------------------------------------------
  // App ids whose native uninstaller ran successfully (or was skipped as
  // already-uninstalled). Apps still installed keep their install-dir items locked.
  let confirmedUninstalled: Set<string> = $state(new Set())
  let gateNotes: string[] = $state([])
  let hasUninstallers = $derived(displayApps.some(a => (a.uninstall_string ?? '').trim().length > 0))
  // Apps whose native uninstaller exited with an error (or failed to start).
  // While non-empty the gate stays open and offers retry / scan-anyway.
  let failedApps: AppInfo[] = $state([])
  let uninstallFailed = $derived(failedApps.length > 0)
  // True when the scan happens while the app is still installed (no native
  // uninstaller, or it failed and the user chose manual removal). The review
  // page then speaks of "removal", not "leftovers".
  let removalScan = $state(false)

  let needGate = $derived(hasUninstallers)

  // --- Delete / verify state -------------------------------------------------
  interface DeletePayload {
    deleted: number
    skipped: number
    skippedItems: SkippedItem[]
    alreadyGone: number
    trashed: string[]
    deleted_ids: string[]
    errors: string[]
    restorePointOk: boolean
    restorePointError: string | null
    scanSnapshot: ScanResult
    deletedByApp: Map<string, string[]>
  }
  let deletePayload: DeletePayload | null = $state(null)
  let verifyBusy = $state(false)
  let exportInfo = $state('')
  let verifyInfo = $state('')

  // Summary scenario — drives the headline and which sections make sense:
  // clean (all removed) / partial (some rule-skipped) / errors (failures) /
  // gone (nothing left to do) / nothing (nothing happened).
  type SummaryOutcome = 'clean' | 'partial' | 'errors' | 'gone' | 'nothing'
  let summaryOutcome = $derived.by<SummaryOutcome>(() => {
    if (!deletePayload) return 'nothing'
    const { deleted, trashed, skipped, alreadyGone, errors } = deletePayload
    if (errors.length > 0) return 'errors'
    if (deleted === 0 && trashed.length === 0) {
      if (skipped > 0) return 'partial'
      if (alreadyGone > 0) return 'gone'
      return 'nothing'
    }
    if (skipped > 0) return 'partial'
    return 'clean'
  })

  // Distinct skip reasons with counts, rendered as short bullets.
  let skippedGroups = $derived.by(() => {
    if (!deletePayload) return [] as { reason: string; count: number }[]
    const map = new Map<string, number>()
    for (const s of deletePayload.skippedItems) {
      map.set(s.reason, (map.get(s.reason) ?? 0) + 1)
    }
    return [...map.entries()].map(([reason, count]) => ({ reason, count }))
  })

  let summaryHeadline = $derived.by(() => {
    if (!deletePayload) return ''
    const { deleted, skipped, alreadyGone, errors } = deletePayload
    switch (summaryOutcome) {
      case 'errors':
        if (deleted === 0) return `Finished with problems — ${errors.length} failed`
        return `Finished with problems — ${deleted} removed, ${errors.length} failed`
      case 'partial':
        if (deleted === 0) return `Nothing removed — ${skipped} skipped`
        return `Cleanup finished — ${deleted} removed, ${skipped} skipped`
      case 'gone':
        return 'Cleanup complete'
      case 'nothing':
        return 'Nothing needed to be removed'
      default:
        return `Cleanup complete — ${deleted} removed`
    }
  })
  let summaryStatus = $derived.by(() => {
    if (!deletePayload) return ''
    switch (summaryOutcome) {
      case 'errors':
        return 'Some items could not be removed. Check the errors below, then re-scan to see what remains.'
      case 'partial':
        return 'Some items were skipped by safety rules — see why below.'
      case 'gone':
        return 'The selected items were already removed — nothing else needed to be done.'
      case 'nothing':
        return 'No deletions were performed.'
      default:
        return 'All selected leftovers were removed.'
    }
  })

  let aiEnabled = $derived(getSettings().aiEnabled && getSettings().apiKey.trim().length > 0)
  let aiAnalyzing = $state(false)
  let aiResults: Map<string, AiResponse> = $state(new Map())
  let aiErrors: Map<string, string> = $state(new Map())
  let aiCurrentIndex = $state(0)
  let aiSelectedItemIds: string[] = $state([])
  let showAiDialog = $state(false)

  function getItems(): LeftoverItem[] {
    if (!scanResult) return []
    switch (activeTab) {
      case 'files': return scanResult.files
      case 'registry': return scanResult.registry
      case 'services': return scanResult.services
      case 'startup': return scanResult.startup
      case 'hosts': return scanResult.hosts ?? []
      case 'tasks': return scanResult.tasks ?? []
      default: return []
    }
  }

  function getAllItems(): LeftoverItem[] {
    if (!scanResult) return []
    return [
      ...scanResult.files,
      ...scanResult.registry,
      ...scanResult.services,
      ...scanResult.startup,
      ...(scanResult.hosts ?? []),
      ...(scanResult.tasks ?? []),
    ]
  }

  let currentItems = $derived(getItems())
  let allItems = $derived(getAllItems())
  let selectedCount = $derived(selectedItems.size)

  let highConfidenceItems = $derived(
    currentItems.filter((item: LeftoverItem) => item.confidence_tier === 'High')
  )

  // Items under the install dir of an app that is still installed → protected.
  let lockedIds = $derived.by(() => {
    const locked = new Set<string>()
    if (!scanResult) return locked
    const stillInstalled = displayApps.filter(a => !confirmedUninstalled.has(a.id))
    if (stillInstalled.length === 0) return locked
    for (const a of stillInstalled) {
      const loc = (a.install_location ?? '').trim().replace(/^"+|"+$/g, '')
      if (!loc) continue
      const locLower = loc.toLowerCase()
      for (const it of scanResult.files) {
        const p = it.path.toLowerCase()
        if (p === locLower || p.startsWith(locLower + '\\')) locked.add(it.id)
      }
    }
    return locked
  })

  function toggleSelectAll() {
    const selectable = currentItems.filter(i => !lockedIds.has(i.id))
    if (selectable.length > 0 && selectable.every((item: LeftoverItem) => selectedItems.has(item.id))) {
      selectable.forEach((item: LeftoverItem) => selectedItems.delete(item.id))
    } else {
      selectable.forEach((item: LeftoverItem) => selectedItems.add(item.id))
    }
    selectedItems = new Set(selectedItems)
  }

  function toggleSelectHigh() {
    const highIds = highConfidenceItems.filter(i => !lockedIds.has(i.id)).map((item: LeftoverItem) => item.id)
    if (highIds.every((id: string) => selectedItems.has(id))) {
      highIds.forEach((id: string) => selectedItems.delete(id))
    } else {
      highIds.forEach((id: string) => selectedItems.add(id))
    }
    selectedItems = new Set(selectedItems)
  }

  function toggleItem(id: string) {
    if (lockedIds.has(id)) return
    if (selectedItems.has(id)) {
      selectedItems.delete(id)
    } else {
      selectedItems.add(id)
    }
    selectedItems = new Set(selectedItems)
  }

  // --- Uninstall gate actions ------------------------------------------------
  function errText(e: unknown): string {
    return e instanceof Error ? e.message : typeof e === 'string' ? e : JSON.stringify(e)
  }

  async function runUninstallers(targets?: AppInfo[]) {
    const apps = targets ?? displayApps
    removalScan = false
    failedApps = []
    scanning = true
    scanStage = apps.length > 1 ? 'Running native uninstallers…' : 'Running native uninstaller…'
    gateNotes = []
    for (let i = 0; i < apps.length; i++) {
      const a = apps[i]
      const us = (a.uninstall_string ?? '').trim()
      if (!us) {
        confirmedUninstalled.add(a.id)
        gateNotes.push(`${a.name}: registered no native uninstaller — its installed files will be included in the scan review.`)
        continue
      }
      scanStage = apps.length > 1
        ? `Running uninstaller: ${a.name} (${i + 1}/${apps.length})…`
        : `Running uninstaller: ${a.name}…`
      try {
        const code = await runUninstaller(us)
        if (code === 0 || code === 3010) {
          confirmedUninstalled.add(a.id)
          gateNotes.push(`${a.name}: native uninstaller finished (exit ${code})`)
        } else {
          failedApps.push(a)
          gateNotes.push(`${a.name}: uninstaller reported exit code ${code} — it may still be installed.`)
        }
      } catch (e) {
        failedApps.push(a)
        gateNotes.push(`${a.name}: uninstaller failed to start — ${errText(e)}`)
      }
    }
    confirmedUninstalled = new Set(confirmedUninstalled)
    failedApps = [...failedApps]
    scanning = false
    // Uninstaller didn't finish: stay on the gate and offer retry / force
    // remove instead of scanning a half-installed app.
    if (failedApps.length > 0) return
    await startScan()
  }

  function skipUninstallers() {
    // User states apps were already removed externally (or wants scan-only review).
    removalScan = false
    displayApps.forEach(a => confirmedUninstalled.add(a.id))
    confirmedUninstalled = new Set(confirmedUninstalled)
    failedApps = []
    gateNotes = []
    gateNotes.push('Scanning without running native uninstallers. Items under an install directory that still exists will be shown for review.')
    startScan()
  }

  function scanTracesDirectly(appName: string, reason: string) {
    // No native uninstaller (or the uninstaller failed) — scan everything
    // belonging to the app so the user can remove it manually. The install
    // directory itself is fair game, so nothing stays locked.
    removalScan = true
    displayApps.forEach(a => confirmedUninstalled.add(a.id))
    confirmedUninstalled = new Set(confirmedUninstalled)
    failedApps = []
    gateNotes = [
      reason,
      `${appName} may still be running — close it before deleting its files, or enable force-close in Settings.`,
    ]
    startScan()
  }

  // --- AI ---------------------------------------------------------------------
  async function handleAnalyzeSelected() {
    if (selectedItems.size === 0 || aiAnalyzing) return
    const cfg = getSettings()
    if (!cfg.aiEnabled || !cfg.apiKey.trim()) return

    aiSelectedItemIds = Array.from(selectedItems)
    aiCurrentIndex = 0
    aiResults = new Map()
    aiErrors = new Map()
    aiAnalyzing = true
    showAiDialog = true

    for (const id of aiSelectedItemIds) {
      const item = allItems.find((i: LeftoverItem) => i.id === id)
      if (!item) continue
      try {
        const name = item.path.split('\\').pop() || item.path.split('/').pop() || item.path
        const res = await askAi(item.path, name, item.item_type, item.associated_app, cfg.apiKey, cfg.aiProvider)
        aiResults = new Map(aiResults).set(id, res)
      } catch (e) {
        aiErrors = new Map(aiErrors).set(id, errText(e))
      }
    }

    aiAnalyzing = false
  }

  async function handleAnalyzeSingle(item: LeftoverItem) {
    if (aiAnalyzing) return
    const cfg = getSettings()
    if (!cfg.aiEnabled || !cfg.apiKey.trim()) return
    if (item.item_type === 'Hosts' || item.item_type === 'Task') return
    aiSelectedItemIds = [item.id]
    aiCurrentIndex = 0
    aiResults = new Map()
    aiErrors = new Map()
    aiAnalyzing = true
    showAiDialog = true
    try {
      const name = item.path.split('\\').pop() || item.path.split('/').pop() || item.path
      const res = await askAi(item.path, name, item.item_type, item.associated_app, cfg.apiKey, cfg.aiProvider)
      aiResults = new Map(aiResults).set(item.id, res)
    } catch (e) {
      aiErrors = new Map(aiErrors).set(item.id, errText(e))
    } finally {
      aiAnalyzing = false
    }
  }

  async function retryAiCurrent() {
    if (!aiCurrentItem) return
    const cfg = getSettings()
    const id = aiCurrentItem.id
    aiErrors.delete(id)
    aiErrors = new Map(aiErrors)
    aiResults.delete(id)
    aiResults = new Map(aiResults)
    aiAnalyzing = true
    try {
      const name = aiCurrentItem.path.split('\\').pop() || aiCurrentItem.path.split('/').pop() || aiCurrentItem.path
      const res = await askAi(aiCurrentItem.path, name, aiCurrentItem.item_type, aiCurrentItem.associated_app, cfg.apiKey, cfg.aiProvider)
      aiResults = new Map(aiResults).set(id, res)
    } catch (e) {
      aiErrors = new Map(aiErrors).set(id, errText(e))
    } finally {
      aiAnalyzing = false
    }
  }

  function aiNavNext() {
    if (aiCurrentIndex < aiSelectedItemIds.length - 1) aiCurrentIndex++
  }

  function aiNavPrev() {
    if (aiCurrentIndex > 0) aiCurrentIndex--
  }

  let aiCurrentItem = $derived.by(() => {
    if (aiSelectedItemIds.length === 0) return null
    const id = aiSelectedItemIds[aiCurrentIndex]
    return allItems.find((i: LeftoverItem) => i.id === id) ?? null
  })

  let aiCurrentResult = $derived.by(() => {
    if (!aiCurrentItem) return null
    return aiResults.get(aiCurrentItem.id) ?? null
  })

  let aiCurrentError = $derived.by(() => {
    if (!aiCurrentItem) return null
    return aiErrors.get(aiCurrentItem.id) ?? null
  })

  let aiIsEmpty = $derived.by(() => {
    if (!aiCurrentResult) return false
    const t = aiCurrentResult.assessment.trim().toLowerCase()
    return t === '' || t === 'no response'
  })

  let aiCurrentLoading = $derived.by(() => {
    if (!aiCurrentItem) return false
    if (aiCurrentResult || aiCurrentError) return false
    return aiAnalyzing
  })

  // --- Delete -----------------------------------------------------------------
  async function handleDelete() {
    showDeleteDialog = false
    if (selectedItems.size === 0) return

    const itemsToDelete = allItems.filter((item: LeftoverItem) => selectedItems.has(item.id))
    deleting = true
    deleteProgress = 0
    deleteStage = 'Preparing...'
    deleteCurrent = 0
    deleteTotal = itemsToDelete.length
    const deleteStartedAt = Date.now()

    try {
      deleteUnsub = await listen<any>('delete-progress', (event) => {
        const { stage, current, total, percent } = event.payload
        deleteProgress = percent
        deleteCurrent = current
        deleteTotal = total
        if (stage === 'start') deleteStage = 'Starting...'
        else if (stage === 'restore-point') deleteStage = 'Creating restore point...'
        else if (stage === 'files') deleteStage = 'Removing files...'
        else if (stage === 'registry') deleteStage = 'Removing registry entries...'
        else if (stage === 'services') deleteStage = 'Removing services...'
        else if (stage === 'startup') deleteStage = 'Removing startup entries...'
        else if (stage === 'hosts') deleteStage = 'Checking hosts (read-only)...'
        else if (stage === 'tasks') deleteStage = 'Checking tasks (read-only)...'
        else if (stage === 'done') deleteStage = 'Done'
      })

      const result = await deleteItems(itemsToDelete, createRestorePoint, getSettings().forceKillAllowed)
      deleteProgress = 100
      deleteStage = 'Done'
      if (deleteUnsub) {
        deleteUnsub()
        deleteUnsub = null
      }
      const elapsed = Date.now() - deleteStartedAt
      if (elapsed < 700) {
        await new Promise((r) => setTimeout(r, 700 - elapsed))
      }

      // Group deleted ids per app so verification can re-scan per app.
      const deletedByApp = new Map<string, string[]>()
      for (const item of itemsToDelete) {
        if (!result.deleted_ids.includes(item.id)) continue
        const list = deletedByApp.get(item.associated_app) ?? []
        list.push(item.id)
        deletedByApp.set(item.associated_app, list)
      }

      deletePayload = {
        deleted: result.deleted,
        skipped: result.skipped,
        skippedItems: result.skipped_items ?? [],
        alreadyGone: result.already_gone ?? 0,
        trashed: result.trashed,
        deleted_ids: result.deleted_ids,
        errors: result.errors,
        restorePointOk: result.restore_point_ok,
        restorePointError: result.restore_point_error ?? null,
        scanSnapshot: scanResult!,
        deletedByApp,
      }
      // The pending cleanup has been acted on — drop the Dashboard card.
      clearPendingCleanup()
      verifyInfo = ''
      selectedItems = new Set()
      phase = 'summary'
      doExport()
    } catch (e) {
      console.error('Delete failed:', e)
      scanError = true
      scanErrorMsg = `Delete failed: ${errText(e)}`
    } finally {
      deleting = false
      if (deleteUnsub) {
        deleteUnsub()
        deleteUnsub = null
      }
    }
  }

  async function doExport() {
    if (!deletePayload || !scanResult) return
    const single = !isMulti ? displayApps[0] : null
    const name = single ? single.name : `${displayApps.length} queued apps`
    const version = single ? single.version : undefined
    const reportDelete: DeleteResult = {
      deleted: deletePayload.deleted,
      skipped: deletePayload.skipped,
      skipped_items: deletePayload.skippedItems,
      already_gone: deletePayload.alreadyGone,
      already_gone_paths: [],
      trashed: deletePayload.trashed,
      deleted_ids: deletePayload.deleted_ids,
      errors: deletePayload.errors,
      restore_point_ok: deletePayload.restorePointOk,
      restore_point_error: deletePayload.restorePointError,
    }
    exportInfo = 'Saving report…'
    try {
      await exportReportJson(name, version, deletePayload.scanSnapshot, reportDelete)
      await exportReportTxt(name, version, deletePayload.scanSnapshot, reportDelete)
      exportInfo = 'Report saved to History'
    } catch (e) {
      exportInfo = `Auto-export failed: ${errText(e)}`
    }
  }

  async function handleVerify() {
    if (!deletePayload || verifyBusy) return
    verifyBusy = true
    verifyInfo = 'Re-scanning…'
    try {
      let confirmed = 0
      let stillPresent = 0
      let failures: string[] = []
      for (const a of displayApps) {
        const ids = deletePayload.deletedByApp.get(a.name) ?? []
        if (ids.length === 0) continue
        const r: VerifyResult = await verifyScan(a, ids, getSettings().scanDepth)
        confirmed += r.deleted_count
        stillPresent += r.remaining_count
        failures = failures.concat(r.failed_items.map(i => `${i.item_type}: ${i.path}`))
      }
      const failNote = failures.length > 0
        ? `\nStill present: ${failures.slice(0, 10).join('\n')}${failures.length > 10 ? `\n…and ${failures.length - 10} more` : ''}`
        : ''
      verifyInfo = `Verified: ${confirmed} of ${deletePayload.deleted_ids.length} deletions confirmed gone.${failNote}`
    } catch (e) {
      verifyInfo = `Verification failed: ${errText(e)}`
    } finally {
      verifyBusy = false
    }
  }

  // --- Scan --------------------------------------------------------------------
  function mergeList(existing: LeftoverItem[], added: LeftoverItem[]): LeftoverItem[] {
    const ids = new Set(existing.map(i => i.id))
    return [...existing, ...added.filter(i => !ids.has(i.id))]
  }

  async function startScan() {
    if (displayApps.length === 0) return
    scanning = true
    scanError = false
    scanErrorMsg = ''
    scanProgress = 0
    deletePayload = null
    verifyInfo = ''
    scanStage = isMulti ? `Scanning ${displayApps.length} apps...` : 'Starting scan...'

    try {
      if (unsubscribe) {
        unsubscribe()
        unsubscribe = null
      }
      unsubscribe = await listen<any>('scan-progress', (event) => {
        const { stage, percent } = event.payload
        scanProgress = percent
        if (stage === 'scanning') scanStage = isMulti ? `Scanning... ${percent}%` : 'Scanning...'
        else if (stage === 'scoring') scanStage = 'Scoring results...'
        else if (stage === 'done') scanStage = 'Done'
      })

      // Batch scan: merge results from all queued apps (dedupe by stable id)
      let merged: ScanResult | null = null
      for (let idx = 0; idx < displayApps.length; idx++) {
        const a = displayApps[idx]
        if (isMulti) {
          scanStage = `Scanning ${a.name} (${idx + 1}/${displayApps.length})...`
          scanProgress = Math.round((idx / displayApps.length) * 80 + 10)
        }
        const res = await scanLeftovers(a)
        if (!merged) {
          merged = res
        } else {
          merged.files = mergeList(merged.files, res.files)
          merged.registry = mergeList(merged.registry, res.registry)
          merged.services = mergeList(merged.services, res.services)
          merged.startup = mergeList(merged.startup, res.startup)
          merged.hosts = mergeList(merged.hosts, res.hosts ?? [])
          merged.tasks = mergeList(merged.tasks, res.tasks ?? [])
        }
      }
      scanResult = merged

      if (unsubscribe) {
        unsubscribe()
        unsubscribe = null
      }

      scanProgress = 100
      scanStage = 'Done'

      if (scanResult) {
        onScanComplete(scanResult)
        // Keep the results as a pending cleanup so the user can leave and
        // resume later from the Dashboard without re-scanning. Nothing is
        // deleted until they approve it on the review page.
        const total = scanResult.files.length + scanResult.registry.length +
          scanResult.services.length + scanResult.startup.length +
          (scanResult.hosts?.length ?? 0) + (scanResult.tasks?.length ?? 0)
        if (total > 0) {
          setPendingCleanup({
            primary: displayApps[0],
            apps: [...displayApps],
            scan: scanResult,
            removalScan,
            notes: [...gateNotes],
            confirmedIds: [...confirmedUninstalled],
            savedAt: Date.now(),
          })
        }
      }

      // Keep Done visible briefly before switching to review
      await new Promise((r) => setTimeout(r, 500))
      phase = 'review'
    } catch (e) {
      console.error('Scan failed:', e)
      scanError = true
      scanErrorMsg = errText(e)
      if (unsubscribe) {
        unsubscribe()
        unsubscribe = null
      }
    } finally {
      scanning = false
    }
  }

  onMount(async () => {
    try {
      isAdmin = await invoke<boolean>('is_admin')
    } catch {
      isAdmin = false
    }
    adminKnown = true
    // Not elevated → a restore point can never succeed; don't let the checkbox
    // pretend otherwise. The deletion itself still works (except services).
    if (!isAdmin) createRestorePoint = false
  })

  onMount(() => {
    if (restore) {
      // Resuming a pending cleanup from the Dashboard — jump straight to the
      // saved review without re-scanning.
      scanResult = restore.scan
      removalScan = restore.removalScan
      gateNotes = [...restore.notes]
      confirmedUninstalled = new Set(restore.confirmedIds)
      phase = 'review'
      return
    }
    phase = 'gate'
    if (!needGate) {
      // Nothing in the queue registers a native uninstaller — no real choice
      // to offer the user, so go straight to a removal scan.
      const names = isMulti ? `${displayApps.length} queued apps` : app.name
      scanTracesDirectly(
        names,
        `${names} registered no native uninstaller. ClearOut scanned everything belonging to the ` +
        'app — including its installed program files — so you can review and remove it manually. ' +
        'Nothing is deleted until you approve, and deletions stay reversible via soft trash, ' +
        'registry backups and the restore point.'
      )
    }
  })

  onDestroy(() => {
    if (unsubscribe) {
      unsubscribe()
    }
    if (deleteUnsub) {
      deleteUnsub()
    }
  })
</script>

{#if deleting}
  <div class="scanning-only">
    <div class="scan-icon">
      <Trash2 size={24} />
    </div>
    <p class="scan-label">{deleteStage || 'Deleting...'}</p>
    <Progress.Root value={deleteProgress} max={100} class="progress-root">
      <div class="progress-indicator" style="width: {deleteProgress}%"></div>
    </Progress.Root>
    <p class="scan-hint">
      {deleteTotal > 0 ? `${deleteCurrent} / ${deleteTotal} items • ${deleteProgress}%` : `${deleteProgress}%`}
    </p>
  </div>
{:else if scanning}
  <div class="scanning-only">
    <div class="scan-icon">
      <Scan size={24} />
    </div>
    <p class="scan-label">{scanStage || 'Scanning files...'}</p>
    <Progress.Root value={scanProgress} max={100} class="progress-root">
      <div class="progress-indicator" style="width: {scanProgress}%"></div>
    </Progress.Root>
    <p class="scan-hint">{scanProgress}% complete</p>
  </div>
{:else if phase === 'gate'}
  <div class="review">
    <nav class="breadcrumb-bar" aria-label="Breadcrumb navigation">
      <button class="breadcrumb-link" onclick={onBack}>Installed Apps</button>
      <ChevronRight size={12} class="breadcrumb-sep" />
      <span class="breadcrumb-current" aria-current="page">Uninstall</span>
    </nav>

    {#if isMulti}
      <div class="hero-stack" role="list" aria-label="Queued apps — batch review">
        {#each displayApps as a (a.id)}
          <div class="hero-mini" role="listitem" title="{a.name}">
            <div class="hero-mini-icon">
              {#if a.icon}
                <img src="data:image/png;base64,{a.icon}" alt="{a.name} icon" class="hero-mini-img" />
              {:else}
                <span class="hero-mini-letter">{a.name.charAt(0).toUpperCase()}</span>
              {/if}
            </div>
            <div class="hero-mini-info">
              <span class="hero-mini-name">{a.name}</span>
              {#if a.version}<span class="hero-mini-version">{a.version}</span>{/if}
            </div>
          </div>
        {/each}
      </div>
    {:else}
      <div class="app-hero">
        <div class="app-hero-icon">
          {#if app.icon && !iconError}
            <img
              src="data:image/png;base64,{app.icon}"
              alt="{app.name} icon"
              class="hero-icon-img"
              onerror={() => iconError = true}
            />
          {:else}
            <span class="hero-icon-letter">{app.name.charAt(0).toUpperCase()}</span>
          {/if}
        </div>
        <div class="app-hero-details">
          <div class="app-hero-title-row">
            <h1>{app.name}</h1>
            {#if app.version}
              <span class="version-tag">{app.version}</span>
            {/if}
          </div>
          <div class="app-hero-meta">
            <span>{app.publisher || 'Unknown publisher'}</span>
            {#if app.estimated_size}
              <span class="meta-dot">&bull;</span>
              <span>{formatSize(app.estimated_size)}</span>
            {/if}
            <span class="meta-dot">&bull;</span>
            <span class="meta-highlight">Step 1 of 2 — Uninstall</span>
          </div>
        </div>
      </div>
    {/if}

    <div class="gate-card">
      <div class="gate-icon"><ShieldCheck size={18} /></div>
      <div class="gate-body">
        <h2 class="gate-title">{uninstallFailed ? 'Uninstaller didn’t finish' : (isMulti ? 'Still installed — uninstall first?' : `${app.name} is still installed`)}</h2>
        <p class="gate-desc">
          {#if uninstallFailed}
            The uninstaller didn’t finish. Try it again, or scan its traces anyway and remove them manually.
          {:else if isMulti}
            {displayApps.length} apps are still installed — run their own uninstallers now, or skip if you already removed them.
          {:else}
            Uninstall {app.name} first — or skip if you already did and just want the leftovers scanned.
          {/if}
        </p>
        {#if gateNotes.length > 0}
          <ul class="gate-notes">
            {#each gateNotes as note (note)}
              <li>{note}</li>
            {/each}
          </ul>
        {/if}
        <div class="gate-actions">
          {#if uninstallFailed}
            <button class="btn-neo btn-neo--delete" onclick={() => runUninstallers(failedApps)}>
              <Trash2 size={13} strokeWidth={1.75} />
              Try the uninstaller again
            </button>
            <button class="btn-secondary" onclick={() => scanTracesDirectly(
              isMulti ? `${displayApps.length} queued apps` : app.name,
              'The uninstaller didn’t finish — scanning its traces manually instead. Review everything before anything is deleted.'
            )}>
              Scan traces without the uninstaller
            </button>
          {:else}
            <button class="btn-neo btn-neo--delete" onclick={() => runUninstallers()}>
              <Trash2 size={13} strokeWidth={1.75} />
              {isMulti ? `Uninstall ${displayApps.length} apps` : `Uninstall ${app.name}`}
            </button>
            <button class="btn-secondary" onclick={skipUninstallers}>
              Skip — scan for leftovers
            </button>
          {/if}
        </div>
        <p class="gate-hint">Nothing is deleted until you review and confirm on the next screens.</p>
      </div>
    </div>
  </div>
{:else if phase === 'summary'}
  <div class="review">
    <nav class="breadcrumb-bar" aria-label="Breadcrumb navigation">
      <button class="breadcrumb-link" onclick={onBack}>Installed Apps</button>
      <ChevronRight size={12} class="breadcrumb-sep" />
      <span class="breadcrumb-current" aria-current="page">Result Summary</span>
    </nav>

    {#if deletePayload}
      <div class="summary-card">
        <h2 class="summary-title">{summaryHeadline}</h2>
        <p class="summary-status">{summaryStatus}</p>
        <div class="summary-stats">
          {#if deletePayload.deleted > 0}
            <div class="summary-stat">
              <span class="summary-stat-num">{deletePayload.deleted}</span>
              <span class="summary-stat-label">removed</span>
            </div>
          {/if}
          {#if deletePayload.skipped > 0}
            <div class="summary-stat">
              <span class="summary-stat-num">{deletePayload.skipped}</span>
              <span class="summary-stat-label">skipped</span>
            </div>
          {/if}
          {#if deletePayload.errors.length > 0}
            <div class="summary-stat">
              <span class="summary-stat-num summary-stat-num--err">{deletePayload.errors.length}</span>
              <span class="summary-stat-label">failed</span>
            </div>
          {/if}
        </div>

        {#if deletePayload.alreadyGone > 0}
          <p class="summary-gone-note">
            <Info size={12} strokeWidth={1.75} />
            {deletePayload.alreadyGone} {deletePayload.alreadyGone === 1 ? 'item was' : 'items were'} already gone — removed by the uninstaller or an earlier cleanup.
          </p>
        {/if}

        {#if deletePayload.restorePointError}
          <div class="summary-block summary-block--warn">
            <h3><ShieldAlert size={13} /> Restore point was not created</h3>
            <ul>
              <li class="font-mono">{deletePayload.restorePointError}</li>
            </ul>
            <p class="summary-extra">Run ClearOut as Administrator with System Protection enabled to get restore points.</p>
          </div>
        {:else if deletePayload.restorePointOk}
          <div class="summary-block summary-block--ok">
            <h3><ShieldCheck size={13} /> Restore point created</h3>
          </div>
        {/if}

        {#if deletePayload.errors.length > 0}
          <div class="summary-block summary-block--err">
            <h3><XCircle size={13} /> Failed</h3>
            <ul>
              {#each deletePayload.errors.slice(0, 20) as e (e)}
                <li class="font-mono">{e}</li>
              {/each}
              {#if deletePayload.errors.length > 20}
                <li>…and {deletePayload.errors.length - 20} more</li>
              {/if}
            </ul>
          </div>
        {/if}

        {#if deletePayload.skipped > 0}
          <div class="summary-block summary-block--skipped">
            <h3><Clock size={13} /> Skipped</h3>
            {#each skippedGroups as group (group.reason)}
              <p class="skipped-reason">
                <span class="skipped-reason-count">{group.count}×</span>
                {group.reason}
              </p>
            {/each}
          </div>
        {/if}

        {#if deletePayload.trashed.length > 0}
          <div class="summary-block">
            <h3><Trash2 size={13} /> Restorable from History for 7 days</h3>
          </div>
        {/if}

        {#if verifyInfo}
          <div class="summary-verify-note font-mono">{verifyInfo}</div>
        {/if}

        {#if exportInfo}
          <p class="export-note" title="Reports are saved to %APPDATA%\ClearOut\reports">{exportInfo}</p>
        {/if}

        <div class="summary-actions">
          {#if deletePayload.deleted_ids.length > 0}
            <button class="btn-neo btn-neo--ai summary-primary" onclick={handleVerify} disabled={verifyBusy}>
              {#if verifyBusy}
                <Loader2 size={14} class="spin" />
                Verifying…
              {:else}
                <Scan size={14} strokeWidth={1.75} />
                Verify deletion (re-scan)
              {/if}
            </button>
          {/if}
          <div class="summary-secondary">
            <button class="btn-secondary" onclick={startScan}>
              <Scan size={13} strokeWidth={1.75} />
              Run another scan
            </button>
            <button class="summary-nav-link" onclick={onBack}>Back to apps</button>
          </div>
        </div>
      </div>
    {/if}
  </div>
{:else if scanError}
  <div class="scanning-only">
    <div class="scan-icon scan-icon--error">
      <Scan size={24} />
    </div>
    <p class="scan-label">Scan Failed</p>
    <p class="scan-hint">Could not scan leftovers for this app.</p>
    {#if scanErrorMsg}
      <p class="scan-error-detail">{scanErrorMsg}</p>
    {/if}
    <div class="scan-error-actions">
      <button class="btn-secondary" onclick={startScan}>Retry Scan</button>
      <button class="btn-secondary" onclick={onBack}>Back to Apps</button>
    </div>
  </div>
{:else if scanResult && phase === 'review'}
  <div class="review">
    <nav class="breadcrumb-bar" aria-label="Breadcrumb navigation">
      <button class="breadcrumb-link" onclick={onBack}>Installed Apps</button>
      <ChevronRight size={12} class="breadcrumb-sep" />
      {#if isMulti}
        <span class="breadcrumb-current" aria-current="page">{displayApps.length} apps queued</span>
      {:else}
        <span class="breadcrumb-current" aria-current="page">{app.name}</span>
      {/if}
    </nav>

    {#if isMulti}
      <div class="hero-stack" role="list" aria-label="Queued apps — batch review">
        {#each displayApps as a (a.id)}
          <div class="hero-mini" role="listitem" title="{a.name} — queued for batch review">
            <div class="hero-mini-icon">
              {#if a.icon}
                <img src="data:image/png;base64,{a.icon}" alt="{a.name} icon" class="hero-mini-img" />
              {:else}
                <span class="hero-mini-letter">{a.name.charAt(0).toUpperCase()}</span>
              {/if}
            </div>
            <div class="hero-mini-info">
              <span class="hero-mini-name">{a.name}</span>
              {#if a.version}<span class="hero-mini-version">{a.version}</span>{/if}
            </div>
          </div>
        {/each}
        <div class="hero-stack-meta">
          {#if scanResult}
            {@const totalReclaim = [...scanResult.files, ...scanResult.registry, ...scanResult.services, ...scanResult.startup, ...(scanResult.hosts ?? []), ...(scanResult.tasks ?? [])].reduce((s, i) => s + (i.size ?? 0), 0)}
            <span>{displayApps.length} apps</span>
            <span class="meta-dot">&bull;</span>
            <span>{scanResult.files.length + scanResult.registry.length + scanResult.services.length + scanResult.startup.length + (scanResult.hosts?.length ?? 0) + (scanResult.tasks?.length ?? 0)} leftovers</span>
            {#if totalReclaim > 0}
              <span class="meta-dot">&bull;</span>
              <span class="meta-reclaim">Reclaimable {formatSize(totalReclaim)}</span>
            {/if}
          {/if}
        </div>
      </div>
    {:else}
      <div class="app-hero">
        <div class="app-hero-icon">
          {#if app.icon && !iconError}
            <img
              src="data:image/png;base64,{app.icon}"
              alt="{app.name} icon"
              class="hero-icon-img"
              onerror={() => iconError = true}
            />
          {:else}
            <span class="hero-icon-letter">{app.name.charAt(0).toUpperCase()}</span>
          {/if}
        </div>
        <div class="app-hero-details">
          <div class="app-hero-title-row">
            <h1>{app.name}</h1>
            {#if app.version}
              <span class="version-tag">{app.version}</span>
            {/if}
          </div>
          <div class="app-hero-meta">
            <span>{app.publisher || 'Unknown publisher'}</span>
            {#if app.estimated_size}
              <span class="meta-dot">&bull;</span>
              <span>{formatSize(app.estimated_size)}</span>
            {/if}
            <span class="meta-dot">&bull;</span>
            <span class="meta-highlight">{removalScan ? 'Removal review — app still installed' : 'Leftover review'}</span>
            {#if scanResult}
              {@const totalReclaim = [...scanResult.files, ...scanResult.registry, ...scanResult.services, ...scanResult.startup, ...(scanResult.hosts ?? []), ...(scanResult.tasks ?? [])].reduce((s, i) => s + (i.size ?? 0), 0)}
              {#if totalReclaim > 0}
                <span class="meta-dot">&bull;</span>
                <span class="meta-reclaim">Reclaimable {formatSize(totalReclaim)}</span>
              {/if}
            {/if}
          </div>
        </div>
      </div>
    {/if}

    {#if gateNotes.length > 0}
      <div class="gate-notes-row">
        {#each gateNotes as note (note)}
          <p>{note}</p>
        {/each}
      </div>
    {/if}

    <div class="tabs-nav" role="tablist">
      <button type="button" role="tab" class="tab-btn" class:active={activeTab === 'files'} onclick={() => activeTab = 'files'} aria-selected={activeTab === 'files'}>
        Files <span class="tab-badge">{scanResult.files.length}</span>
      </button>
      <button type="button" role="tab" class="tab-btn" class:active={activeTab === 'registry'} onclick={() => activeTab = 'registry'} aria-selected={activeTab === 'registry'}>
        Registry <span class="tab-badge">{scanResult.registry.length}</span>
      </button>
      <button type="button" role="tab" class="tab-btn" class:active={activeTab === 'services'} onclick={() => activeTab = 'services'} aria-selected={activeTab === 'services'}>
        Services <span class="tab-badge">{scanResult.services.length}</span>
      </button>
      <button type="button" role="tab" class="tab-btn" class:active={activeTab === 'startup'} onclick={() => activeTab = 'startup'} aria-selected={activeTab === 'startup'}>
        Startup <span class="tab-badge">{scanResult.startup.length}</span>
      </button>
      <button type="button" role="tab" class="tab-btn tab-btn--readonly" class:active={activeTab === 'hosts'} onclick={() => activeTab = 'hosts'} aria-selected={activeTab === 'hosts'}>
        <Globe size={12} />
        Hosts <span class="tab-badge">{(scanResult.hosts ?? []).length}</span>
      </button>
      <button type="button" role="tab" class="tab-btn tab-btn--readonly" class:active={activeTab === 'tasks'} onclick={() => activeTab = 'tasks'} aria-selected={activeTab === 'tasks'}>
        <Clock size={12} />
        Tasks <span class="tab-badge">{(scanResult.tasks ?? []).length}</span>
      </button>
    </div>

    <div class="bulk-actions">
      <button class="btn-neo btn-neo--ai" onclick={toggleSelectAll} aria-label="Toggle select tab items">
        {currentItems.length > 0 && currentItems.every((item: LeftoverItem) => selectedItems.has(item.id) || lockedIds.has(item.id)) ? 'Deselect Tab' : 'Select Tab'}
      </button>
      <button class="btn-neo btn-neo--ai" onclick={toggleSelectHigh} aria-label="Select high confidence items">
        Select High Confidence
      </button>
      {#if lockedIds.size > 0}
        <span class="locked-hint">Shield {lockedIds.size} — still installed, run uninstaller first</span>
      {/if}
    </div>

    <LeftoverTable items={currentItems} {selectedItems} onSelect={toggleItem} onAnalyzeSingle={handleAnalyzeSingle} lockedIds={lockedIds} />

    <div class="bottom-bar">
      <div class="bottom-left">
        <span class="selected-count">{selectedCount} selected</span>
        <label class="restore-check" class:off={adminKnown && !isAdmin}>
          <Checkbox.Root
            bind:checked={createRestorePoint}
            disabled={adminKnown && !isAdmin}
            class="checkbox-root"
            aria-label="Create restore point before deleting"
          >
            <span class="checkbox-indicator">
              <Check size={11} strokeWidth={2.6} />
            </span>
          </Checkbox.Root>
          <span>Create restore point</span>
        </label>
        {#if adminKnown && !isAdmin}
          <span class="rp-warn"><ShieldAlert size={12} strokeWidth={1.75} /> restore point needs admin — off</span>
        {/if}
      </div>
      <div class="bottom-right">
        {#if aiEnabled && selectedCount > 0}
          <button class="btn-neo btn-neo--ai" disabled={aiAnalyzing || selectedCount === 0} onclick={handleAnalyzeSelected} aria-label="Analyze selected items with AI">
            {#if aiAnalyzing}
              <Loader2 size={13} strokeWidth={1.9} class="spin" />
              Analyzing…
            {:else}
              <MessageCircle size={13} strokeWidth={1.75} />
              Analyze with AI
            {/if}
          </button>
        {/if}
        <button class="btn-neo btn-neo--delete" disabled={selectedCount === 0 || deleting} onclick={() => showDeleteDialog = true} aria-label="Delete {selectedCount} selected items">
          <Trash2 size={13} strokeWidth={1.75} />
          {deleting ? 'Deleting…' : 'Delete Selected'}
        </button>
      {#if (activeTab === 'hosts' || activeTab === 'tasks') && currentItems.length > 0}
        <span class="readonly-hint">Read-only — review only, no auto-delete</span>
      {/if}
      </div>
    </div>
  </div>
{:else}
  <div class="scanning-only">
    <div class="scan-icon">
      <Scan size={24} />
    </div>
    <p class="scan-label">No results</p>
    <p class="scan-hint">No leftovers found for this app.</p>
    <button class="btn-secondary" onclick={onBack}>Back to Apps</button>
  </div>
{/if}

<Dialog.Root bind:open={showDeleteDialog}>
  <Dialog.Portal>
    <Dialog.Overlay class="dialog-overlay" />
    <Dialog.Content class="dialog-content">
      <Dialog.Title class="dialog-title">Confirm Delete</Dialog.Title>
      <Dialog.Description class="dialog-desc">
        Delete {selectedCount} selected {selectedCount === 1 ? 'item' : 'items'}?
        Files can be restored for 7 days. Registry and startup entries are backed up first.
        {#if createRestorePoint}A restore point will be created first.{/if}
      </Dialog.Description>
      {#if adminKnown && !isAdmin}
        <div class="rp-dialog-warn" role="note">
          <ShieldAlert size={13} strokeWidth={1.75} />
          <span>
            <strong>Not running as administrator</strong><br />
            Restore points and some service removals may be unavailable.
          </span>
        </div>
      {/if}
      <div class="dialog-actions">
        <Dialog.Close class="btn-secondary">Cancel</Dialog.Close>
        <Dialog.Close class="btn-delete" onclick={handleDelete}>
          <Trash2 size={14} />
          Delete
        </Dialog.Close>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<Dialog.Root bind:open={showAiDialog}>
  <Dialog.Portal>
    <Dialog.Overlay class="dialog-overlay" />
    <Dialog.Content class="dialog-content ai-dialog">
      <div class="ai-dialog-header">
        <Dialog.Title class="dialog-title">AI Advisory</Dialog.Title>
        <div class="ai-header-right">
          {#if aiSelectedItemIds.length > 1}
            <span class="ai-nav-label">{aiCurrentIndex + 1} / {aiSelectedItemIds.length}</span>
          {/if}
          <Dialog.Close class="ai-close" aria-label="Close AI advisory">
            <X size={15} strokeWidth={1.75} />
          </Dialog.Close>
        </div>
      </div>

      {#if aiCurrentItem}
        <p class="ai-path font-mono">{aiCurrentItem.path}</p>
      {/if}

      {#if aiCurrentLoading}
        <div class="ai-loading">
          <Loader2 size={22} class="spin" />
          <p>Analyzing this item...</p>
          <span class="ai-loading-hint">Usually 2–4 seconds</span>
        </div>
      {:else if aiCurrentError}
        <div class="ai-empty">
          <div class="ai-empty-icon ai-empty-icon--error">
            <MessageCircle size={20} />
          </div>
          <p class="ai-empty-title">Couldn't get an analysis</p>
          <p class="ai-empty-desc">
            {#if aiCurrentError.includes('API key') || aiCurrentError.includes('401') || aiCurrentError.includes('403')}
              Your API key seems invalid or has no credits. Open Settings to check the key for {getSettings().aiProvider}.
            {:else if aiCurrentError.includes('rate') || aiCurrentError.includes('429')}
              The provider is rate-limited. Wait about 30 seconds, then try again.
            {:else if aiCurrentError.includes('no content')}
              The AI returned an empty reply. This can happen with temporary provider hiccups — try again.
            {:else}
              Something went wrong for this item. You can retry just this one or skip to the next.
            {/if}
          </p>
          {#if !aiCurrentError.includes('API key') && !aiCurrentError.includes('401') && !aiCurrentError.includes('403')}
            <p class="ai-empty-detail font-mono">{aiCurrentError.slice(0, 200)}</p>
          {/if}
          <div class="ai-empty-actions">
            <button class="btn-primary" onclick={retryAiCurrent}>Try again</button>
            {#if aiSelectedItemIds.length > 1}
              <button class="btn-secondary" onclick={aiNavNext} disabled={aiCurrentIndex >= aiSelectedItemIds.length - 1}>Skip</button>
            {/if}
          </div>
        </div>
      {:else if aiCurrentResult && aiIsEmpty}
        <div class="ai-empty">
          <div class="ai-empty-icon">
            <MessageCircle size={20} />
          </div>
          <p class="ai-empty-title">No assessment available</p>
          <p class="ai-empty-desc">
            The AI returned no usable text for this path. This is usually a temporary provider issue.
          </p>
          <div class="ai-empty-actions">
            <button class="btn-primary" onclick={retryAiCurrent}>Try again</button>
            {#if aiSelectedItemIds.length > 1}
              <button class="btn-secondary" onclick={aiNavNext} disabled={aiCurrentIndex >= aiSelectedItemIds.length - 1}>Skip</button>
            {/if}
          </div>
        </div>
      {:else if aiCurrentResult}
        <div class="ai-body">
          <p class="ai-assessment">{aiCurrentResult.assessment}</p>
          <div class="ai-meta">
            <span class="ai-chip" class:ai-chip--high={aiCurrentResult.confidence === 'High'} class:ai-chip--low={aiCurrentResult.confidence === 'Low'}>Confidence: {aiCurrentResult.confidence}</span>
            <span class="ai-chip" class:ai-chip--delete={aiCurrentResult.recommendation === 'delete'} class:ai-chip--keep={aiCurrentResult.recommendation === 'keep'}>Recommendation: {aiCurrentResult.recommendation}</span>
          </div>
          <p class="ai-disclaimer">AI assessment only — not a guarantee. Review before deleting.</p>
        </div>
      {/if}

      <div class="dialog-actions ai-dialog-actions">
        {#if aiSelectedItemIds.length > 1}
          <button class="btn-secondary" onclick={aiNavPrev} disabled={aiCurrentIndex === 0}>Previous</button>
          {#if aiCurrentIndex < aiSelectedItemIds.length - 1}
            <button class="btn-primary" onclick={aiNavNext}>Next</button>
          {:else}
            <Dialog.Close class="btn-secondary">Done</Dialog.Close>
          {/if}
        {:else}
          <Dialog.Close class="btn-secondary">Done</Dialog.Close>
        {/if}
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<style>
  .scanning-only {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    min-height: 60vh;
    gap: 16px;
    text-align: center;
  }

  .scanning-only .scan-icon {
    width: 56px;
    height: 56px;
    border-radius: 14px;
    background-color: var(--color-accent-soft);
    color: var(--color-accent-text);
    display: flex;
    align-items: center;
    justify-content: center;
    animation: pulse 1.5s ease-in-out infinite;
  }

  .scanning-only .scan-icon--error {
    background-color: color-mix(in srgb, var(--color-danger) 12%, var(--color-surface));
    color: var(--color-danger);
    animation: none;
  }

  .scanning-only .scan-label {
    font-size: 16px;
    font-weight: 500;
    margin: 0;
    color: var(--color-text-primary);
  }

  .scanning-only .scan-hint {
    font-size: 13px;
    color: var(--color-text-secondary);
    margin: 0;
  }

  .scan-error-detail {
    font-size: 12px;
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 10%, var(--color-surface));
    border: 1px dashed color-mix(in srgb, var(--color-danger) 30%, var(--color-border));
    border-radius: 6px;
    padding: 8px 12px;
    max-width: 420px;
    word-break: break-word;
    font-family: var(--font-mono);
  }

  .scan-error-actions {
    display: flex;
    gap: 8px;
    margin-top: 4px;
  }

  .review {
    max-width: 960px;
    padding-bottom: 70px;
  }

  /* Breadcrumb Navigation — minimal, no redundant arrow */
  .breadcrumb-bar {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 18px;
  }

  .breadcrumb-link {
    display: inline-flex;
    align-items: center;
    background: transparent;
    border: none;
    color: var(--color-text-secondary);
    font-size: 13px;
    font-weight: 500;
    letter-spacing: -0.01em;
    cursor: pointer;
    padding: 3px 0;
    transition-property: color, opacity;
    transition-duration: 140ms;
    transition-timing-function: cubic-bezier(0.16, 1, 0.3, 1);
    -webkit-font-smoothing: antialiased;
  }

  .breadcrumb-link:hover {
    color: var(--color-accent-text);
  }

  .breadcrumb-link:active {
    opacity: 0.78;
  }

  .breadcrumb-link:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }

  :global(.breadcrumb-sep) {
    color: var(--color-text-secondary);
    opacity: 0.35;
  }

  .breadcrumb-current {
    font-size: 13px;
    font-weight: 600;
    color: var(--color-text-primary);
    letter-spacing: -0.01em;
    padding: 3px 0;
  }

  /* Uninstall gate */
  .gate-card {
    display: flex;
    gap: 14px;
    background-color: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 10px;
    padding: 20px;
    margin-bottom: 20px;
  }

  .gate-icon {
    width: 36px;
    height: 36px;
    border-radius: 9px;
    background-color: var(--color-accent-soft);
    color: var(--color-accent-text);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .gate-body {
    flex: 1;
    min-width: 0;
  }

  .gate-title {
    font-size: 15px;
    font-weight: 600;
    margin: 0 0 6px 0;
  }

  .gate-desc {
    font-size: 13px;
    line-height: 1.55;
    color: var(--color-text-secondary);
    margin: 0 0 14px 0;
    max-width: 640px;
  }

  .gate-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    align-items: center;
    margin-top: 4px;
  }

  .gate-hint {
    font-size: 12px;
    color: var(--color-text-secondary);
    margin: 12px 0 0 0;
    opacity: 0.85;
  }

  .gate-notes {
    list-style: none;
    margin: 0 0 12px 0;
    padding: 10px 12px;
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    font-size: 12.5px;
    color: var(--color-text-primary);
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .gate-notes li {
    line-height: 1.45;
  }

  .gate-notes-row {
    margin-bottom: 12px;
    padding: 10px 14px;
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: 8px;
  }

  .gate-notes-row p {
    font-size: 12.5px;
    color: var(--color-text-secondary);
    margin: 0;
    line-height: 1.5;
  }

  /* Result summary */
  .summary-card {
    background-color: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 10px;
    padding: 20px;
    margin-bottom: 20px;
  }

  .summary-title {
    font-size: 16px;
    font-weight: 600;
    margin: 0 0 6px 0;
  }

  .summary-status {
    font-size: 12.5px;
    color: var(--color-text-secondary);
    margin: 0 0 16px 0;
    line-height: 1.5;
    max-width: 640px;
  }

  .summary-stats {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    margin-bottom: 16px;
  }

  .summary-stat {
    display: flex;
    flex-direction: column;
    gap: 2px;
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 10px 16px;
    min-width: 110px;
  }

  .summary-stat-num {
    font-size: 20px;
    font-weight: 600;
    color: var(--color-accent-text);
    font-variant-numeric: tabular-nums;
  }

  .summary-stat-num--err {
    color: var(--color-danger);
  }

  .summary-stat-label {
    font-size: 11.5px;
    color: var(--color-text-secondary);
  }

  .summary-block {
    border-top: 1px dashed var(--color-border);
    padding: 14px 0;
  }

  .summary-block h3 {
    display: flex;
    align-items: center;
    gap: 7px;
    font-size: 12.5px;
    font-weight: 600;
    margin: 0 0 8px 0;
    color: var(--color-text-primary);
  }

  .summary-block--err h3 {
    color: var(--color-danger);
  }

  .summary-block--warn {
    border-color: color-mix(in srgb, #E8A33D 55%, var(--color-border));
  }

  .summary-block--warn h3 {
    color: #C98A2E;
  }

  .summary-block--ok {
    border-color: color-mix(in srgb, var(--color-accent) 40%, var(--color-border));
  }

  .summary-block--ok h3 {
    color: var(--color-accent-text);
  }

  .summary-extra {
    font-size: 11.5px;
    color: var(--color-text-secondary);
    margin: 8px 0 0 0;
    line-height: 1.5;
  }

  .rp-warn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    color: var(--color-text-secondary);
  }

  .rp-dialog-warn {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin: -12px 0 20px 0;
    padding: 9px 12px;
    border-radius: 6px;
    font-size: 12px;
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 8%, var(--color-surface));
    border: 1px dashed color-mix(in srgb, var(--color-danger) 35%, var(--color-border));
    line-height: 1.5;
  }

  .rp-dialog-warn span {
    flex: 1;
    min-width: 0;
  }

  .rp-dialog-warn :global(svg) {
    flex-shrink: 0;
    margin-top: 1px;
  }

  .restore-check.off {
    opacity: 0.6;
  }

  .summary-block ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .summary-block li {
    font-size: 12px;
    color: var(--color-text-secondary);
    word-break: break-all;
    line-height: 1.4;
  }

  .summary-verify-note {
    font-size: 12.5px;
    color: var(--color-text-primary);
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 10px 12px;
    margin: 10px 0;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .export-note {
    font-size: 12px;
    color: var(--color-accent-text);
    margin: 4px 0 10px 0;
    word-break: break-all;
  }

  .summary-actions {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-top: 14px;
  }

  .summary-primary {
    align-self: flex-start;
    height: 38px;
    padding: 0 18px;
    font-size: 13px;
  }

  .summary-secondary {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
  }

  .summary-nav-link {
    background: none;
    border: none;
    padding: 6px 4px;
    font-size: 12.5px;
    color: var(--color-text-secondary);
    text-decoration: underline;
    text-underline-offset: 3px;
    cursor: pointer;
    border-radius: 6px;
  }

  .summary-nav-link:hover {
    color: var(--color-text-primary);
  }

  .summary-block--skipped {
    border-left: 3px solid var(--color-text-secondary);
  }

  /* Informational only — already-gone must never read as a failure state */
  .summary-gone-note {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--color-text-secondary);
    margin: -6px 0 14px 0;
  }

  .summary-gone-note :global(svg) {
    flex-shrink: 0;
  }

  .skipped-reason {
    display: flex;
    align-items: baseline;
    gap: 8px;
    font-size: 12.5px;
    color: var(--color-text-primary);
    margin: 8px 0 4px 0;
  }

  .skipped-reason-count {
    font-size: 11px;
    font-weight: 600;
    color: var(--color-text-secondary);
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: 5px;
    padding: 1px 6px;
    font-variant-numeric: tabular-nums;
    flex-shrink: 0;
  }

  .locked-hint {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 11.5px;
    font-weight: 500;
    color: var(--color-text-secondary);
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: 9999px;
    padding: 4px 10px;
  }

  /* Hero Stack — multi-app carousel, uses same tokens as single hero */
  .hero-stack {
    display: flex;
    align-items: center;
    gap: 10px;
    background-color: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 10px;
    padding: 12px 14px;
    margin-bottom: 20px;
    overflow-x: auto;
    scrollbar-width: thin;
    -webkit-font-smoothing: antialiased;
  }

  .hero-mini {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border-radius: 8px;
    border: 1px solid var(--color-border);
    background: var(--color-bg);
    flex-shrink: 0;
    min-width: 0;
    max-width: 180px;
    transition: border-color 0.14s ease, background-color 0.14s ease;
  }

  .hero-mini-icon {
    width: 30px; height: 30px; border-radius: 6px; background: var(--color-surface); border: 1px solid var(--color-border);
    display:flex; align-items:center; justify-content:center; flex-shrink:0; overflow:hidden;
  }

  .hero-mini-img { width:22px; height:22px; object-fit:contain; }
  .hero-mini-letter { font-size:12px; font-weight:600; color:var(--color-accent-text); }
  .hero-mini-info { display:flex; flex-direction:column; min-width:0; line-height:1.15; }
  .hero-mini-name { font-size:12.5px; font-weight:600; letter-spacing:-0.01em; white-space:nowrap; overflow:hidden; text-overflow:ellipsis; }
  .hero-mini-version { font-size:10.5px; color:var(--color-text-secondary); }
  .hero-stack-meta { margin-left:auto; display:flex; align-items:center; gap:6px; font-size:12px; color:var(--color-text-secondary); white-space:nowrap; padding-left:8px; border-left:1px solid var(--color-border); }

  /* App Hero Header */
  .app-hero {
    display: flex;
    align-items: center;
    gap: 16px;
    background-color: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 10px;
    padding: 16px 20px;
    margin-bottom: 20px;
  }

  .app-hero-icon {
    width: 44px;
    height: 44px;
    border-radius: 8px;
    background-color: var(--color-bg);
    border: 1px solid var(--color-border);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    overflow: hidden;
  }

  .hero-icon-img {
    width: 32px;
    height: 32px;
    object-fit: contain;
  }

  .hero-icon-letter {
    font-size: 18px;
    font-weight: 600;
    color: var(--color-accent-text);
  }

  .app-hero-details {
    flex: 1;
    min-width: 0;
  }

  .app-hero-title-row {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .app-hero-title-row h1 {
    font-size: 18px;
    font-weight: 600;
    margin: 0;
    letter-spacing: -0.02em;
    color: var(--color-text-primary);
  }

  .version-tag {
    font-size: 11px;
    font-weight: 500;
    padding: 2px 8px;
    background-color: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: 6px;
    color: var(--color-text-secondary);
  }

  .app-hero-meta {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--color-text-secondary);
    margin-top: 4px;
  }

  .meta-dot {
    opacity: 0.4;
  }

  .meta-highlight {
    color: var(--color-accent-text);
    font-weight: 500;
  }

  .meta-reclaim {
    color: var(--color-text-primary);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.6; }
  }

  :global(.progress-root) {
    width: 100%;
    max-width: 300px;
    height: 4px;
    background-color: var(--color-border);
    border-radius: 2px;
    overflow: hidden;
  }

  :global(.progress-indicator) {
    height: 100%;
    background-color: var(--color-accent);
    border-radius: 2px;
    transition: width 0.2s ease;
  }

  .tabs-nav {
    display: flex;
    gap: 4px;
    border-bottom: 1px dashed var(--color-border);
    margin-bottom: 16px;
  }

  .tab-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 16px;
    border: none;
    background: transparent;
    cursor: pointer;
    font-size: 13px;
    color: var(--color-text-secondary);
    border-bottom: 2px solid transparent;
    margin-bottom: -1px;
    transition: color 0.1s ease, border-color 0.1s ease;
  }

  .tab-btn:hover {
    color: var(--color-text-primary);
  }

  .tab-btn.active {
    color: var(--color-accent-text);
    border-bottom-color: var(--color-accent);
    font-weight: 500;
  }

  .tab-badge {
    background-color: var(--color-bg);
    border: 1px solid var(--color-border);
    padding: 2px 7px;
    border-radius: 10px;
    font-size: 11px;
    font-weight: 500;
  }

  .tab-btn.active .tab-badge {
    background-color: var(--color-accent-soft);
    border-color: var(--color-accent-soft);
    color: var(--color-accent-text);
  }

  .tab-btn--readonly {
    font-size: 11.5px;
    opacity: 0.9;
  }

  .readonly-hint {
    font-size: 11px;
    font-weight: 500;
    color: var(--color-text-secondary);
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: 9999px;
    padding: 3px 8px;
    white-space: nowrap;
  }

  .bulk-actions {
    display: flex;
    gap: 8px;
    margin-bottom: 12px;
    align-items: center;
  }

  .bottom-bar {
    position: fixed;
    bottom: 0;
    left: 200px;
    right: 0;
    background-color: var(--color-surface);
    border-top: 1px dashed var(--color-border);
    padding: 10px 28px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    z-index: 40;
  }

  .bottom-left {
    display: flex;
    align-items: center;
    gap: 20px;
    font-size: 13px;
    color: var(--color-text-secondary);
  }

  .bottom-right {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .selected-count {
    font-weight: 500;
    color: var(--color-text-primary);
  }

  .restore-check {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    font-size: 13px;
    font-weight: 500;
    letter-spacing: -0.01em;
    -webkit-font-smoothing: antialiased;
  }

  :global(.dialog-actions) {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  :global(.dialog-overlay) {
    position: fixed;
    inset: 0;
    background-color: rgba(0, 0, 0, 0.4);
    z-index: 50;
    animation: fadeIn 0.1s ease;
  }

  :global(.dialog-content) {
    position: fixed;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 10px;
    padding: 24px;
    width: 400px;
    max-width: 90vw;
    z-index: 51;
    animation: scaleIn 0.15s ease;
  }

  :global(.dialog-title) {
    font-size: 16px;
    font-weight: 600;
    margin: 0 0 8px 0;
  }

  :global(.dialog-desc) {
    font-size: 13px;
    color: var(--color-text-secondary);
    margin: 0 0 20px 0;
    line-height: 1.5;
  }

  :global(.ai-dialog) {
    width: 480px;
    display: flex;
    flex-direction: column;
    gap: 0;
  }

  .ai-dialog-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 12px;
  }

  .ai-header-right {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  :global(.ai-close) {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border-radius: 6px;
    border: 1px solid var(--color-border);
    background: transparent;
    color: var(--color-text-secondary);
    cursor: pointer;
    transition: background-color 0.12s ease, color 0.12s ease;
  }

  :global(.ai-close:hover) {
    background: var(--color-bg);
    color: var(--color-text-primary);
  }

  :global(.ai-dialog .dialog-title) {
    margin: 0;
  }

  .ai-nav-label {
    font-size: 12px;
    font-weight: 500;
    color: var(--color-text-secondary);
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: 6px;
    padding: 2px 8px;
  }

  .ai-path {
    font-size: 11px;
    color: var(--color-text-secondary);
    word-break: break-all;
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: 6px;
    padding: 8px 10px;
    margin: 0 0 12px 0;
  }

  .ai-loading {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 28px 12px;
    color: var(--color-accent-text);
    background: var(--color-bg);
    border: 1px dashed var(--color-border);
    border-radius: 8px;
  }

  .ai-loading p {
    font-size: 13px;
    color: var(--color-text-secondary);
    margin: 0;
  }

  .ai-loading-hint {
    font-size: 11px;
    color: var(--color-text-secondary);
    opacity: 0.7;
  }

  .ai-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 10px;
    padding: 20px 12px;
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: 8px;
  }

  .ai-empty-icon {
    width: 44px;
    height: 44px;
    border-radius: 10px;
    background: var(--color-accent-soft);
    color: var(--color-accent);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .ai-empty-icon--error {
    background: color-mix(in srgb, var(--color-danger) 12%, var(--color-surface));
    color: var(--color-danger);
  }

  .ai-empty-title {
    font-size: 14px;
    font-weight: 600;
    margin: 0;
    color: var(--color-text-primary);
  }

  .ai-empty-desc {
    font-size: 12px;
    line-height: 1.5;
    color: var(--color-text-secondary);
    margin: 0;
    max-width: 400px;
  }

  .ai-empty-detail {
    font-size: 10px;
    line-height: 1.4;
    color: var(--color-text-secondary);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 6px;
    padding: 6px 8px;
    max-width: 100%;
    word-break: break-all;
    opacity: 0.8;
  }

  .ai-empty-actions {
    display: flex;
    gap: 8px;
    margin-top: 6px;
  }

  .ai-body {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .ai-assessment {
    font-size: 13.5px;
    line-height: 1.6;
    white-space: pre-wrap;
    margin: 0;
    color: var(--color-text-primary);
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 12px 14px;
  }

  .ai-meta {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }

  .ai-chip {
    font-size: 11px;
    font-weight: 600;
    background: var(--color-accent-soft);
    color: var(--color-accent-text);
    border: 1px dashed color-mix(in srgb, var(--color-accent) 35%, var(--color-border));
    padding: 4px 10px;
    border-radius: 20px;
    letter-spacing: 0.02em;
  }

  .ai-chip--high {
    background: color-mix(in srgb, var(--color-accent) 14%, var(--color-surface));
    color: color-mix(in srgb, var(--color-accent-text) 85%, #000000);
    border-color: color-mix(in srgb, var(--color-accent) 40%, var(--color-border));
  }

  .ai-chip--low {
    background: var(--color-surface);
    color: var(--color-text-secondary);
    border-color: var(--color-border);
  }

  .ai-chip--delete {
    background: color-mix(in srgb, var(--color-danger) 12%, var(--color-surface));
    color: var(--color-danger);
    border-color: color-mix(in srgb, var(--color-danger) 35%, var(--color-border));
  }

  .ai-chip--keep {
    background: color-mix(in srgb, var(--color-accent) 14%, var(--color-surface));
    color: color-mix(in srgb, var(--color-accent-text) 85%, #000000);
    border-color: color-mix(in srgb, var(--color-accent) 40%, var(--color-border));
  }

  .ai-disclaimer {
    font-size: 11px;
    color: var(--color-text-secondary);
    font-style: italic;
    margin: 0;
  }

  .ai-dialog-actions {
    margin-top: 20px;
    padding-top: 16px;
    border-top: 1px solid var(--color-border);
    width: 100%;
  }

  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  @keyframes scaleIn {
    from { opacity: 0; transform: translate(-50%, -50%) scale(0.95); }
    to { opacity: 1; transform: translate(-50%, -50%) scale(1); }
  }

  :global(.spin) {
    animation: spin 0.85s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }
</style>
