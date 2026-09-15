<script lang="ts">
  import { onMount, onDestroy } from 'svelte'
  import { scanLeftovers, deleteItems, askAi, runUninstaller, exportReportJson, exportReportTxt, previewUninstall, registryImpact } from '../lib/tauri-api'
  import type { AppInfo, ScanResult, LeftoverItem, AiResponse, DeleteResult, SkippedItem, AttentionItem } from '../lib/types'
  import { formatSize } from '../lib/utils'
  import { getSettings, aiReady } from '../lib/stores/settings.svelte'
  import { setPendingCleanup, clearPendingCleanup, type PendingCleanup } from '../lib/stores/pending.svelte'
  import LeftoverTable from '../lib/components/LeftoverTable.svelte'
  import UninstallPreviewList from '../lib/components/UninstallPreviewList.svelte'
  import RegistryImpactList from '../lib/components/RegistryImpactList.svelte'
  import { errText, mergeList, isOverridableRegistryPath, needsTypedConfirm } from '../lib/stores/review-helpers'
  import { Trash2, Scan, ChevronRight, MessageCircle, Loader2, Globe, Clock, Check, ShieldCheck, ShieldAlert, XCircle, X } from '@lucide/svelte'
  import { Checkbox, Dialog, Progress } from 'bits-ui'
  import { listen } from '@tauri-apps/api/event'
  import { invoke } from '@tauri-apps/api/core'

  let {
    app,
    apps = [],
    onBack,
    restore = null
  }: {
    app: AppInfo
    apps?: AppInfo[]
    onBack: () => void
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
  // Delete failures keep the review (selection intact) and banner the error.
  // Routing them through scanError showed the wrong screen and lost context.
  let deleteError = $state(false)
  let deleteErrorMsg = $state('')
  // Elevation awareness — restore points need admin; surface it before delete
  // instead of failing silently mid-operation.
  let isAdmin = $state(true)
  let adminKnown = $state(false)
  let deleting = $state(false)
  let activeTab: 'files' | 'registry' | 'services' | 'startup' | 'hosts' | 'tasks' = $state('files')
  let showDeleteDialog = $state(false)
  let showProtectDialog = $state(false)
  let scanProgress = $state(0)
  let scanStage = $state('')
  let deleteProgress = $state(0)
  let deleteStage = $state('')
  let deleteCurrent = $state(0)
  let deleteTotal = $state(0)
  let deletePath = $state('')
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

  // --- Delete state -----------------------------------------------------------
  interface DeletePayload {
    deleted: number
    skipped: number
    skippedItems: SkippedItem[]
    alreadyGone: number
    alreadyGonePaths: string[]
    attentionItems: AttentionItem[]
    deleted_ids: string[]
    errors: string[]
    restorePointOk: boolean
    restorePointError: string | null
    scanSnapshot: ScanResult
  }
  let deletePayload: DeletePayload | null = $state(null)
  let exportInfo = $state('')

  // Summary scenario — drives the headline and which sections make sense:
  // clean (all removed) / partial (some rule-skipped) / errors (failures) /
  // gone (nothing left to do) / nothing (nothing happened).
  type SummaryOutcome = 'clean' | 'partial' | 'errors' | 'gone' | 'nothing'
  let summaryOutcome = $derived.by<SummaryOutcome>(() => {
    if (!deletePayload) return 'nothing'
    const { deleted, attentionItems, skipped, alreadyGone, errors } = deletePayload
    if (errors.length > 0 || attentionItems.length > 0) return 'errors'
    if (deleted === 0) {
      if (skipped > 0) return 'partial'
      if (alreadyGone > 0) return 'gone'
      return 'nothing'
    }
    if (skipped > 0) return 'partial'
    return 'clean'
  })

  // Distinct skip reasons with their paths, rendered as grouped bullets.
  let skippedGroups = $derived.by(() => {
    if (!deletePayload) return [] as { reason: string; paths: string[] }[]
    const map = new Map<string, string[]>()
    for (const s of deletePayload.skippedItems) {
      if ((map.get(s.reason)?.length ?? 0) >= 50) continue
      map.set(s.reason, [...(map.get(s.reason) ?? []), s.path])
    }
    return [...map.entries()].map(([reason, paths]) => ({ reason, paths }))
  })

  // Backend skip reasons read like log lines. Map the known ones to plain
  // words; unknown reasons print raw so nothing ever hides.
  function humanSkipReason(reason: string): string {
    const r = reason.toLowerCase()
    if (r.includes('needs your approval')) return 'Protected registry key. Approve it in the delete dialog to remove it.'
    if (r.includes('protected windows registry')) return 'Sits inside a protected Windows key. ClearOut left it alone.'
    if (r.includes('protected windows')) return 'Too close to Windows. ClearOut left it alone.'
    return reason
  }

  let summaryHeadline = $derived.by(() => {
    if (!deletePayload) return ''
    const { deleted, skipped, alreadyGone, attentionItems } = deletePayload
    const needAttention = attentionItems.length
    switch (summaryOutcome) {
      case 'errors':
        if (deleted === 0) return `Finished with problems: ${needAttention} need attention`
        return `Finished with problems: ${deleted} removed, ${needAttention} need attention`
      case 'partial':
        if (deleted === 0) return `Nothing removed — ${skipped} left in place`
        return `${deleted} removed, ${skipped} left in place`
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
        return 'Some items stayed behind. Check the list below, restart if asked, then scan again.'
      case 'partial':
        return 'These sit too close to Windows itself, so ClearOut left them alone.'
      case 'gone':
        return 'The selected items were already gone. Nothing left to do.'
      case 'nothing':
        return 'You removed nothing.'
      default:
        return 'You permanently removed every selected leftover.'
    }
  })

  let aiEnabled = $derived(aiReady())
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

  function isReadOnlyType(t: LeftoverItem['item_type']) {
    return t === 'Hosts' || t === 'Task'
  }

  // Mirrors the backend overridable guard: leaf app keys nested under a
  // protected parent (Uninstall\{App}, Run\{App}). Parents themselves,
  // files, and services never qualify — no override path exists for them.
  function isOverridableRegistry(item: LeftoverItem): boolean {
    if (item.item_type !== 'Registry') return false
    return isOverridableRegistryPath(item.path)
  }

  // Selected registry leaves sitting in protected Windows areas. Deleting
  // them needs the dialog checkbox; without consent they skip as before.
  let protectedSelected = $derived(allItems.filter(i => selectedItems.has(i.id) && isOverridableRegistry(i)))
  let protectConsent = $state(false)
  // ponytail: dialog state beats new store; ceiling is local state per review, upgrade is shared delete store.
  let allowWithoutRestore = $state(false)
  let typedDelete = $state('')
  let needsTyped = $derived(needsTypedConfirm(selectedCount))
  // ponytail: pagination beats virtualization dependency; ceiling is 100 rows per page, upgrade is virtual scroll.
  let reviewPage = $state(0)
  const REVIEW_PAGE_SIZE = 100
  let pageItems = $derived(currentItems.slice(reviewPage * REVIEW_PAGE_SIZE, (reviewPage + 1) * REVIEW_PAGE_SIZE))
  let pageCount = $derived(Math.max(1, Math.ceil(currentItems.length / REVIEW_PAGE_SIZE)))
  $effect(() => {
    activeTab
    reviewPage = 0
  })
  let uninstallPreviews = $state(new Map<string, { program: string; args: string[]; is_msi: boolean }>())
  let registryImpacts = $state(new Map<string, { keys: number; values: number }>())

  // Items under the install dir of an app that is still installed → protected.
  // Files match by install path; registry/services/startup match by
  // associated app so nothing deletable survives while its app is installed.
  let lockedIds = $derived.by(() => {
    const locked = new Set<string>()
    if (!scanResult) return locked
    const stillInstalled = displayApps.filter(a => !confirmedUninstalled.has(a.id))
    if (stillInstalled.length === 0) return locked
    const installedNames = new Set(stillInstalled.map(a => a.name))
    for (const a of stillInstalled) {
      const loc = (a.install_location ?? '').trim().replace(/^"+|"+$/g, '')
      if (!loc) continue
      const locLower = loc.toLowerCase()
      for (const it of scanResult.files) {
        const p = it.path.toLowerCase()
        if (p === locLower || p.startsWith(locLower + '\\')) locked.add(it.id)
      }
    }
    for (const list of [scanResult.registry, scanResult.services, scanResult.startup]) {
      for (const it of list) {
        if (installedNames.has(it.associated_app)) locked.add(it.id)
      }
    }
    return locked
  })

  function toggleSelectAll() {
    const selectable = currentItems.filter(i => !lockedIds.has(i.id) && !isReadOnlyType(i.item_type))
    if (selectable.length > 0 && selectable.every((item: LeftoverItem) => selectedItems.has(item.id))) {
      selectable.forEach((item: LeftoverItem) => selectedItems.delete(item.id))
    } else {
      selectable.forEach((item: LeftoverItem) => selectedItems.add(item.id))
    }
    selectedItems = new Set(selectedItems)
  }

  function toggleItem(id: string) {
    if (lockedIds.has(id)) return
    const target = allItems.find(i => i.id === id)
    if (target && isReadOnlyType(target.item_type)) return
    if (selectedItems.has(id)) {
      selectedItems.delete(id)
    } else {
      selectedItems.add(id)
    }
    selectedItems = new Set(selectedItems)
  }

  // --- Uninstall gate actions ------------------------------------------------
  async function runUninstallers(targets?: AppInfo[]) {
    const apps = targets ?? displayApps
    removalScan = false
    failedApps = []
    scanning = true
    scanStage = apps.length > 1 ? 'Running native uninstallers…' : 'Running native uninstaller…'
    gateNotes = []
    uninstallPreviews = new Map()
    for (let i = 0; i < apps.length; i++) {
      const a = apps[i]
      const us = (a.uninstall_string ?? '').trim()
      if (!us) {
        confirmedUninstalled.add(a.id)
        gateNotes.push(`${a.name} has no uninstaller. Its installed files are included in this scan.`)
        continue
      }
      // ponytail: preview-then-run beats blind execute; ceiling is parsed command text, upgrade is signature check.
      try {
        const prev = await previewUninstall(us)
        uninstallPreviews.set(a.id, prev)
        uninstallPreviews = new Map(uninstallPreviews)
        gateNotes.push(`${a.name} by ${a.publisher ?? 'unknown publisher'}: ${prev.program} ${prev.args.join(' ')}${prev.is_msi ? ' [MSI]' : ''}`)
      } catch (e) {
        gateNotes.push(`${a.name}: preview failed — ${errText(e)}`)
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
          gateNotes.push(`${a.name} exited with code ${code} and might still be installed.`)
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
    gateNotes.push('Skipping the uninstallers. Leftover files are listed for your review, including files in folders that still exist.')
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
      `${appName} might still be running. Close it first, or turn on force-close in Settings.`,
    ]
    startScan()
  }

  // --- AI ---------------------------------------------------------------------
  async function handleAnalyzeSelected() {
    if (selectedItems.size === 0 || aiAnalyzing) return
    const cfg = getSettings()
    if (!aiReady()) return

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
        const res = await askAi(item.path, name, item.item_type, item.associated_app, cfg.apiKey, cfg.aiProvider, cfg.aiEndpoint, cfg.aiModel)
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
    if (!aiReady()) return
    if (item.item_type === 'Hosts' || item.item_type === 'Task') return
    aiSelectedItemIds = [item.id]
    aiCurrentIndex = 0
    aiResults = new Map()
    aiErrors = new Map()
    aiAnalyzing = true
    showAiDialog = true
    try {
      const name = item.path.split('\\').pop() || item.path.split('/').pop() || item.path
      const res = await askAi(item.path, name, item.item_type, item.associated_app, cfg.apiKey, cfg.aiProvider, cfg.aiEndpoint, cfg.aiModel)
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
      const res = await askAi(aiCurrentItem.path, name, aiCurrentItem.item_type, aiCurrentItem.associated_app, cfg.apiKey, cfg.aiProvider, cfg.aiEndpoint, cfg.aiModel)
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
  // Step 1: main confirm. Selections holding protected registry leaves route
  // to a second popup instead of cramming the warning into this dialog.
  async function handleDeleteStep1() {
    showDeleteDialog = false
    typedDelete = ''
    // ponytail: live count beats static guess; ceiling is preview-time count, upgrade is snapshot diff.
    for (const i of protectedSelected.slice(0, 10)) {
      if (registryImpacts.has(i.id)) continue
      try {
        const impact = await registryImpact(i.path)
        registryImpacts.set(i.id, impact)
      } catch {
        registryImpacts.set(i.id, { keys: 0, values: 0 })
      }
    }
    registryImpacts = new Map(registryImpacts)
    if (protectedSelected.length > 0) {
      protectConsent = false
      showProtectDialog = true
      return
    }
    handleDelete()
  }

  async function handleDelete() {
    showDeleteDialog = false
    showProtectDialog = false
    // Dialog.Close fires per click; a fast double-click would run the
    // whole destructive pass twice without this guard.
    if (deleting) return
    if (selectedItems.size === 0) return
    deleteError = false
    deleteErrorMsg = ''

      const itemsToDelete = allItems.filter((item: LeftoverItem) => selectedItems.has(item.id))
      // Override consent covers only the protected leaves listed in the
      // dialog. Without the checkbox they stay selected but skip server-side.
      const forceIds = protectConsent ? protectedSelected.map(i => i.id) : []
    deleting = true
    deleteProgress = 0
    deleteStage = 'Preparing...'
    deleteCurrent = 0
    // Total comes from backend progress events (files + tops), not the
    // selection count — seeding from selection made the bar jump mid-run.
    deleteTotal = 0
    deletePath = ''
    const deleteStartedAt = Date.now()

    try {
      deleteUnsub = await listen<any>('delete-progress', (event) => {
        const { stage, current, total, percent, path } = event.payload
        deleteProgress = percent
        deleteCurrent = current
        deleteTotal = total
        if (typeof path === 'string' && path.length > 0) deletePath = path
        if (stage === 'start') deleteStage = 'Starting...'
        else if (stage === 'restore-point') deleteStage = 'Creating restore point...'
        else if (stage === 'files') deleteStage = 'Deleting files permanently...'
        else if (stage === 'registry') deleteStage = 'Removing registry entries...'
        else if (stage === 'services') deleteStage = 'Removing services...'
        else if (stage === 'startup') deleteStage = 'Removing startup entries...'
        else if (stage === 'hosts') deleteStage = 'Checking hosts (read-only)...'
        else if (stage === 'tasks') deleteStage = 'Checking tasks (read-only)...'
        else if (stage === 'done') deleteStage = 'Done'
      })

      const result = await deleteItems(itemsToDelete, createRestorePoint, getSettings().forceKillAllowed, forceIds, allowWithoutRestore)
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

      deletePayload = {
        deleted: result.deleted,
        skipped: result.skipped,
        skippedItems: result.skipped_items ?? [],
        alreadyGone: result.already_gone ?? 0,
        alreadyGonePaths: result.already_gone_paths ?? [],
        attentionItems: result.attention_items ?? [],
        deleted_ids: result.deleted_ids,
        errors: result.errors,
        restorePointOk: result.restore_point_ok,
        restorePointError: result.restore_point_error ?? null,
        scanSnapshot: scanResult!,
      }
      // The pending cleanup has been acted on — drop the Dashboard card.
      clearPendingCleanup()
      selectedItems = new Set()
      phase = 'summary'
      doExport()
    } catch (e) {
      console.error('Delete failed:', e)
      deleteError = true
      deleteErrorMsg = `Delete failed: ${errText(e)}`
    } finally {
      deleting = false
      protectConsent = false
      typedDelete = ''
      allowWithoutRestore = false
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
      already_gone_paths: deletePayload.alreadyGonePaths,
      attention_items: deletePayload.attentionItems,
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

  // --- Scan --------------------------------------------------------------------
  async function startScan() {
    if (displayApps.length === 0) return
    scanning = true
    scanError = false
    scanErrorMsg = ''
    deleteError = false
    deleteErrorMsg = ''
    // Fresh selection per scan: ids from a previous result would otherwise
    // leak into the new dialog count. Leaving summary phase here so a scan
    // error from summary lands on the error screen, not a blank page.
    selectedItems = new Set()
    deletePayload = null
    phase = 'review'
    scanProgress = 0
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
        `${names} has no uninstaller. ClearOut scanned all of its files for your review. ` +
        'You approve every deletion. File deletion is permanent.'
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
      {deleteTotal > 0 ? `${deleteCurrent} / ${deleteTotal} • ${deleteProgress}%` : `${deleteProgress}%`}
    </p>
    {#if deletePath}
      <p class="scan-path font-mono">{deletePath}</p>
    {/if}
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
            <span class="meta-highlight">Step 1 of 2: Uninstall</span>
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
            The uninstaller didn’t finish. Retry it, or scan its traces and remove them yourself.
          {:else if isMulti}
            {displayApps.length} apps are still installed. Run each uninstaller now.
          {:else}
            Uninstall {app.name} first.
          {/if}
        </p>
        {#if gateNotes.length > 0 || uninstallPreviews.size > 0}
          <UninstallPreviewList previews={uninstallPreviews} notes={gateNotes} />
        {/if}
        <div class="gate-actions">
          {#if uninstallFailed}
            <button class="btn-neo btn-neo--delete" onclick={() => runUninstallers(failedApps)}>
              <Trash2 size={13} strokeWidth={1.75} />
              Try the uninstaller again
            </button>
            <button class="btn-secondary" onclick={() => scanTracesDirectly(
              isMulti ? `${displayApps.length} queued apps` : app.name,
              'The uninstaller didn’t finish, so ClearOut scans its traces directly. Review everything before you delete anything.'
            )}>
              Scan traces without the uninstaller
            </button>
          {:else}
            <button class="btn-neo btn-neo--delete" onclick={() => runUninstallers()}>
              <Trash2 size={13} strokeWidth={1.75} />
              {isMulti ? `Uninstall ${displayApps.length} apps` : `Uninstall ${app.name}`}
            </button>
          {/if}
        </div>
        <button class="summary-nav-link gate-fallback" onclick={skipUninstallers}>
          App already gone? Scan leftovers instead
        </button>
        <p class="gate-hint">You review and confirm before anything gets deleted.</p>
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

        {#if deletePayload.restorePointError}
          <div class="terminal-panel">
            <div class="terminal-bar">
              <span class="terminal-dot terminal-dot--close"></span>
              <span class="terminal-dot terminal-dot--min"></span>
              <span class="terminal-dot terminal-dot--max"></span>
            </div>
            <div class="terminal-body">
              <h3 class="terminal-title"><ShieldAlert size={13} /> Restore point was not created</h3>
              <div class="terminal-paths">
                <div class="terminal-line">
                  <span class="terminal-prompt">$</span>
                  <span class="terminal-path">{deletePayload.restorePointError}</span>
                </div>
              </div>
              <p class="skipped-reason">Relaunch as administrator and turn on System Protection for restore points.</p>
            </div>
          </div>
        {:else if deletePayload.restorePointOk}
          <div class="terminal-panel">
            <div class="terminal-bar">
              <span class="terminal-dot terminal-dot--close"></span>
              <span class="terminal-dot terminal-dot--min"></span>
              <span class="terminal-dot terminal-dot--max"></span>
            </div>
            <div class="terminal-body">
              <h3 class="terminal-title"><ShieldCheck size={13} /> Restore point created</h3>
            </div>
          </div>
        {/if}

        {#if deletePayload.attentionItems.length > 0}
          <div class="terminal-panel">
            <div class="terminal-bar">
              <span class="terminal-dot terminal-dot--close"></span>
              <span class="terminal-dot terminal-dot--min"></span>
              <span class="terminal-dot terminal-dot--max"></span>
            </div>
            <div class="terminal-body">
              <h3 class="terminal-title"><XCircle size={13} /> Needs attention — {deletePayload.attentionItems.length}</h3>
              <p class="skipped-reason">Only exceptions appear here. The rest is gone for good.</p>
              <div class="terminal-paths">
                {#each deletePayload.attentionItems.slice(0, 50) as a (a.path + '|' + a.status + '|' + a.reason)}
                  <div class="terminal-line">
                    <span class="terminal-prompt">$</span>
                    <span class="terminal-path">{a.path} — {a.reason} — {a.action} [{a.status}]</span>
                  </div>
                {/each}
              </div>
              {#if deletePayload.attentionItems.length > 50}
                <p class="skipped-reason">…and {deletePayload.attentionItems.length - 50} more. The full list is in the History report.</p>
              {/if}
            </div>
          </div>
        {:else if deletePayload.skipped === 0 && deletePayload.alreadyGone === 0}
          <div class="terminal-panel">
            <div class="terminal-bar">
              <span class="terminal-dot terminal-dot--close"></span>
              <span class="terminal-dot terminal-dot--min"></span>
              <span class="terminal-dot terminal-dot--max"></span>
            </div>
            <div class="terminal-body">
              <h3 class="terminal-title"><ShieldCheck size={13} /> Nothing needs attention</h3>
              <div class="terminal-paths">
                <div class="terminal-line">
                  <span class="terminal-prompt">$</span>
                  <span class="terminal-path">All selected items are gone.</span>
                </div>
              </div>
            </div>
          </div>
        {/if}

        {#if deletePayload.alreadyGone > 0}
          <div class="terminal-panel">
            <div class="terminal-bar">
              <span class="terminal-dot terminal-dot--close"></span>
              <span class="terminal-dot terminal-dot--min"></span>
              <span class="terminal-dot terminal-dot--max"></span>
            </div>
            <div class="terminal-body">
              <h3 class="terminal-title"><Clock size={13} /> Already gone — {deletePayload.alreadyGone}</h3>
              <p class="skipped-reason">These vanished before deletion. Nothing needed doing.</p>
              <div class="terminal-paths">
                {#each deletePayload.alreadyGonePaths.slice(0, 50) as p (p)}
                  <div class="terminal-line">
                    <span class="terminal-prompt">$</span>
                    <span class="terminal-path">{p}</span>
                  </div>
                {/each}
              </div>
              {#if deletePayload.alreadyGonePaths.length > 50}
                <p class="skipped-reason">…and {deletePayload.alreadyGonePaths.length - 50} more. The full list is in the History report.</p>
              {/if}
            </div>
          </div>
        {/if}

        {#if deletePayload.skipped > 0}
          {#each skippedGroups as group (group.reason)}
            <div class="terminal-panel">
                <div class="terminal-bar">
                  <span class="terminal-dot terminal-dot--close"></span>
                  <span class="terminal-dot terminal-dot--min"></span>
                  <span class="terminal-dot terminal-dot--max"></span>
                </div>
                <div class="terminal-body">
                  <h3 class="terminal-title"><Clock size={13} /> Left in place — {deletePayload.skipped}</h3>
                  <p class="skipped-reason">{humanSkipReason(group.reason)}</p>
                  <div class="terminal-paths">
                    {#each group.paths as p (p)}
                      <div class="terminal-line">
                        <span class="terminal-prompt">$</span>
                        <span class="terminal-path">{p}</span>
                      </div>
                    {/each}
                  </div>
                </div>
              </div>
            {/each}
        {/if}

        <div class="summary-actions">
          <div class="summary-secondary">
            <button class="btn-secondary" onclick={startScan}>
              <Scan size={13} strokeWidth={1.75} />
              Run another scan
            </button>
            <button class="summary-nav-link" onclick={onBack}>Back to apps</button>
          </div>
          {#if exportInfo}
            <p class="export-note" title="Reports are saved to %APPDATA%\ClearOut\reports">{exportInfo}</p>
          {/if}
        </div>
      </div>
    {/if}
  </div>
{:else if scanError}
  <div class="scanning-only">
    <div class="scan-icon scan-icon--error">
      <Scan size={24} />
    </div>
    <p class="scan-label">Scan failed</p>
    <p class="scan-hint">ClearOut couldn't read leftovers for this app.</p>
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

    {#if deleteError}
      <div class="summary-block summary-block--err" role="alert">
        <h3><XCircle size={13} /> Delete failed</h3>
        <ul>
          <li class="font-mono">{deleteErrorMsg}</li>
        </ul>
        <p class="summary-extra">Your selection is intact. Fix the cause and try again.</p>
      </div>
    {/if}

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
              <span class="meta-reclaim">Frees {formatSize(totalReclaim)}</span>
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
            <span class="meta-highlight">{removalScan ? 'Removal review: app still installed' : 'Leftover review'}</span>
            {#if scanResult}
              {@const totalReclaim = [...scanResult.files, ...scanResult.registry, ...scanResult.services, ...scanResult.startup, ...(scanResult.hosts ?? []), ...(scanResult.tasks ?? [])].reduce((s, i) => s + (i.size ?? 0), 0)}
              {#if totalReclaim > 0}
                <span class="meta-dot">&bull;</span>
                <span class="meta-reclaim">Frees {formatSize(totalReclaim)}</span>
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
      <button class="btn-neo btn-neo--ai" onclick={toggleSelectAll} disabled={currentItems.every((i: LeftoverItem) => lockedIds.has(i.id) || isReadOnlyType(i.item_type))} aria-label="Toggle select tab items">
        {currentItems.length > 0 && currentItems.every((item: LeftoverItem) => selectedItems.has(item.id) || lockedIds.has(item.id)) ? 'Deselect Tab' : 'Select Tab'}
      </button>
      {#if lockedIds.size > 0}
        <span class="locked-hint">Shield {lockedIds.size}: still installed. Run the uninstaller first</span>
      {/if}
    </div>

    <LeftoverTable items={pageItems} {selectedItems} onSelect={toggleItem} onAnalyzeSingle={handleAnalyzeSingle} lockedIds={lockedIds} />
    {#if pageCount > 1}
      <div class="bulk-actions" aria-label="Review pagination">
        <button class="btn-secondary" disabled={reviewPage === 0} onclick={() => reviewPage--}>Prev</button>
        <span class="locked-hint">Page {reviewPage + 1} of {pageCount} — {currentItems.length} items</span>
        <button class="btn-secondary" disabled={reviewPage + 1 >= pageCount} onclick={() => reviewPage++}>Next</button>
      </div>
    {/if}

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
          <span class="rp-warn"><ShieldAlert size={12} strokeWidth={1.75} /> Restore point needs admin. Turned off</span>
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
        <button class="btn-neo btn-neo--delete" disabled={selectedCount === 0 || deleting} onclick={() => { protectConsent = false; typedDelete = ''; allowWithoutRestore = false; showDeleteDialog = true }} aria-label="Delete {selectedCount} selected items">
          <Trash2 size={13} strokeWidth={1.75} />
          {deleting ? 'Deleting…' : 'Delete Selected'}
        </button>
      {#if (activeTab === 'hosts' || activeTab === 'tasks') && currentItems.length > 0}
        <span class="readonly-hint">Read-only tab. Review only</span>
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
    <p class="scan-hint">No leftovers found.</p>
    <button class="btn-secondary" onclick={onBack}>Back to Apps</button>
  </div>
{/if}

<Dialog.Root bind:open={showDeleteDialog}>
  <Dialog.Portal>
    <Dialog.Overlay class="dialog-overlay" />
    <Dialog.Content class="dialog-content">
      <Dialog.Title class="dialog-title">Confirm Permanent Delete</Dialog.Title>
      <Dialog.Description class="dialog-desc">
        Delete {selectedCount} selected {selectedCount === 1 ? 'item' : 'items'} permanently?

        No undo. Files skip recycle bin. Locked files go after restart. Registry plus startup backed up first.
        {#if createRestorePoint}Restore point first. Deletion blocks when creation fails unless allowed below.{/if}
      </Dialog.Description>
      {#if adminKnown && !isAdmin}
        <div class="rp-dialog-warn" role="note">
          <ShieldAlert size={13} strokeWidth={1.75} />
          <span>
            <strong>You launched without admin rights</strong><br />
            Restore points and some service removals stay unavailable.
          </span>
        </div>
      {/if}
      {#if createRestorePoint}
        <label class="restore-check">
          <Checkbox.Root
            bind:checked={allowWithoutRestore}
            class="checkbox-root"
            aria-label="Allow deletion when restore point fails"
          >
            <span class="checkbox-indicator">
              <Check size={11} strokeWidth={2.6} />
            </span>
          </Checkbox.Root>
          <span>Allow delete when restore point fails (not recommended)</span>
        </label>
      {/if}
      {#if needsTyped}
        <label class="restore-check" for="bulk-delete-input">Type <span class="font-mono">DELETE</span> to confirm {selectedCount} items</label>
        <input
          id="bulk-delete-input"
          class="confirm-input font-mono"
          placeholder="DELETE"
          bind:value={typedDelete}
          autocomplete="off"
        />
      {/if}
      <div class="dialog-actions">
        <Dialog.Close class="btn-secondary">Cancel</Dialog.Close>
        <Dialog.Close class="btn-delete" disabled={needsTyped && typedDelete.trim() !== 'DELETE'} onclick={handleDeleteStep1}>
          <Trash2 size={14} />
          Delete
        </Dialog.Close>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

<Dialog.Root bind:open={showProtectDialog}>
  <Dialog.Portal>
    <Dialog.Overlay class="dialog-overlay" />
    <Dialog.Content class="dialog-content">
      <Dialog.Title class="dialog-title">Protected registry keys</Dialog.Title>
      <Dialog.Description class="dialog-desc">
        {protectedSelected.length} selected {protectedSelected.length === 1 ? 'key sits' : 'keys sit'} in a Windows area.
        Deleting the wrong key breaks apps or login. ClearOut backs them up first, and History can restore them.
      </Dialog.Description>
      <RegistryImpactList items={protectedSelected} impacts={registryImpacts} />
      <label class="restore-check">
        <Checkbox.Root
          bind:checked={protectConsent}
          class="checkbox-root"
          aria-label="Delete protected registry keys too"
        >
          <span class="checkbox-indicator">
            <Check size={11} strokeWidth={2.6} />
          </span>
        </Checkbox.Root>
        <span>I understand — delete protected keys too</span>
      </label>
      <div class="dialog-actions">
        <Dialog.Close class="btn-secondary" onclick={() => { showProtectDialog = false; showDeleteDialog = true }}>Back</Dialog.Close>
        <Dialog.Close class="btn-delete" disabled={!protectConsent} onclick={handleDelete}>
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
              Your API key looks invalid or out of credit. Check it in Settings under {getSettings().aiProvider}.
            {:else if aiCurrentError.includes('rate') || aiCurrentError.includes('429')}
              The provider rate-limited you. Wait 30 seconds and try again.
            {:else if aiCurrentError.includes('no content')}
              The AI sent back an empty reply. This is usually a temporary provider hiccup. Try again.
            {:else}
              Something went wrong with this item. Retry it or move on.
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
            The AI sent back no usable text. This is usually temporary. Try again.
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
          <p class="ai-disclaimer">AI advice only. Review before you delete.</p>
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

  .gate-fallback {
    margin-top: 10px;
    padding-left: 0;
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

  .summary-block {
    border-top: 1px dashed var(--color-border);
    padding: 16px 0;
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
    margin: 6px 0 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 5px;
  }

  .summary-block li {
    font-size: 12px;
    color: var(--color-text-secondary);
    word-break: break-all;
    line-height: 1.5;
  }

  .export-note {
    font-size: 12px;
    color: var(--color-accent-text);
    margin: 0;
    text-align: right;
    word-break: break-all;
  }

  .summary-actions {
    display: flex;
    flex-direction: row;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    margin-top: 20px;
    padding-top: 16px;
    border-top: 1px dashed var(--color-border);
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

  .skipped-reason {
    font-size: 12.5px;
    color: var(--color-text-primary);
    margin: 0;
    font-weight: 600;
    line-height: 1.5;
  }

  /* ── Terminal panel ───────────────────────────────────────── */
  .terminal-panel {
    border-radius: 8px;
    border: 1px solid var(--color-border);
    overflow: hidden;
    font-family: var(--font-mono);
    font-size: 11.5px;
  }

  /* Dots-only dark titlebar */
  .terminal-bar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 12px;
    background: color-mix(in srgb, var(--color-bg) 50%, #000 50%);
    border-bottom: 1px solid var(--color-border);
  }

  .terminal-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .terminal-dot--close { background: #FF5F57; }
  .terminal-dot--min   { background: #FEBC2E; }
  .terminal-dot--max   { background: #28C840; }

  /* Clean content body */
  .terminal-body {
    background: var(--color-bg);
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .terminal-title {
    margin: 0 0 2px 0;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--color-text-primary);
    display: flex;
    align-items: center;
    gap: 7px;
    border: none !important;
    padding: 0 !important;
  }

  .terminal-paths {
    display: flex;
    flex-direction: column;
    gap: 3px;
    margin-top: 6px;
    padding-top: 10px;
    border-top: 1px dashed var(--color-border);
    max-height: 160px;
    overflow-y: auto;
    scrollbar-width: thin;
    scrollbar-color: var(--color-border) transparent;
  }

  .terminal-line {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  .terminal-prompt {
    color: var(--color-accent-text);
    font-weight: 600;
    user-select: none;
    flex-shrink: 0;
  }

  .terminal-path {
    color: var(--color-text-secondary);
    word-break: break-all;
    line-height: 1.5;
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

  .scan-path {
    font-size: 11px;
    color: var(--color-text-secondary);
    max-width: 420px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    direction: rtl;
    text-align: left;
    margin: 0;
  }

</style>
