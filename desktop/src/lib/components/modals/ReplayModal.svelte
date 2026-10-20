<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { FastForward, Pause, Play, RotateCcw, X } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import { buildTerminalOptions } from "$lib/stores/terminal.svelte";
  import {
    attachXtermAddons,
    attachXtermKeyHandler,
    safeWrite,
    waitForTerminalFont,
    type LoadedAddons,
  } from "$lib/utils/terminal-xterm";

  interface Props {
    open: boolean;
    sessionId: string | null;
    connectionName?: string;
    startedLabel?: string;
    onClose: () => void;
  }

  let { open, sessionId, connectionName = "", startedLabel = "", onClose }: Props = $props();

  const IDLE_CAP_SECONDS = 2.0;
  const SPEEDS: Array<1 | 2 | 4> = [1, 2, 4];

  interface CastEvent {
    /** Effective timestamp (seconds) with 2 s idle cap applied. */
    time: number;
    data: string;
  }

  interface ParsedCast {
    width: number;
    height: number;
    title: string;
    events: CastEvent[];
    duration: number;
  }

  let containerRef = $state<HTMLDivElement | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let cast = $state<ParsedCast | null>(null);

  let playing = $state(false);
  let speed = $state<1 | 2 | 4>(1);
  let currentTime = $state(0);

  let term: Terminal | undefined;
  let fitAddon: FitAddon | undefined;
  let addons: LoadedAddons | undefined;
  let eventIndex = 0;
  let rafId: number | null = null;
  let lastFrameMs = 0;

  function parseAsciicast(raw: string): ParsedCast {
    const lines = raw.split(/\r?\n/);
    let headerIdx = 0;
    while (headerIdx < lines.length && !lines[headerIdx].trim()) {
      headerIdx++;
    }
    if (headerIdx >= lines.length) {
      throw new Error("Recording file is empty.");
    }

    const header = JSON.parse(lines[headerIdx]) as {
      version?: number;
      width?: number;
      height?: number;
      title?: string;
    };
    if (header.version !== 2) {
      throw new Error(`Unsupported asciicast version: ${String(header.version)}`);
    }

    const events: CastEvent[] = [];
    let rawPrev = 0;
    let effectiveNow = 0;

    for (let i = headerIdx + 1; i < lines.length; i++) {
      const line = lines[i].trim();
      if (!line) continue;
      const tuple = JSON.parse(line) as [number, string, string];
      if (!Array.isArray(tuple) || tuple.length < 3) continue;
      const [rawTime, kind, data] = tuple;
      if (kind !== "o" || typeof data !== "string" || typeof rawTime !== "number") continue;

      const rawGap = Math.max(0, rawTime - rawPrev);
      rawPrev = Math.max(rawPrev, rawTime);
      effectiveNow += Math.min(rawGap, IDLE_CAP_SECONDS);
      events.push({ time: effectiveNow, data });
    }

    return {
      width: Number(header.width) > 0 ? Number(header.width) : 80,
      height: Number(header.height) > 0 ? Number(header.height) : 24,
      title: header.title || connectionName || "Session replay",
      events,
      duration: effectiveNow,
    };
  }

  function stopLoop() {
    if (rafId !== null) {
      cancelAnimationFrame(rafId);
      rafId = null;
    }
  }

  function destroyTerminal() {
    stopLoop();
    try {
      addons?.webglAddon?.dispose();
    } catch {
      // Ignore WebGL cleanup error
    }
    try {
      term?.dispose();
    } catch {
      // Ignore xterm dispose error
    }
    term = undefined;
    fitAddon = undefined;
    addons = undefined;
  }

  function stepFrame(nowMs: number) {
    if (!playing || !cast || !term) {
      rafId = null;
      return;
    }
    const deltaSec = Math.max(0, (nowMs - lastFrameMs) / 1000) * speed;
    lastFrameMs = nowMs;

    const nextTime = Math.min(cast.duration, currentTime + deltaSec);
    let chunk = "";
    while (eventIndex < cast.events.length && cast.events[eventIndex].time <= nextTime) {
      chunk += cast.events[eventIndex].data;
      eventIndex++;
    }
    if (chunk) {
      safeWrite(term, chunk);
    }
    currentTime = nextTime;

    if (eventIndex >= cast.events.length || currentTime >= cast.duration) {
      currentTime = cast.duration;
      playing = false;
      rafId = null;
      return;
    }

    rafId = requestAnimationFrame(stepFrame);
  }

  function startLoop() {
    stopLoop();
    lastFrameMs = performance.now();
    rafId = requestAnimationFrame(stepFrame);
  }

  function togglePlay() {
    if (!cast || !term) return;
    if (playing) {
      playing = false;
      stopLoop();
      return;
    }
    if (eventIndex >= cast.events.length || (cast.duration > 0 && currentTime >= cast.duration)) {
      term.reset();
      eventIndex = 0;
      currentTime = 0;
    }
    if (cast.duration === 0 && cast.events.length > 0) {
      const all = cast.events.map((e) => e.data).join("");
      safeWrite(term, all);
      eventIndex = cast.events.length;
      playing = false;
      return;
    }
    playing = true;
    startLoop();
  }

  function jumpToEnd() {
    if (!cast || !term) return;
    playing = false;
    stopLoop();
    let remaining = "";
    while (eventIndex < cast.events.length) {
      remaining += cast.events[eventIndex].data;
      eventIndex++;
    }
    if (remaining) {
      safeWrite(term, remaining);
    }
    currentTime = cast.duration;
  }

  function seekTo(targetSec: number) {
    if (!cast || !term) return;
    const clamped = Math.max(0, Math.min(cast.duration, targetSec));
    if (clamped < currentTime) {
      term.reset();
      eventIndex = 0;
    }
    let batch = "";
    while (eventIndex < cast.events.length && cast.events[eventIndex].time <= clamped) {
      batch += cast.events[eventIndex].data;
      eventIndex++;
    }
    if (batch) {
      safeWrite(term, batch);
    }
    currentTime = clamped;
    if (playing) {
      lastFrameMs = performance.now();
    }
  }

  function formatClock(sec: number): string {
    const total = Math.max(0, Math.floor(sec));
    const m = Math.floor(total / 60);
    const s = total % 60;
    return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  }

  $effect(() => {
    if (!open || !sessionId) {
      destroyTerminal();
      cast = null;
      error = null;
      playing = false;
      currentTime = 0;
      return;
    }

    let cancelled = false;
    loading = true;
    error = null;
    cast = null;
    playing = false;
    currentTime = 0;
    eventIndex = 0;

    (async () => {
      try {
        const raw = await invoke<string>("read_recording", { sessionId });
        if (cancelled) return;
        const parsed = parseAsciicast(raw);
        cast = parsed;
      } catch (e) {
        if (!cancelled) {
          error = String(e);
        }
      } finally {
        if (!cancelled) {
          loading = false;
        }
      }
    })();

    return () => {
      cancelled = true;
      destroyTerminal();
    };
  });

  $effect(() => {
    if (!open || !cast || !containerRef) return;

    destroyTerminal();
    const opts = buildTerminalOptions();
    const instance = new Terminal({
      ...opts,
      cols: cast.width,
      rows: cast.height,
      disableStdin: true,
      cursorBlink: false,
    });
    const fit = new FitAddon();
    instance.loadAddon(fit);

    const container = containerRef;
    let disposed = false;

    void waitForTerminalFont(String(opts.fontFamily ?? "monospace"), () => {
      if (!disposed) fit.fit();
    }).then(() => {
      if (disposed) return;
      instance.open(container);
      addons = attachXtermAddons(instance);
      attachXtermKeyHandler(instance);
      fit.fit();
      term = instance;
      fitAddon = fit;
      eventIndex = 0;
      currentTime = 0;

      if (cast && cast.events.length > 0) {
        if (cast.duration === 0) {
          safeWrite(instance, cast.events.map((e) => e.data).join(""));
          eventIndex = cast.events.length;
        } else {
          playing = true;
          startLoop();
        }
      }
    });

    const resizeObserver = new ResizeObserver(() => {
      if (!disposed) fitAddon?.fit();
    });
    resizeObserver.observe(container);

    return () => {
      disposed = true;
      resizeObserver.disconnect();
      destroyTerminal();
    };
  });

  const progressRatio = $derived(
    cast && cast.duration > 0 ? Math.min(1, Math.max(0, currentTime / cast.duration)) : cast && eventIndex > 0 ? 1 : 0,
  );
  const atEnd = $derived(
    Boolean(cast && (eventIndex >= cast.events.length || (cast.duration > 0 && currentTime >= cast.duration))),
  );
