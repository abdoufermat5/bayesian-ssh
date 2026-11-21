<script lang="ts">
  import { ChevronDown, Check, Search } from "lucide-svelte";
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
    /** Explicit accessible name when a separate external <label> is used. */
    ariaLabel?: string;
    /** Use the compact 28px trigger. */
    size?: "sm" | "md";
    /** Show a compact filter input at the top of the listbox. */
    searchable?: boolean;
    /** Placeholder for the search input when searchable is true. */
    searchPlaceholder?: string;
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
    ariaLabel,
    size = "md",
    searchable = false,
    searchPlaceholder = "Filter...",
  }: Props = $props();

  let isOpen = $state(false);
  let containerRef = $state<HTMLDivElement | null>(null);
  let triggerRef = $state<HTMLButtonElement | null>(null);
  let popoverRef = $state<HTMLDivElement | null>(null);
  let optionsListRef = $state<HTMLDivElement | null>(null);
  let searchInputRef = $state<HTMLInputElement | null>(null);
  let activeIndex = $state(0);
  let searchQuery = $state("");
  let popoverStyle = $state("");

  const selectedOption = $derived(options.find((o) => o.value === value));
  const selectedIndex = $derived(Math.max(0, options.findIndex((o) => o.value === value)));

  const filteredOptions = $derived.by(() => {
    if (!searchable) return options;
    const q = searchQuery.trim().toLowerCase();
    if (!q) return options;
    return options.filter(
      (o) =>
        o.label.toLowerCase().includes(q) ||
        (o.description ? o.description.toLowerCase().includes(q) : false)
    );
  });

  const activeOptionId = $derived(
    isOpen && filteredOptions[activeIndex]
      ? `${id ?? "custom-select"}-option-${activeIndex}`
      : undefined
  );

  const listboxId = $derived(`${id ?? "custom-select"}-listbox`);

  function updatePosition() {
    if (!triggerRef || typeof window === "undefined") return;
    const rect = triggerRef.getBoundingClientRect();
    const gap = 4;
    const margin = 8;
    const defaultMaxH = searchable ? 304 : 256;

    const spaceBelow = window.innerHeight - rect.bottom - margin;
    const spaceAbove = rect.top - margin;
    const estimatedHeight = Math.min(
      defaultMaxH,
      popoverRef?.offsetHeight || (filteredOptions.length * 36 + (searchable ? 46 : 10))
    );
    const openUpward = spaceBelow < estimatedHeight && spaceAbove > spaceBelow;

    const maxH = Math.max(
      112,
      Math.min(defaultMaxH, (openUpward ? spaceAbove : spaceBelow) - gap)
    );

    const minWidth = Math.max(rect.width, 0);
    const maxWidth = Math.max(minWidth, window.innerWidth - margin * 2);
    const left = Math.max(margin, Math.min(rect.left, window.innerWidth - minWidth - margin));

    const parts = [
      `position: fixed`,
      `z-index: 120`,
      `left: ${Math.round(left)}px`,
      `min-width: ${Math.round(minWidth)}px`,
      `max-width: ${Math.round(maxWidth)}px`,
      `max-height: ${Math.round(maxH)}px`,
    ];

    if (openUpward) {
      parts.push(`bottom: ${Math.round(window.innerHeight - rect.top + gap)}px`);
    } else {
      parts.push(`top: ${Math.round(rect.bottom + gap)}px`);
    }

    popoverStyle = parts.join("; ");
  }

  function openSelect(index = selectedIndex) {
    if (disabled || options.length === 0) return;
    searchQuery = "";
    activeIndex = Math.max(0, Math.min(options.length - 1, index));
    updatePosition();
    isOpen = true;
    requestAnimationFrame(() => {
      updatePosition();
      if (searchable) {
        searchInputRef?.focus();
      }
      scrollActiveIntoView();
    });
  }

  function closeSelect(restoreFocus = false) {
    isOpen = false;
    searchQuery = "";
    if (restoreFocus) {
      requestAnimationFrame(() => triggerRef?.focus());
    }
  }

  function selectOption(val: string) {
    value = val;
    closeSelect(true);
    onChange?.(val);
  }

  function scrollActiveIntoView() {
    if (!optionsListRef || !activeOptionId) return;
    const el = optionsListRef.querySelector<HTMLElement>(`#${CSS.escape(activeOptionId)}`);
    el?.scrollIntoView({ block: "nearest" });
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
      } else if (filteredOptions.length > 0) {
        activeIndex = (activeIndex + 1) % filteredOptions.length;
        requestAnimationFrame(scrollActiveIntoView);
      }
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      if (!isOpen) {
        openSelect(selectedIndex);
      } else if (filteredOptions.length > 0) {
        activeIndex = (activeIndex - 1 + filteredOptions.length) % filteredOptions.length;
        requestAnimationFrame(scrollActiveIntoView);
      }
    } else if (event.key === "Home" && isOpen) {
      event.preventDefault();
      activeIndex = 0;
      requestAnimationFrame(scrollActiveIntoView);
    } else if (event.key === "End" && isOpen && filteredOptions.length > 0) {
      event.preventDefault();
      activeIndex = filteredOptions.length - 1;
      requestAnimationFrame(scrollActiveIntoView);
    } else if (event.key === "Escape" && isOpen) {
      event.preventDefault();
      event.stopPropagation();
      closeSelect(true);
    } else if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      if (isOpen && filteredOptions[activeIndex]) {
        selectOption(filteredOptions[activeIndex].value);
      } else if (!isOpen) {
        openSelect(selectedIndex);
      }
    }
  }

  function handleSearchKeydown(event: KeyboardEvent) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      if (filteredOptions.length > 0) {
        activeIndex = (activeIndex + 1) % filteredOptions.length;
        requestAnimationFrame(scrollActiveIntoView);
      }
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      if (filteredOptions.length > 0) {
        activeIndex = (activeIndex - 1 + filteredOptions.length) % filteredOptions.length;
        requestAnimationFrame(scrollActiveIntoView);
      }
    } else if (event.key === "Enter") {
      event.preventDefault();
      if (filteredOptions[activeIndex]) {
        selectOption(filteredOptions[activeIndex].value);
      }
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      closeSelect(true);
    }
  }

  $effect(() => {
    if (!isOpen) return;
    if (options.length === 0) {
      closeSelect(true);
    } else if (filteredOptions.length === 0) {
      activeIndex = 0;
    } else if (activeIndex >= filteredOptions.length) {
      activeIndex = filteredOptions.length - 1;
    }
  });

  $effect(() => {
    if (!isOpen || !searchable) return;
    // Reset highlight to first match when search query changes.
    void searchQuery;
    activeIndex = 0;
    requestAnimationFrame(updatePosition);
  });

  $effect(() => {
    if (!isOpen) return;
    const onScroll = (event: Event) => {
      if (popoverRef && event.target instanceof Node && popoverRef.contains(event.target)) {
        return;
      }
      updatePosition();
    };
    const onResize = () => updatePosition();
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", onResize);
    };
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
    class="flex w-full items-center gap-2 rounded-md border text-left transition-[border-color,box-shadow] duration-fast
      {size === 'sm' ? 'h-7 px-2 text-xs' : 'h-8 px-2.5 text-sm'}
      {disabled
        ? 'cursor-not-allowed border-dashed border-border bg-surface-subtle text-disabled-fg'
        : isOpen
          ? 'cursor-pointer border-border-focus bg-surface-input shadow-[0_0_0_3px_var(--color-accent-muted)]'
          : 'cursor-pointer border-border-control bg-surface-input hover:border-border-strong'}"
    onclick={() => {
      if (disabled) return;
      if (isOpen) closeSelect(true);
      else openSelect(selectedIndex);
    }}
    onkeydown={handleKeydown}
    aria-label={ariaLabel || label || selectedOption?.label || placeholder}
    aria-expanded={isOpen}
    aria-controls={isOpen ? listboxId : undefined}
    aria-haspopup="listbox"
    aria-activedescendant={activeOptionId}
    {disabled}
  >
    {#if label}
      <span class="shrink-0 {disabled ? 'text-disabled-fg' : 'text-muted'}">{label}</span>
    {/if}
    <span
      class="min-w-0 flex-1 truncate {disabled
        ? 'text-disabled-fg'
        : selectedOption
          ? 'text-primary'
          : 'text-muted'}"
    >
      {selectedOption ? selectedOption.label : placeholder}
    </span>
    <ChevronDown
      size={14}
      class="shrink-0 transition-transform duration-fast {disabled ? 'text-disabled-fg' : 'text-muted'} {isOpen ? 'rotate-180' : ''}"
    />
  </button>

  {#if isOpen}
    <div
      bind:this={popoverRef}
      id={listboxId}
      style={popoverStyle}
      class="popover flex flex-col overflow-hidden"
      role="listbox"
    >
      {#if searchable}
        <div class="mb-1 flex shrink-0 items-center gap-1.5 border-b border-border px-2 pb-1.5 pt-1">
          <Search size={13} class="shrink-0 text-muted" aria-hidden="true" />
          <input
            bind:this={searchInputRef}
            bind:value={searchQuery}
            type="text"
            placeholder={searchPlaceholder}
            aria-label={searchPlaceholder}
            aria-activedescendant={activeOptionId}
            onkeydown={handleSearchKeydown}
            class="h-6 w-full min-w-0 border-none bg-transparent text-xs text-primary outline-none placeholder:text-muted"
          />
        </div>
      {/if}

      <div bind:this={optionsListRef} class="min-h-0 flex-1 overflow-y-auto">
        {#if filteredOptions.length === 0}
          <div class="px-2 py-2 text-xs text-muted">No matches</div>
        {:else}
          {#each filteredOptions as option, index (option.value)}
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
        {/if}
      </div>
    </div>
  {/if}
</div>
