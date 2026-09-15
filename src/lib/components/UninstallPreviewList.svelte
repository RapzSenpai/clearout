<script lang="ts">
  let { previews, notes }: {
    previews: Map<string, { program: string; args: string[]; is_msi: boolean }>
    notes: string[]
  } = $props()
</script>

<!-- ponytail: preview list beats inline gate markup; ceiling is text only, upgrade is publisher icons. -->
<div class="preview-list">
  {#if previews.size > 0}
    <ul>
      {#each [...previews.entries()] as [id, p] (id)}
        <li class="font-mono">{p.program} {p.args.join(' ')}{#if p.is_msi} [MSI]{/if}</li>
      {/each}
    </ul>
  {/if}
  {#if notes.length > 0}
    <ul class="gate-notes">
      {#each notes as n}
        <li>{n}</li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .preview-list ul { margin: 8px 0; padding-left: 18px; font-size: 12px; }
  .gate-notes { color: var(--color-text-secondary); }
</style>
