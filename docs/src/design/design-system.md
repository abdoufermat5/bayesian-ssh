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
│ Hosts · Terminals · Files · Tunnels         │   .view-header  title + count │ actions │
│ Security: Keys · Audit                      │   .view-toolbar search · chips · sort   │
│ Activity: History · Snippets                │   .view-body    scrolling content       │
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

Settings keeps the `.view-header` but replaces toolbar and body with `.settings-shell` (its own section nav plus a `.settings-page`).

Long lists are windowed: the Hosts table mounts only the rows in view (spacer rows keep the scroll height) and the grid mounts cards page by page as you scroll. Reuse that approach for any list that can reach hundreds of rows.

---

## Tokens

All colors are semantic `--color-*` tokens, which generate Tailwind utilities (`bg-panel`, `text-muted`, `border-border`, …). Components never use raw hex values. The one exception is the theme preview swatches in Appearance settings.

| Token | Role |
| --- | --- |
| `chrome` | Title bar and sidebar: the frame around the workspace |
| `surface` | Workspace / view background |
| `panel` | Cards, tables, settings groups |
| `surface-raised` | Popovers, menus, modals, toasts |
| `surface-input` | Inputs, selects, search boxes |
| `surface-hover` / `surface-active` | Row hover / selected row, active nav item |
| `surface-terminal` | xterm background |
| `border` / `border-subtle` / `border-hover` / `border-strong` | Hairlines, from quietest to strongest |
| `border-focus` | Focused control border and focus ring |
| `primary` / `secondary` / `muted` / `faint` | Text, from content down to disabled/decorative |
| `accent` / `accent-hover` / `accent-muted` / `on-accent` | Primary action, selection, focus; text on accent fills |
| `success` / `warning` / `error` (`running`, `danger` aliases) | State only: never decoration |
| `overlay` | Modal backdrop |

The default theme sets these in the `@theme` block. Each `html.theme-*` class overrides them and also defines the xterm palette variables (`--bg-terminal`, `--accent-*`, `--text-*`, `--selection-bg`) that `lib/utils/theme.ts` reads.

| Theme id (persisted) | Name in UI | Character |
| --- | --- | --- |
| `zinc` | Graphite | Neutral graphite, periwinkle accent (default) |
| `cyberpunk` | Midnight | Deep navy, cyan accent |
| `oled` | OLED black | True black, white accent |
| `slate` | Slate | Cool blue-grey, sky accent |

**Type scale** (Inter; JetBrains Mono for data): `2xs` 11px for badges and kbd · `xs` 12px for meta · `sm` 13px for body/default · `base` 14px for view titles · `lg` 16px for modal/settings titles · `xl` 20px for figures. Weights are 400/500/600 only. Arbitrary sizes (`text-[10px]`) are not allowed. Use `tabular-nums` (or `.tabular`) for numbers that line up.

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
| Badges | `.badge` + `-neutral` `-accent` `-success` `-warning` `-error`, `.badge-mono`; `.count`; `.tag`; `.kbd` | Badges are 20px, rounded-sm, never pills |
| Status | `.status-dot` + `-success` `-warning` `-error` `-offline` `-running`, `-sm`, `.status-dot-live` | |
| Text | `.section-label`, `.link`, `.mono`, `.code-block` | |
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
