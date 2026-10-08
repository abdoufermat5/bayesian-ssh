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
    /** Muted inline prefix inside the trigger, e.g. "Sort". */
    label?: string;
    /** Use the compact 28px trigger. */
    size?: "sm" | "md";
  }

  let {
    options,
    value = $bindable(),
    onChange,
    placeholder = "Select...",
    id,
    class: className = "",
    disabled = false,
    label,
    size = "md",
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
  class="relative inline-block select-none {/(^|\s)!?w-/.test(className) ? '' : 'w-full'} {className}"
>
  <button
    bind:this={triggerRef}
    type="button"
    role="combobox"
    {id}
    class="flex w-full cursor-pointer items-center gap-2 rounded-md border bg-surface-input text-left transition-[border-color,box-shadow] duration-fast
      {size === 'sm' ? 'h-7 px-2 text-xs' : 'h-8 px-2.5 text-sm'}
      {isOpen ? 'border-border-focus shadow-[0_0_0_3px_var(--color-accent-muted)]' : 'border-border hover:border-border-hover'}
      {disabled ? 'cursor-not-allowed opacity-50' : ''}"
    onclick={() => {
      if (disabled) return;
      if (isOpen) closeSelect(true);
      else openSelect(selectedIndex);
    }}
    onkeydown={handleKeydown}
    aria-label={label || selectedOption?.label || placeholder}
    aria-expanded={isOpen}
    aria-controls={isOpen ? listboxId : undefined}
    aria-haspopup="listbox"
    aria-activedescendant={activeOptionId}
    {disabled}
  >
    {#if label}
      <span class="shrink-0 text-muted">{label}</span>
    {/if}
    <span class="min-w-0 flex-1 truncate {selectedOption ? 'text-primary' : 'text-muted'}">
      {selectedOption ? selectedOption.label : placeholder}
    </span>
    <ChevronDown size={14} class="shrink-0 text-muted transition-transform duration-fast {isOpen ? 'rotate-180' : ''}" />
  </button>

  {#if isOpen}
    <div
      id={listboxId}
      class="popover absolute left-0 top-full mt-1 max-h-64 min-w-full overflow-y-auto"
      role="listbox"
    >
      {#each options as option, index}
        <button
          type="button"
          id={`${id ?? "custom-select"}-option-${index}`}
          class="menu-item h-auto min-h-8 py-1.5 {activeIndex === index ? 'menu-item-active' : ''} {value === option.value ? 'text-primary' : ''}"
          onmouseenter={() => (activeIndex = index)}
          onclick={() => selectOption(option.value)}
          role="option"
          aria-selected={value === option.value}
        >
          <span class="flex min-w-0 flex-1 flex-col">
            <span class="truncate whitespace-nowrap">{option.label}</span>
            {#if option.description}
              <span class="truncate text-xs text-muted">{option.description}</span>
            {/if}
          </span>
          {#if value === option.value}
            <Check size={14} class="shrink-0 text-accent" />
          {/if}
        </button>
      {/each}
    </div>
  {/if}
</div>
