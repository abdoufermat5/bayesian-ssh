<script lang="ts">
  import { HardDrive, RefreshCw, Shield, ShieldCheck, TerminalSquare } from "lucide-svelte";

  interface Props {
    title?: string;
    subtitle?: string;
  }

  let {
    title = "Initializing Bayesian SSH",
    subtitle = "Loading environment, security policies, and connection registry...",
  }: Props = $props();
</script>

<div class="fixed inset-0 bg-[#09090b] z-[300] flex flex-col items-center justify-center p-6 text-center select-none overflow-hidden">
  <!-- Glowing Background Pulse Effects -->
  <div class="absolute w-96 h-96 rounded-full bg-accent/10 blur-3xl pointer-events-none animate-pulse"></div>
  <div class="absolute w-64 h-64 rounded-full bg-cyan-500/10 blur-2xl pointer-events-none animate-pulse" style="animation-delay: 1s;"></div>

  <div class="relative z-10 flex flex-col items-center gap-6 max-w-sm">
    <!-- Animated Logo Icon Badge with Spinning Border -->
    <div class="relative flex items-center justify-center">
      <div class="w-20 h-20 rounded-2xl bg-surface-card border border-border shadow-2xl flex items-center justify-center relative overflow-hidden backdrop-blur-md">
        <ShieldCheck size={36} class="text-accent animate-bounce" style="animation-duration: 2s;" />
      </div>

      <!-- Outer Gradient Spinner Ring -->
      <div class="absolute -inset-2 rounded-3xl border-2 border-transparent border-t-accent border-r-cyan-400 animate-spin" style="animation-duration: 1.5s;"></div>
    </div>

    <!-- Title & Subtitle -->
    <div class="flex flex-col gap-1.5">
      <h2 class="text-lg font-extrabold text-primary m-0 tracking-tight flex items-center justify-center gap-2">
        <span>{title}</span>
        <span class="inline-flex h-2 w-2 rounded-full bg-accent animate-ping"></span>
      </h2>
      <p class="text-xs text-muted m-0 font-medium leading-relaxed">
        {subtitle}
      </p>
    </div>

    <!-- Pulsing Loading Bar -->
    <div class="w-48 h-1.5 bg-surface-input rounded-full overflow-hidden border border-border/50 relative">
      <div class="h-full bg-gradient-to-r from-accent via-cyan-400 to-accent rounded-full animate-pulse w-full"></div>
    </div>
  </div>
</div>
