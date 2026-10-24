# Design System

How the desktop GUI looks and how new screens must be built. The implementation lives in
[`desktop/src/lib/styles/`](../../../../desktop/src/lib/styles/):

| File | Contents |
| --- | --- |
| `tokens.css` | Color, type, radius, shadow, motion and layout tokens, plus per-theme overrides |
| `base.css` | Reset, typography defaults, focus ring, scrollbars, keyframes, reduced-motion guard |
| `components.css` | Every shared primitive (buttons, inputs, tables, modals, settings rows, …) |
| `app.css` | Entry point: Tailwind, bundled fonts, the files above |

Fonts are bundled (`@fontsource-variable/inter`, `@fontsource-variable/jetbrains-mono`):
the app's CSP blocks remote fonts, and the UI must look the same offline.

---

## Principles

- **Calm, dense, technical.** Neutral surfaces, one accent, real data in front. No gradients, glow, glassmorphism or decorative icon tiles.
- **One title per screen.** The view header is the page header; there is no global top bar repeating it.
- **One primary action per view or modal.** Everything else is secondary or ghost.
- **Keyboard first.** Every action is reachable by keyboard and shortcuts are visible (`.kbd`).
- **Motion only signals change** (100–220 ms, `--ease-out`). `prefers-reduced-motion` disables it.
- **Cheap to render.** No `backdrop-filter` (expensive in WebKitGTK, especially over the terminal's WebGL canvas), no `transition-all`, no colored shadows.

---

## Layout

```
┌ titlebar (bg-chrome, 38px): brand · command search (Ctrl/⌘K) · help · window buttons ┐
├ sidebar (bg-chrome, 224px / 56px collapsed) ┬ workspace (bg-surface, rounded top-left) ┤
│ profile switcher                            │ .view                                   │
│ Hosts · Terminals · Files                   │   .view-header  title + count │ actions │
│ Security: Keys · Security check             │   .view-toolbar search · chips · sort   │
│ Activity: History · Team: Teams             │   .view-body    scrolling content       │
│ … agent / Kerberos status · Settings        │                                         │
└─────────────────────────────────────────────┴─────────────────────────────────────────┘
```

Every top-level view uses exactly this skeleton:

```svelte
<div class="view">
  <header class="view-header">
    <div class="flex min-w-0 items-baseline gap-2.5">
      <h1 class="view-title">Hosts</h1>
      <span class="text-sm tabular-nums text-muted">12</span>
    </div>
    <div class="view-actions">
      <button type="button" class="btn btn-ghost">Secondary</button>
      <button type="button" class="btn btn-primary">Primary</button>
    </div>
  </header>
  <div class="view-toolbar"><!-- optional filters --></div>
  <div class="view-body"><!-- content --></div>
</div>
```

Settings sits in a centred `.settings-frame` (960–1040 px), keeps one `.view-header` (title "Settings" plus the current section) and replaces toolbar and body with `.settings-shell`: a rail of the six sections from `stores/settingsNav.svelte.ts` (icons only below 1180 px) and a `.settings-stack` of panels. Other code opens a section with `requestSettingsSection(id)`.

Empty states use one pattern: `.empty-state` > `.empty-state-icon` + `.empty-state-title` (one sentence) + `.empty-state-action` (one primary action). Don't repeat the view header in it.

Long lists are windowed: the Hosts table mounts only the rows in view (spacer rows keep the scroll height). Reuse that approach for any list that can reach hundreds of rows.

---

## Tokens

All colors are semantic `--color-*` tokens, which generate Tailwind utilities (`bg-panel`, `text-muted`, `border-border`, …). Components never use raw hex values. The one exception is the theme preview swatches in Appearance settings.

| Token | Role |
| --- | --- |
| `chrome` | Title bar and sidebar: the frame around the workspace |
| `surface` | Workspace / view background |
| `panel` | Cards, tables, settings groups |
| `surface-raised` | Floating layers: command palette, popovers, menus, modals, toasts. A visible step above `panel` in every theme |
| `surface-input` | Inputs, selects, search boxes |
| `surface-hover` / `surface-active` | Row hover / pressed state, active nav item |
| `selected` | Tint for the selected row or palette item; always paired with a 2 px `accent` left rail |
| `hover-on-floating` | Hover fill for items inside floating layers (they are lighter than `surface-hover`) |
| `surface-terminal` | xterm background |
| `border` / `border-subtle` / `border-hover` / `border-strong` | Decorative hairlines (panels, dividers, table rules), from quietest to strongest. Not for controls |
| `border-control` | Boundary of inputs, selects, secondary buttons, chips, switch off-track, checkboxes. ≥ 3:1 on the surface behind it |
| `border-floating` | Edge of floating layers (`.popover`, `.modal-panel`, `.toast`, command palette) |
| `border-focus` | Focused control border and focus ring |
| `primary` / `secondary` / `muted` / `faint` | Text, from content down to disabled/decorative |
| `accent` / `accent-hover` / `accent-muted` / `on-accent` | Primary action, selection, focus; text on accent fills |
| `accent-strong` / `accent-strong-hover` | Primary button fill / hover fill. Hover is a real token, not `brightness()`, so it stays ≥ 4.5:1 with `on-accent` |
| `danger-strong` / `danger-strong-hover` / `on-danger` | Danger button fill / hover / text (≥ 4.5:1). `error`/`danger` are the lighter red for text and icons |
| `disabled-fg` | Text of disabled controls: ≥ 3:1 yet dimmer than `muted` |
| `success` / `warning` / `error` (`running`, `danger` aliases) | State only: never decoration |
| `overlay` | Modal backdrop |

**Color policy.** One accent, blue, in every theme. It marks the primary action, focus, selection, links and "in progress", nothing else. Labels and badges (team, role, source, counts) are neutral; their icon or text says what they are. `success` / `warning` / `error` mark state only. The only multicolor surface is the terminal, whose ANSI palette is program output; it is muted to sit with the UI.

The `@theme` block is the Dark theme. `html.theme-light` overrides it. Each theme also defines the xterm palette (`--term-*`, `--selection-bg`) that `lib/utils/theme.ts` reads.

| Theme id (persisted) | Name in UI | Character |
| --- | --- | --- |
| `dark` | Dark | Neutral graphite, accent `#5b8def`, primary buttons `#2563eb` (default) |
| `light` | Light | Neutral light grays, accent and primary buttons `#2563eb` |
| `system` | Match system | Resolves to `light` or `dark` from `prefers-color-scheme` and follows changes live; `<html data-theme="system" data-resolved-theme="…">` |

`THEME_OPTIONS` in `lib/utils/theme.ts` is the one list of pickable themes (onboarding, Appearance and the palette read it). Themes from earlier versions load as their replacement (`zinc`, `cyberpunk`, `oled`, `slate` → `dark`; `paper` → `light`), in both the Rust settings loader and `normalizeTheme`.

**Density:** `data-density` on `<html>` (`compact` / `comfortable` / `spacious`) sets `--row-height` to 32 / 40 / 48 px. List and table rows use `height: var(--row-height, 40px)`.

### Contrast rules (WCAG 2.2 AA)

Checked per theme by `desktop/scripts/check-contrast.mjs`, which parses `tokens.css` (the `@theme` block plus each `html.theme-*` override):

| Pair | Minimum |
| --- | --- |
| `primary`, `secondary`, `muted` on every surface (`surface`, `panel`, `surface-raised`, `surface-input`, `surface-hover`, `surface-active`, `chrome`), on selected rows/palette items and on menu hover | 4.5:1 |
| `accent`, `success`, `warning`, `error` text on `panel` | 4.5:1 |
| `error` on a floating layer and on its danger-menu hover | 4.5:1 |
| `on-accent` on `accent-strong` and `accent-strong-hover` (primary button, normal and hover) | 4.5:1 |
| `on-danger` on `danger-strong` and `danger-strong-hover` (danger button, normal and hover) | 4.5:1 |
| `border-control` on `surface`, `panel`, `surface-raised`, `surface-input`, `surface-active` | 3:1 |
| `border-focus` on `surface`, `surface-raised` | 3:1 |
| `disabled-fg` on `panel`, `surface-input`, `surface-active`, `surface-subtle` | 3:1 |
| `surface-raised` vs `surface` / `border-floating` vs `surface-raised` | 1.1:1 / 1.5:1 (elevation must be visible) |

Run it with `cd desktop && npm run check:contrast`; it prints one row per check and exits 1 on any failure. CI runs it in the *Test Desktop* job. When you change a color token, run it before committing.

**State rules**

- Use `border-control` (never `border`) for anything the user must locate and operate. Keep `border` for panel edges.
- Floating layers = `surface-raised` + `border-floating` + `shadow-lg`/`shadow-xl`, whose inset top highlight keeps the edge visible on near-black themes where drop shadows vanish.
- Selected rows and options: `selected` tint plus a 2 px accent left rail (`tr.is-selected`, `.menu-item-active`, `.nav-item-active`, `.settings-nav-item-active`, `.card-selected`, `[role=option][aria-selected=true]`). The rail is a shape, so the state does not depend on color alone.
- Disabled controls keep `disabled-fg` text on a flat fill, drop shadows, show a dashed edge on fields and use `cursor-not-allowed`. They are not dimmed with opacity. Disabled buttons keep pointer events so a `title` can explain why.
- `.tag` is a static label (flat filled rectangle, no border). `.chip` is an interactive filter (pill, `border-control`, 28 px high; `.chip-active` adds accent border, tint and a ✓). Don't use one for the other.
- Class-based hit areas are ≥ 24 px: `.btn-icon-sm` is 24 px, `.switch` and `.checkbox` extend their hit box with a transparent `::before`.

**Type scale** (Inter; JetBrains Mono for data): five sizes as `--type-*` tokens and classes, each with one job: `.type-title` 20px/600 for view titles · `.type-section` and `.section-heading` 14px/600 for section headings · `.type-body` 13px for body/default · `.type-meta` 12px muted for meta · `.type-caption` 11px/500 for badges, kbd and eyebrows. Weights are 400/500/600 only. Arbitrary sizes (`text-[10px]`) are not allowed. Use `tabular-nums` (or `.tabular`) for numbers that line up.

**Radius:** `md` 6px for controls · `lg` 8px for cards and tables · `xl` 10px for modals.

**Layout:** `--titlebar-h` 38px · `--header-h` 52px · `--sidebar-w` 224px / `--sidebar-w-collapsed` 56px.

---

## Primitives

| Group | Classes | Notes |
| --- | --- | --- |
| Buttons | `.btn` + `-primary` `-secondary` `-ghost` `-danger` `-danger-ghost`; `.btn-sm` (28px) `.btn-lg` (36px); `.btn-icon` `.btn-icon-sm` `.btn-icon-danger` | Default height 32px. Icon buttons need `aria-label`. |
| Inputs | `.input` `.input-mono` `.input-invalid`, `textarea.input`; `.search-box > input` | Focus shows the `border-focus` border plus an accent-muted ring |
| Fields | `.field` `.field-label` `.field-hint` `.field-error` | Validation messages sit inline under the control |
| Toggles | `input.switch` (on/off settings), `input.checkbox` (selection) | Native checkboxes, restyled |
| Select | `CustomSelect` component (`label`, `size="sm"\|"md"`) | Keyboard navigable listbox |
| Badges | `.badge` + `-neutral` `-accent` `-success` `-warning` `-error`, `.badge-mono`; `.count`; `.tag` (static label); `.kbd` | Badges are 20px, rounded-sm, never pills |
| Status | `.status-dot` + `-success` `-warning` `-error` `-offline` `-running`, `-sm`, `.status-dot-live` | |
| Text | `.section-label`, `.link`, `.mono`, `.code-block` | |
| Diff | `.diff` > `.diff-line` + `-add` `-del` `-hunk` (`DiffView` component) | The +/- prefix carries the meaning, the tint only supports it. Make the `.diff` region focusable so long lines scroll by keyboard |
| Containers | `.panel` (+ `-header` `-title` `-body`), `.card` `.card-interactive` `.card-selected`, `.stat` (+ `-label` `-value`), `.divider` | |
| Tables | `.table-wrap > table.data-table`; `tr.is-selected`; `.row-actions` | Actions are revealed on row hover, selection and focus |
| Navigation | `.segmented` `.segmented-item(-active)`, `.chip(-active)`, `.tabs` `.tab(-active)` | |
| Menus | `.popover`, `.menu-item(-active)(-danger)`, `.menu-separator` | |
| Feedback | `.alert` + `-error` `-warning` `-success` `-info`, `.alert-title`; `.toast`; `.skeleton`; `.spinner` | |
| Empty states | `.empty-state` `-icon` `-title` `-desc` `-action` | One sentence of description, at most two actions |
| Modal | `ModalShell` component → `.modal-overlay` `.modal-panel`; `.modal-header` `.modal-title` `.modal-subtitle` `.modal-close` `.modal-body` `.modal-footer` | Footer: secondary, then primary, right-aligned |
| Settings | `.settings-shell` `.settings-sidebar` `.settings-nav-item(-active)` `.settings-content` `.settings-page` `.settings-heading` `.settings-desc` `.settings-group` `.settings-group-title` `.setting-row` `.setting-row-main` `.setting-title` `.setting-meta` `.setting-control` `.system-value` | Rows inside a group are separated automatically |

If a pattern repeats in markup, it belongs in `components.css`; don't add a local variant.

---

## Writing

- Sentence case everywhere ("New host", "Run security audit").
- Labels say what happens; avoid marketing terms ("high-performance", "studio").
- Errors say what failed and what to do next. Prefer an inline `.field-error` to a toast when the problem is in a form field.
- Relative times ("5 min ago") in lists, with the absolute date in the `title` tooltip (`formatRelative` / `formatDateTime` in `lib/utils/timezone.ts`).

## Accessibility

- Every interactive element is a real `<button type="button">`, link or input, and gets the global `:focus-visible` ring.
- Icon-only buttons have `aria-label`. Toggles and segmented controls expose `aria-pressed` / `aria-checked`.
- Modals trap focus, close on Escape and restore focus on close (`ModalShell`).
- Never encode meaning in color alone: status dots carry a `title`, and badges carry text.