</script>

{#if open}
  <ModalShell {open} title="Session replay" {onClose} width="lg" panelClass="max-w-5xl">
    <div class="modal-header">
      <div class="min-w-0">
        <h2 class="modal-title truncate">
          {connectionName || cast?.title || "Session replay"}
        </h2>
        <p class="modal-subtitle flex items-center gap-2 font-mono text-xs text-muted">
          {#if sessionId}
            <span>{sessionId.slice(0, 8)}</span>
          {/if}
          {#if startedLabel}
            <span>·</span>
            <span>{startedLabel}</span>
          {/if}
          <span>·</span>
          <span>Read-only · 2s idle cap</span>
        </p>
      </div>
      <button type="button" class="modal-close" onclick={onClose} aria-label="Close">
        <X size={16} />
      </button>
    </div>

    <div class="modal-body flex flex-col gap-3">
      {#if loading}
        <div class="flex h-96 items-center justify-center rounded-md border border-border bg-surface-terminal">
          <span class="spinner" aria-hidden="true"></span>
          <span class="ml-2 text-sm text-muted">Loading recording…</span>
        </div>
      {:else if error}
        <div class="alert alert-error">
          <div class="alert-title">Could not load recording</div>
          <p class="m-0 text-xs">{error}</p>
        </div>
      {:else}
        <div
          bind:this={containerRef}
          class="h-[420px] w-full overflow-hidden rounded-md border border-border bg-surface-terminal p-2"
        ></div>

        <div class="flex flex-wrap items-center gap-3 rounded-md border border-border bg-panel px-3 py-2">
          <button
            type="button"
            class="btn btn-primary btn-sm"
            onclick={togglePlay}
            aria-label={playing ? "Pause" : atEnd ? "Replay from start" : "Play"}
          >
            {#if playing}
              <Pause size={14} />
              Pause
            {:else if atEnd}
              <RotateCcw size={14} />
              Restart
            {:else}
              <Play size={14} />
              Play
            {/if}
          </button>

          <button
            type="button"
            class="btn btn-secondary btn-sm"
            onclick={jumpToEnd}
            disabled={!cast || atEnd}
            title="Render all remaining output immediately"
          >
            <FastForward size={14} />
            Jump to end
          </button>

          <div class="flex flex-1 items-center gap-2.5">
            <span class="font-mono text-xs tabular-nums text-secondary">
              {formatClock(currentTime)}
            </span>
            <button
              type="button"
              class="relative h-2 flex-1 cursor-pointer overflow-hidden rounded-sm bg-surface-input"
              aria-label="Playback progress"
              onclick={(e) => {
                if (!cast || cast.duration <= 0) return;
                const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
                const ratio = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
                seekTo(ratio * cast.duration);
              }}
            >
              <span
                class="block h-full bg-accent"
                style="width: {(progressRatio * 100).toFixed(1)}%"
              ></span>
            </button>
            <span class="font-mono text-xs tabular-nums text-muted">
              {formatClock(cast?.duration ?? 0)}
            </span>
          </div>

          <div class="segmented" role="group" aria-label="Playback speed">
            {#each SPEEDS as s (s)}
              <button
                type="button"
                class="segmented-item {speed === s ? 'segmented-item-active' : ''}"
                onclick={() => (speed = s)}
                aria-pressed={speed === s}
              >
                {s}×
              </button>
            {/each}
          </div>
        </div>
      {/if}
    </div>
  </ModalShell>
{/if}
