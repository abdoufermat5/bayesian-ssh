<script lang="ts">
  import { ChevronDown, Check } from "lucide-svelte";
  import { onMount } from "svelte";

  export interface SelectOption {
    value: string;
    label: string;
    description?: string;
  }

  interface Props {
    options: SelectOption[];
    value: string;
    onChange?: (val: string) => void;
    placeholder?: string;
    id?: string;
    class?: string;
    disabled?: boolean;
  }

  let {
    options,
    value = $bindable(),
    onChange,
    placeholder = "Select...",
    id,
    class: className = "",
    disabled = false,
  }: Props = $props();

  let isOpen = $state(false);
  let containerRef = $state<HTMLDivElement | null>(null);

  const selectedOption = $derived(options.find((o) => o.value === value));

  function selectOption(val: string) {
    value = val;
    isOpen = false;
    onChange?.(val);
  }

  function handleClickOutside(event: MouseEvent) {
    if (containerRef && !containerRef.contains(event.target as Node)) {
      isOpen = false;
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (disabled) return;
    if (event.key === "Escape") {
      isOpen = false;
    } else if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      isOpen = !isOpen;
    }
  }

  onMount(() => {
    window.addEventListener("click", handleClickOutside);
    return () => window.removeEventListener("click", handleClickOutside);
  });
</script>

<div
  bind:this={containerRef}
  class="relative inline-block w-full text-xs font-medium text-primary select-none {className}"
>
  <!-- Trigger Button -->
  <button
    type="button"
    {id}
    class="w-full flex items-center justify-between gap-2 px-3 py-2 bg-surface-input border border-border rounded-lg cursor-pointer outline-none transition-all duration-150
      {isOpen ? 'border-accent shadow-[0_0_0_2px_rgba(59,130,246,0.15)] bg-surface-input/90' : 'hover:border-border-hover hover:bg-surface-input/80'}
      {disabled ? 'opacity-50 cursor-not-allowed' : ''}"
    onclick={() => !disabled && (isOpen = !isOpen)}
    onkeydown={handleKeydown}
    aria-expanded={isOpen}
    {disabled}
  >
    <span class="truncate text-left text-primary">
      {selectedOption ? selectedOption.label : placeholder}
    </span>
    <ChevronDown size={14} class="text-muted shrink-0 transition-transform duration-200 {isOpen ? 'rotate-180 text-accent' : ''}" />
  </button>

  <!-- Dropdown Menu Popup -->
  {#if isOpen}
    <div
      class="absolute top-full left-0 right-0 mt-1 z-50 bg-surface-raised border border-border rounded-xl shadow-2xl py-1 overflow-hidden max-h-60 overflow-y-auto backdrop-blur-md animate-in fade-in zoom-in-95 duration-100"
      role="listbox"
    >
      {#each options as option}
        <button
          type="button"
          class="w-full flex items-center justify-between px-3 py-2 text-left cursor-pointer transition-colors text-xs text-secondary hover:bg-accent/10 hover:text-primary
            {value === option.value ? 'bg-accent/15 text-accent font-semibold' : ''}"
          onclick={() => selectOption(option.value)}
          role="option"
          aria-selected={value === option.value}
        >
          <div class="flex flex-col min-w-0 pr-2">
            <span class="truncate">{option.label}</span>
            {#if option.description}
              <span class="text-[10px] text-muted truncate font-normal">{option.description}</span>
            {/if}
          </div>
          {#if value === option.value}
            <Check size={14} class="text-accent shrink-0" />
          {/if}
        </button>
      {/each}
    </div>
  {/if}
</div>
