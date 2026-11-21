# Design System

How the desktop GUI looks and how new screens must be built. The implementation lives in
[`desktop/src/lib/styles/`](../../../../desktop/src/lib/styles/):

| File | Contents |
| --- | --- |
| `tokens.css` | Color, type, radius, shadow, motion and layout tokens, plus per-theme overrides |
| `base.css` | Reset, typography defaults, focus ring, scrollbars, boot keyframe, reduced-motion guard |
| `components.css` | Every shared primitive (buttons, inputs, tables, lamps, modals, settings rows, …) |
| `app.css` | Entry point: Tailwind, bundled fonts, the files above |

Fonts are bundled (`@fontsource-variable/archivo`, `@fontsource-variable/ibm-plex-sans`,
`@fontsource-variable/jetbrains-mono`): the app's CSP blocks remote fonts, and the UI must look
the same offline.

---

## Principles

- **Clean precision workbench.** Cool obsidian/graphite (dark) or crisp cool neutral (light)
  surfaces, hairline rules, modern radii (`4px`–`12px`), dense real data. No gradients, glow,
  glassmorphism, grain textures, decorative icon tiles or colored shadows.
- **One signal accent: indigo.** It marks the primary action, focus, selection, links and a live
  session, nothing else. Labels and badges stay neutral; their icon or text says what they are.
- **Hosts are a patch bay.** Each host row carries a round jack lamp that lights accent indigo
  while a session to that host is live; a failed session fills it with `error`. Never encode the
  state in color alone — the lamp is paired with a `title` or sr-only text.
- **One title per screen.** The view header is the page header; there is no global top bar
  repeating it.
- **One primary action per view or modal.** Everything else is secondary or ghost.
- **Keyboard first.** Every action is reachable by keyboard and shortcuts are visible (`.kbd`,
  sidebar digits 1–8).
- **Motion only signals change** (100–220 ms, `--ease-out`). The workspace settles once on app
  load (`.workspace > *`, `console-boot`, 220 ms); lamps animate only to signal a state — the
  connecting jack breathes, live and error are static. `prefers-reduced-motion` disables all of it.
