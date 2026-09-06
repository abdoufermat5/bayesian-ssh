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
  let triggerRef = $state<HTMLButtonElement | null>(null);
  let activeIndex = $state(0);

  const selectedOption = $derived(options.find((o) => o.value === value));
  const selectedIndex = $derived(Math.max(0, options.findIndex((o) => o.value === value)));
  const activeOptionId = $derived(isOpen && options[activeIndex] ? `${id ?? "custom-select"}-option-${activeIndex}` : undefined);

  const listboxId = $derived(`${id ?? "custom-select"}-listbox`);

  function openSelect(index = selectedIndex) {
    if (disabled || options.length === 0) return;
    activeIndex = Math.max(0, Math.min(options.length - 1, index));
    isOpen = true;
  }

  function closeSelect(restoreFocus = false) {
    isOpen = false;
    if (restoreFocus) {
      requestAnimationFrame(() => triggerRef?.focus());
    }
  }

  function selectOption(val: string) {
    value = val;
    closeSelect(true);
    onChange?.(val);
  }

  function handleClickOutside(event: MouseEvent) {
    if (containerRef && !containerRef.contains(event.target as Node)) {
      closeSelect(false);
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (disabled) return;

    if (options.length === 0) {
      if (event.key === "Escape" && isOpen) {
        event.preventDefault();
        event.stopPropagation();
        closeSelect(true);
      }
      return;
    }

    if (event.key === "ArrowDown") {
      event.preventDefault();
      if (!isOpen) {
        openSelect(selectedIndex);
      } else {
        activeIndex = (activeIndex + 1) % options.length;
      }
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      if (!isOpen) {
        openSelect(selectedIndex);
      } else {
        activeIndex = (activeIndex - 1 + options.length) % options.length;
      }
    } else if (event.key === "Home" && isOpen) {
      event.preventDefault();
      activeIndex = 0;
    } else if (event.key === "End" && isOpen) {
      event.preventDefault();
      activeIndex = options.length - 1;
    } else if (event.key === "Escape" && isOpen) {
      event.preventDefault();
      event.stopPropagation();
      closeSelect(true);
    } else if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      if (isOpen && options[activeIndex]) {
        selectOption(options[activeIndex].value);
      } else {
        openSelect(selectedIndex);
      }
    }
  }

  $effect(() => {
    if (!isOpen) return;
    if (options.length === 0) {
      closeSelect(true);
    } else if (activeIndex >= options.length) {
      activeIndex = options.length - 1;
    }
  });

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
    bind:this={triggerRef}
    type="button"
    role="combobox"
    {id}
    class="flex w-full cursor-pointer items-center justify-between gap-2 rounded-md border border-border bg-surface-input px-3 py-2 text-sm transition-colors duration-fast
      {isOpen ? 'border-accent shadow-[0_0_0_2px_var(--color-accent-muted)]' : 'hover:border-border-hover hover:bg-surface-hover'}
      {disabled ? 'opacity-50 cursor-not-allowed' : ''}"
    onclick={() => {
      if (disabled) return;
      if (isOpen) closeSelect(true);
      else openSelect(selectedIndex);
    }}
    onkeydown={handleKeydown}
    aria-expanded={isOpen}
    aria-controls={isOpen ? listboxId : undefined}
    aria-haspopup="listbox"
    aria-activedescendant={activeOptionId}
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
      id={listboxId}
      class="absolute top-full left-0 right-0 z-50 mt-1 max-h-60 overflow-y-auto rounded-md border border-border bg-surface-raised py-1 shadow-lg animate-[popover-enter_0.12s_var(--ease-out)_forwards]"
      role="listbox"
    >
      {#each options as option, index}
        <button
          type="button"
          id={`${id ?? "custom-select"}-option-${index}`}
          class="flex w-full cursor-pointer items-center justify-between px-3 py-2 text-left text-sm text-secondary transition-colors
            {activeIndex === index ? 'bg-surface-hover text-primary' : ''}
            {value === option.value ? 'text-accent font-semibold' : ''}"
          onmouseenter={() => (activeIndex = index)}
          onclick={() => selectOption(option.value)}
          role="option"
          aria-selected={value === option.value}
        >
          <div class="flex flex-col min-w-0 pr-2">
            <span class="truncate">{option.label}</span>
            {#if option.description}
              <span class="truncate text-xs font-normal text-muted">{option.description}</span>
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
