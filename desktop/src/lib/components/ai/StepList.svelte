<script lang="ts">
  import type { AiAnalysis, AiStep } from "$lib/types";
  import RiskTags from "./RiskTags.svelte";
  import DiffView from "./DiffView.svelte";

  interface Props {
    label: string;
    steps: AiStep[];
    /** Same order as `steps`; null where the analyzer had nothing to say. */
    analyses: (AiAnalysis | null)[];
  }

  let { label, steps, analyses }: Props = $props();
</script>

{#if steps.length > 0}
  <section aria-label={label} class="flex flex-col gap-2">
    <h3 class="section-label m-0">{label}</h3>
    <ol class="m-0 flex list-none flex-col gap-2 p-0">
      {#each steps as step, i (step.id)}
        {@const analysis = analyses[i] ?? null}
        {@const action = step.action}
        <li class="flex flex-col gap-2 rounded-lg border border-border bg-panel p-3">
          <div class="flex flex-wrap items-center gap-1.5">
            <span class="tag font-mono">{step.id}</span>
            <span class="tag">{step.host ?? "no host"}</span>
            {#if analysis}<RiskTags tags={analysis.tags} />{/if}
          </div>

          {#if "run" in action}
            <pre class="code-block m-0 whitespace-pre-wrap break-words">{action.run.command}</pre>
          {:else if "edit_file" in action}
            <p class="m-0 text-sm text-primary">
              Edit <span class="mono break-all">{action.edit_file.path}</span>
            </p>
            <DiffView patch={action.edit_file.patch} label={action.edit_file.path} />
          {:else if "service" in action}
            <p class="m-0 text-sm text-primary">
              {action.service.action[0].toUpperCase() + action.service.action.slice(1)} service
              <span class="mono break-all">{action.service.unit}</span>
            </p>
          {:else if "undo" in action}
            <p class="m-0 text-sm text-primary">
              Undo <span class="mono">{action.undo.step}</span>
            </p>
          {/if}

          {#if analysis?.unknown_reason}
            <p class="alert alert-warning m-0">
              Could not be fully analysed: {analysis.unknown_reason}
            </p>
          {/if}
        </li>
      {/each}
    </ol>
  </section>
{/if}