- **Cheap to render.** No `backdrop-filter` (expensive in WebKitGTK, especially over the
  terminal's WebGL canvas), no `transition-all`, no colored shadows.

---

## Layout

```
┌ titlebar (bg-chrome, 38px): brand · command bar (Ctrl/⌘K) · help · window buttons   ┐
├ sidebar (bg-chrome, 224px / 56px collapsed) ┬ workspace (bg-surface, rounded top-left) ┤
│ profile switcher                            │ Console                                 │
│ OPERATE  Console 1 · Files 2 · AI agents 3  │   host list (home), or                  │
│ SECURE   Keys 4 · Security check 5 · Teams 6│   pinned Hosts tab + session tabs,      │
│ REVIEW   History 7 · AI reports 8           │   terminal panes stay mounted           │
│ … agent / Kerberos status · Settings        │ .view-header · toolbar · body           │
└─────────────────────────────────────────────┴─────────────────────────────────────────┘
```

The title bar holds the brand wordmark (display font, uppercase "BAYESIAN SSH") and, centred, a
command bar (up to 440 px): mono placeholder `› Jump to host, view or action`, keycaps on the
right, `aria-label="Jump to…"`.

Every top-level view uses exactly this skeleton:

```svelte
<div class="view">
  <header class="view-header">
    <div class="flex min-w-0 items-baseline gap-2.5">
      <h1 class="view-title">Hosts</h1>
      <span class="font-mono text-sm tabular-nums text-muted">08</span>
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

`.view-header` carries a 1 px bottom hairline (`border`), the title is sentence-case IBM Plex Sans
at 18 px (`-0.02em`), and the count is a zero-padded mono number in `muted`. `.view-toolbar` adds
top breathing room (`pt-3`), `.view-body` defaults to `pt-4`, and `.view-toolbar + .view-body` or
`.view-subbar + .view-body` collapses the body's top padding to `0`.

### Console

One Console workspace replaces the old separate **Hosts** and **Terminals** entries. The internal
`AppTab` values `"connections"` and `"terminals"` are unchanged.

- **No open tabs and no away sessions:** the Console is the host list (`ConnectionsView`) alone.
- **Otherwise** `TerminalsView` is the Console. Its strip starts with a pinned **Hosts** tab (home,
  house icon, accent top rail when active); the host list is layered above the terminal panes,
  which stay mounted (hidden with `visibility`, not unmounted, so xterm buffers survive). Clicking
  a session tab switches to `"terminals"`.
- The pinned tab is active when `activeTab === "connections"` or there are zero tabs. The separate
  terminal empty state (`TerminalsEmptyState.svelte`) is gone; away sessions already show as chips
  in the strip.

Settings sits in a centred `.settings-frame` (960–1040 px), keeps one `.view-header` (title
"Settings" plus the current section) and replaces toolbar and body with `.settings-shell`: a rail
of the sections from `stores/settingsNav.svelte.ts` (icons only below 1180 px) and a
`.settings-stack` of panels. Other code opens a section with `requestSettingsSection(id)`.

Empty states use one pattern: `.empty-state` > `.empty-state-icon` + `.empty-state-title`
(one sentence) + `.empty-state-action` (one primary action). Don't repeat the view header in it.

Long lists are windowed: the host table mounts only the rows in view (spacer rows keep the scroll
height). Reuse that approach for any list that can reach hundreds of rows.

### Sidebar

Three sections, each label an `.eyebrow`. The digit shortcuts run 1–8 down the list (`VIEW_ORDER`,
`AI_REPORTS_DIGIT = "8"`) but are not printed on the items: the button's `title` and
`aria-keyshortcuts` carry them, and the shortcuts sheet (`?`) lists them. Only a badge or attention
count sits at the right edge.

- **Operate:** Console (1, badge = live session count), Files (2, if enabled), AI agents (3)
- **Secure:** Keys (4), Security check (5), Teams (6)
- **Review:** History (7), AI reports (8)

The Console item is active for both `"connections"` and `"terminals"`; clicking it opens the
session strip when sessions exist, otherwise the host list.

---

## Tokens

All colors are semantic `--color-*` tokens, which generate Tailwind utilities (`bg-panel`,
`text-muted`, `border-border`, …). Components never use raw hex values. The one exception is the
theme preview swatches in Appearance settings.

| Token | Role |
| --- | --- |
| `chrome` | Title bar and sidebar: the frame around the workspace |
| `surface` | Workspace / view background |
| `panel` | Cards, tables, settings groups |
| `surface-card` / `surface-elevated` | Aliases of `panel` / `surface-raised` |
| `surface-raised` | Floating layers: command palette, popovers, menus, modals, toasts. A visible step above `panel` in every theme |
| `surface-input` | Inputs, selects, search boxes |
| `surface-hover` / `surface-active` | Row hover / pressed state, active nav item |
| `selected` | Tint for the selected row or palette item; always paired with a 2 px `accent` left rail |
| `hover-on-floating` | Hover fill for items inside floating layers (they are lighter than `surface-hover`) |
| `surface-terminal` | xterm background |
| `border` / `border-subtle` / `border-hover` / `border-strong` | Decorative hairlines (panels, dividers, table rules, view-header rule), from quietest to strongest. Not for form controls |
| `border-control` | Boundary of inputs, selects, switch off-track, checkboxes. ≥ 3:1 on the surface behind it |
| `border-floating` | Edge of floating layers (`.popover`, `.modal-panel`, `.toast`, command palette) |
| `border-focus` | Focused control border and focus ring |
| `primary` / `secondary` / `muted` / `faint` | Text, from content down to disabled/decorative |
| `accent` / `accent-hover` / `accent-muted` / `on-accent` | Indigo signal: primary action, selection, focus, links; white text on indigo fills |
| `accent-strong` / `accent-strong-hover` | Primary button fill / hover fill. Hover is a real token, not `brightness()`, so it stays ≥ 4.5:1 with `on-accent` |
| `danger-strong` / `danger-strong-hover` / `on-danger` | Danger button fill / hover / text (≥ 4.5:1). `error`/`danger` are the lighter red for text and icons |
| `disabled-fg` | Text of disabled controls: ≥ 3:1 yet dimmer than `muted` |
| `success` / `warning` / `error` (`running`, `danger` aliases) | State only: never decoration |
| `overlay` | Modal backdrop |

**Color policy.** One accent, indigo, in every theme. It marks the primary action, focus,
selection, links and "live", nothing else. The primary button is a solid indigo fill with white
`on-accent` text (no shadow, no gradient). Labels and badges (team, role, source, counts) are
neutral; their icon or text says what they are. `success` / `warning` / `error` mark state only.
The only multicolor surface is the terminal, whose Ghostty-inspired ANSI palette represents program
output.

The `@theme` block is the cool obsidian Dark theme. `html.theme-light` overrides it with crisp cool
neutrals. Each theme also defines the xterm palette (`--term-*`, `--selection-bg`) that
`lib/utils/theme.ts` reads.

| Theme id (persisted) | Name in UI | Character |
| --- | --- | --- |
| `dark` | Dark | Cool obsidian chrome `#09090b`, surface `#0e0f12`, panels `#14161b`, indigo accent `#818cf8`, primary buttons `#5e6ad2` with white text (default) |
| `light` | Light | Cool neutral chrome `#e9ecf1`, surface `#f1f3f6`, panels `#fafafa`, indigo accent and primary buttons `#4f46e5` with white text |
| `system` | Match system | Resolves to `light` or `dark` from `prefers-color-scheme` and follows changes live; `<html data-theme="system" data-resolved-theme="…">` |

`THEME_OPTIONS` in `lib/utils/theme.ts` is the one list of pickable themes (onboarding, Appearance
and the palette read it). Themes from earlier versions load as their replacement (`zinc`,
`cyberpunk`, `oled`, `slate` → `dark`; `paper` → `light`), in both the Rust settings loader and
`normalizeTheme`.

**Density:** `data-density` on `<html>` (`compact` / `comfortable` / `spacious`) sets
`--row-height` to 32 / 40 / 48 px. List and table rows use `height: var(--row-height, 40px)`.

### Contrast rules (WCAG 2.2 AA)

Checked per theme by `desktop/scripts/check-contrast.mjs`, which parses `tokens.css` (the
`@theme` block plus each `html.theme-*` override):

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

Run it with `cd desktop && npm run check:contrast`; it prints one row per check and exits 1 on any
failure. CI runs it in the *Test Desktop* job. When you change a color token, run it before
committing.

**State rules**

- Use `border-control` (never `border`) for anything the user must locate and operate. Keep
  `border` for panel edges.
- Floating layers = `surface-raised` + `border-floating` + `shadow-lg`/`shadow-xl`, whose inset top
  highlight keeps the edge visible on near-black themes where drop shadows vanish.
- Selected rows and options: `selected` tint plus a 2 px accent left rail (`tr.is-selected`,
  `.menu-item-active`, `.nav-item-active`, `.settings-nav-item-active`, `.card-selected`,
  `[role=option][aria-selected=true]`). The rail is a shape, so the state does not depend on color
  alone.
- Disabled controls keep `disabled-fg` text on a flat fill, drop shadows, show a dashed edge on
  fields and use `cursor-not-allowed`. They are not dimmed with opacity. Disabled buttons keep
  pointer events so a `title` can explain why.
- `.tag` is a static label (square filled rectangle, mono caption, no border). `.chip` is an
  interactive filter (pill, subtle fill and border, 28 px high; `.chip-active` adds accent border,
  tint and a ✓). Don't use one for the other.
- Lamps are 10 px round jacks: `.lamp` is hollow (idle), `.lamp-live` is filled accent indigo,
  `.lamp-connecting` pulses opacity, `.lamp-error` is filled `error`. Always pair with a `title` or
  sr-only text; never color alone.
- Class-based hit areas are ≥ 24 px: `.btn-icon-sm` is 24 px, `.switch` and `.checkbox` extend
  their hit box with a transparent `::before`.

**Type.** UI text, view titles, modal titles and section eyebrows all use **IBM Plex Sans**
(`--font-sans`) at normal width (`font-stretch: normal`). **Archivo** (`--font-display`) is
reserved for the brand wordmark. **JetBrains Mono** (`--font-mono`) is for every machine value —
addresses, paths, counts, keys — and the terminals reference it by name. View titles (18 px/24 px)
and modal titles (15 px/20 px) are sentence case with `-0.02em` tracking and weight 600. Eyebrows,
sidebar section labels, table header cells and `.settings-group-title` use normal-width uppercase
IBM Plex Sans with `0.06em` tracking.

**Type scale.** Five sizes as `--type-*` tokens and classes, each with one job: `.type-title`
18 px/600 sans for view titles (`.view-title`) · `.type-section` and `.section-heading`
14 px/600 for section headings · `.type-body` 13 px for body/default · `.type-meta` 12 px muted for
meta · `.type-caption` 11 px/500 for badges, kbd and eyebrows. Weights are 400/500/600 only.
Arbitrary sizes in markup (`text-[10px]`) are not allowed; `.lamp-count` is the one fixed 10 px
primitive. Use `tabular-nums` (or `.tabular`) for numbers that line up.

**Radius:** `xs` 3 px · `sm` 4 px · `md` 6 px for controls · `lg` 8 px for cards and tables · `xl`
10 px for modals · `2xl` 12 px.

**Layout:** `--titlebar-h` 38px · `--header-h` 52px · `--sidebar-w` 224px /
`--sidebar-w-collapsed` 56px.

---

## Primitives

| Group | Classes | Notes |
| --- | --- | --- |
| Buttons | `.btn` + `-primary` `-secondary` `-ghost` `-danger` `-danger-ghost`; `.btn-sm` (28px) `.btn-lg` (36px); `.btn-icon` `.btn-icon-sm` `.btn-icon-danger` | Default height 32px. Primary is solid indigo with white `on-accent` text, no shadow; secondary sits on `panel` with a subtle `border-hover` edge. Icon buttons need `aria-label`. |
| Inputs | `.input` `.input-mono` `.input-invalid`, `textarea.input`; `.search-box > input` | Focus shows the `border-focus` border plus an accent-muted ring |
| Fields | `.field` `.field-label` `.field-hint` `.field-error` | Validation messages sit inline under the control |
| Toggles | `input.switch` (on/off settings), `input.checkbox` (selection) | Native checkboxes, restyled |
| Select | `CustomSelect` component (`label`, `size="sm"\|"md"`, `searchable`, `searchPlaceholder`) | Viewport-aware fixed-position keyboard navigable listbox with optional search filter |
| Badges | `.badge` + `-neutral` `-accent` `-success` `-warning` `-error`, `.badge-mono`; `.count`; `.tag` (static label); `.kbd` | Badges are 20 px, tight radius (`3px`), never pills. `.count` is square mono; `.tag` is a square mono caption; `.kbd` is a quiet mono keycap |
| Lamps | `.lamp` + `-live` `-connecting` `-error`, `.lamp-count` | 10 px round jack; `.lamp-count` is the optional mono session count beside it. Pair with text/title |
| Status | `.status-dot` + `-success` `-warning` `-error` `-offline` `-running`, `-sm`, `.status-dot-live` | |
| Text | `.eyebrow`, `.section-label`, `.nav-section-label`, `.settings-eyebrow`; `.link`, `.mono`, `.code-block`, `.view-count` | Eyebrows use normal-width IBM Plex Sans, 11 px, uppercase, `0.06em` |
| Diff | `.diff` > `.diff-line` + `-add` `-del` `-hunk` (`DiffView` component) | The +/- prefix carries the meaning, the tint only supports it. Make the `.diff` region focusable so long lines scroll by keyboard |
| Containers | `.panel` (+ `-header` `-title` `-body`), `.card` `.card-interactive` `.card-selected`, `.stat` (+ `-label` `-value`), `.divider` | |
| Tables | `.table-wrap > table.data-table`; `tr.is-selected`; `.row-actions` | Header cells are eyebrow style; row separators are `border-subtle` hairlines; the selected row keeps the accent rail. Actions are revealed on row hover, selection and focus |
| Navigation | `.segmented` `.segmented-item(-active)`, `.chip(-active)`, `.tabs` `.tab(-active)` | |
| Menus | `.popover`, `.menu-item(-active)(-danger)`, `.menu-separator` | |
| Feedback | `.alert` + `-error` `-warning` `-success` `-info`, `.alert-title`, `.alert-body`; `.toast`; `.skeleton`; `.spinner` | Use `.alert-body` for multi-line alerts |
| Empty states | `.empty-state` `-icon` `-title` `-desc` `-action` | One sentence of description, at most two actions |
| Modal | `ModalShell` component → `.modal-overlay` `.modal-panel`; `.modal-header` `.modal-title` `.modal-subtitle` `.modal-close` `.modal-body` `.modal-footer` | Sentence-case sans title (`15px`/20px); footer: secondary, then primary, right-aligned |
| Settings | `.settings-shell` `.settings-sidebar` `.settings-nav-item(-active)` `.settings-content` `.settings-page` `.settings-heading` `.settings-desc` `.settings-group` `.settings-group-title` `.setting-row` `.setting-row-main` `.setting-title` `.setting-meta` `.setting-control` `.system-value` | Group titles are uppercase sans (`0.06em`); rows inside a group are separated automatically |

If a pattern repeats in markup, it belongs in `components.css`; don't add a local variant.

---

## Writing

- Sentence case everywhere ("New host", "Run security audit"). Display type may uppercase visually;
  the DOM text and accessible names stay sentence case.
- Labels say what happens; avoid marketing terms ("high-performance", "studio").
- Errors say what failed and what to do next. Prefer an inline `.field-error` to a toast when the
  problem is in a form field.
- Relative times ("5 min ago") in lists, with the absolute date in the `title` tooltip
  (`formatRelative` / `formatDateTime` in `lib/utils/timezone.ts`).

## Accessibility

- Every interactive element is a real `<button type="button">`, link or input, and gets the global
  `:focus-visible` ring.
- Icon-only buttons have `aria-label`. Toggles and segmented controls expose `aria-pressed` /
  `aria-checked`. Sidebar items name their digit shortcut in `title` and `aria-keyshortcuts`.
- Modals trap focus, close on Escape and restore focus on close (`ModalShell`).
- Never encode meaning in color alone: status dots and host lamps carry a `title` or sr-only text,
  and badges carry text.
