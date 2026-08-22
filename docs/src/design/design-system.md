# Design System

Bayesian SSH's visual design system: tokens, primitives, and rules that keep every
screen consistent. This document is the single source of truth for how the desktop
GUI looks and behaves. The implementation lives in
[`desktop/src/lib/styles/`](../../../../desktop/src/lib/styles/) —
`tokens.css` (values), `base.css` (foundation), `components.css` (primitives) —
and is documented below in full.

> **Status:** the token layer, base styles, and component primitives are implemented
> and shared components (title bars, sidebar, modals, toasts, selects, loader,
> shortcut sheet, delete dialog) already consume them. Screens are being migrated to
> the primitives progressively; the rules below apply to all new and edited markup.

---

## Design Principles

| Principle | Meaning |
| --- | --- |
| **Premium developer tool** | Visuals serve the craft: precise spacing, restrained color, real data front and center. |
| **Fast and technical** | Monospace for data, dense rows, keyboard-first interaction, 100–240 ms motion. |
| **Calm, not flashy** | No gradients, glassmorphism, glow, bounce, or ping. One accent, quiet surfaces. |
| **Excellent dark mode** | Dark is the default and first-class. Light mode is token-ready (see [Light mode](#light-mode)). |
| **Keyboard-first** | Every action reachable by keyboard; focus rings are mandatory, shortcuts are documented in-product. |
| **Dense but never cluttered** | 11 px is the smallest *readable* size; hierarchy comes from weight and color, not size chaos. |
| **Strong hierarchy** | One primary action per view; eyebrow labels over panels; muted metadata below content. |
| **Motion communicates state** | Animations exist only to say "this changed / is entering". Nothing decorative. |
| **Usability over novelty** | If a pattern is unfamiliar, it's wrong. Reuse the primitives; never invent a fifth variant. |

---

## 1. Color Tokens

All colors are semantic tokens in `tokens.css`. Utilities are generated from the
`--color-*` namespace (`bg-surface`, `text-muted`, `border-error`, …).

| Token | Role | Zinc | Cyberpunk | OLED | Slate |
| --- | --- | --- | --- | --- | --- |
| `surface` | App background (window, page) | `#09090b` | `#061724` | `#000000` | `#0f172a` |
| `surface-raised` | Elevated layer: popovers, dropdowns, toasts | `#18181b` | `#0b2234` | `#0a0a0a` | `#1e293b` |
| `surface-hover` | Hover fill for rows / icon buttons | `#27272a` | `#13334a` | `#171717` | `#334155` |
| `surface-input` | Input & select fills | `#141417` | `#081d2c` | `#060606` | `#141d31` |
| `surface-terminal` | Terminal embedding surface | `#0c0d12` | `#061724` | `#000000` | `#0f172a` |
| `panel` | Cards, tables, side panels | `#141417` | `#081d2c` | `#060606` | `#141d31` |
| `border` | Hairlines between surfaces | `rgba(255,255,255,.08)` | `rgba(6,182,212,.15)` | `rgba(255,255,255,.06)` | `rgba(255,255,255,.07)` |
| `border-hover` | Stronger hairline on hover | `…16` | `…30` | `…12` | `…14` |
| `border-focus` | Focused control border | `rgba(59,130,246,.5)` | `rgba(6,182,212,.6)` | `rgba(96,165,250,.5)` | `rgba(56,189,248,.5)` |
| `primary` | Primary text (foreground) | `#f4f4f5` | `#ecfeff` | `#ffffff` | `#f8fafc` |
| `secondary` | Supporting text, labels | `#a1a1aa` | `#a5f3fc` | `#d4d4d8` | `#cbd5e1` |
| `muted` | Metadata, captions | `#71717a` | `#0891b2` | `#52525b` | `#64748b` |
| `accent` | Brand / interactive | `#3b82f6` | `#06b6d4` | `#60a5fa` | `#38bdf8` |
| `accent-hover` | Accent pressed/hover | `#2563eb` | `#0284c7` | `#3b82f6` | `#0ea5e9` |
| `accent-muted` | Accent tint (10–15 % alpha) | `rgba(59,130,246,.12)` | `rgba(6,182,212,.15)` | `rgba(96,165,250,.12)` | `rgba(56,189,248,.12)` |
| `success` | Completed operation (toast, saved) | `#10b981` | same | same | same |
| `running` | Live process / connection state | `#34d399` | same | same | same |
| `warning` | Degraded / expiring / attention | `#fbbf24` | same | same | same |
| `error` | Failure, destructive foreground | `#f87171` | same | same | same |
| `danger` | Legacy alias of `error` (compat) | `= error` | same | same | same |
| `on-accent` | Text on filled accent surfaces | `#ffffff` | `#ffffff` | `#ffffff` | `#ffffff` |
| `overlay` | Modal backdrop | `rgba(0,0,0,.65)` | same | same | same |

### Color rules

- **Never** hardcode palette colors (`text-emerald-400`, `bg-white/5`, `border-black/…`)
  in components. Use tokens only.
- `success` = one-shot confirmation. `running` = live/connected state. They share a
  hue family but are separate tokens because their usage rules differ (dot vs toast).
- `warning`/`error` at 400-level lightness are the *text* shades. Tinted surfaces use
  the same token with an alpha modifier (`bg-error/10`).
- `danger` exists only for backward compatibility; new code writes `error`.
- Terminal-palette colors (`--accent-cyan`, `--accent-pink`, …) are reserved for
  terminal/technical content — never for UI chrome.
- **Known issue:** white on `accent` is below WCAG AA for small text in zinc (3.7:1),
  oled (2.9:1) and cyberpunk (2.2:1). The `on-accent` token exists so the fix is a
  one-line change per theme (darken the accent for filled buttons). Tracked as
  follow-up work.

---

## 2. Typography Scale

UI type uses **Outfit** (weights 400/500/600; 700 only for emphasis). Data uses
**JetBrains Mono** (400/500/600). Stacks are tokenized (`--font-sans`,
`--font-mono`).

| Token | Size / Line-height | Usage |
| --- | --- | --- |
| `text-2xs` | 10 / 14 px | Eyebrow labels, table headers, badges, tags, kbd |
| `text-xs` | 11 / 15 px | Dense rows, status text, tabs, buttons |
| `text-sm` | 13 / 18 px | **Default body and control text** |
| `text-base` | 15 / 21 px | Section titles, emphasized text |
| `text-lg` | 18 / 25 px | Modal titles |
| `text-xl` | 20 / 28 px | Page headers (rare) |

### Type rules

- One size per purpose — if two elements look the same size, they should be the same
  token. **Never** invent arbitrary sizes (`text-[13px]`).
- Labels over sections: `eyebrow` (10 px, bold, uppercase, tracked).
- Metadata sits *below* content, in `text-muted`.
- Monospace is for machine data: addresses, commands, IDs, tags, timestamps in logs.
- Line-height comes from the token; never set `leading-*` on body text.

---

## 3. Spacing Scale

Tailwind's 4 px base grid (`--spacing: 0.25rem`) is the spacing system. All spacing
is a multiple of 4 (2 px and 6 px appear only as border/divider hairlines or icon
insets, never as layout gaps).

| Step | px | Typical use |
| --- | --- | --- |
| 0.5 | 2 | Icon-to-text inset in tiny flags |
| 1 | 4 | Tight icon gaps, dot-to-label |
| 1.5 | 6 | Button icon gaps, row-internal gaps |
| 2 | 8 | Between controls in a row, card padding (sm) |
| 2.5 | 10 | Button horizontal padding |
| 3 | 12 | Card/panel padding (md), stacked form fields |
| 4 | 16 | Page section gaps, modal padding |
| 5 | 20 | Between major regions |
| 6 | 24 | View padding from window edges |
| 8 | 32 | Between distinct blocks on a screen |

### Spacing rules

- Vertical rhythm: 4 → 8 → 12 → 16 → 24 → 32. Don't invent 14, 18, 22 px gaps.
- Group related controls at 8 px; separate groups at 16 px+.
- Consistent control height: inputs/buttons ≈ 32–34 px; rows 36–40 px.

---

## 4. Border Radius Rules

| Token | Value | Use for |
| --- | --- | --- |
| `rounded-xs` | 4 px | kbd caps, tags, tiny flags, dots |
| `rounded-sm` | 6 px | small chips, logo marks |
| `rounded-md` | 8 px | **Controls**: buttons, inputs, selects, icon buttons |
| `rounded-lg` | 12 px | Cards, tables, panels, empty-state containers |
| `rounded-xl` | 16 px | Modals, toasts, popovers |
| `rounded-full` | pill | Badges, status dots, count pills |

### Radius rules

- One radius per element type; controls are always `md`, surfaces `lg`, overlays `xl`.
- Never use `rounded-2xl+` on UI chrome (only the loader logo mark at 16 px).
- Hover states change color, never radius.

---

## 5. Iconography Rules

Icons come from **lucide-svelte** (1.5 px stroke, outline style) and are always
`currentColor`.

| Size | Use |
| --- | --- |
| 12–13 | Row-level actions, status rows |
| 14 | Buttons, compact controls |
| 16 | Nav items, headers, toasts |
| 18 | Modal titles, section headers |
| 20–28 | Empty-state icons (inside a 48 px tile) |

### Icon rules

- Icons communicate meaning; they are never decorative fills or duotones.
- Color follows semantics: accent = interactive, `running`/`warning`/`error` = state,
  `muted` = passive. Hover moves icon + text to `primary` together.
- If an icon needs a label, show the label. Icon-only buttons must have
  `title` + `aria-label`.
- Prefer the existing icon set; add a new import only when no lucide icon fits.

---

## 6. Button Hierarchy

One primary action per view; secondary for peers; ghost for toolbar-level actions;
destructive only for irreversible ones. All sizes/colors live in `components.css`.

| Class | Look | Use |
| --- | --- | --- |
| `btn btn-primary` | Accent fill, `on-accent` text | The one action the view wants (Add Host, Connect, Save) |
| `btn btn-secondary` | Input surface + hairline | Equal-weight actions (Cancel, Browse, Ping All) |
| `btn btn-ghost` | Transparent, hover fill | Toolbar / inline actions that shouldn't compete |
| `btn btn-danger` | Error tint + hairline | Irreversible actions (Delete, Terminate) |
| `btn btn-icon` | Icon-only, hover fill | Row actions, dismiss buttons |
| `btn btn-sm` / `btn-lg` | Tighter / roomier | Density adjustments, modal CTAs |

### Button rules

- Never more than one `btn-primary` per view. In dialogs, the primary sits last.
- Destructive never shares a row with primary on equal footing; Cancel is secondary.
- Disabled: `opacity-50`, keep the shape (no disabled fills).
- Text: 11 px semibold (`text-xs`); large CTAs 13 px (`btn-lg`).
- Focus: the global focus-visible ring (section 16) applies to all buttons.

---

## 7. Input & Search Styling

| Pattern | Class | Look |
| --- | --- | --- |
| Text field | `input` | Input surface, hairline, 13 px text, `muted` placeholder |
| Search field | `search-box` (wrapper) + bare `input` inside | Same, plus a leading search icon and clear button |

### Input rules

- Focus = `border-accent` + 2 px `accent-muted` ring. Never remove the ring.
- One height everywhere (`py-2`); labels are 11 px `secondary` above the field.
- Helper text below is 11 px `muted`; errors below are 11 px `error`.
- Search: leading icon `muted`, clear button appears only when non-empty, Escape
  clears and blurs.
- `CustomSelect` is the dropdown select; native `<select>` styling is global and
  token-driven for plain forms.

---

## 8. Tags & Badges

| Pattern | Class | Look / use |
| --- | --- | --- |
| Tag | `tag` | `#label` metadata: mono, 10 px, neutral chip |
| Badge | `badge` + `badge-accent / badge-success / badge-running / badge-warning / badge-error / badge-neutral` | Counts and states: pill, 10 px bold uppercase |
| Flag | `flag` + `flag-accent` | Tiny feature markers (`krb5`, `jump`): 9 px bold uppercase |

### Tag/badge rules

- Tags describe *data* (connection tags); badges describe *state or count*.
- One badge color per semantic state; never decorative colors on badges.
- Flags are for compact technical markers in dense rows — keep them to 1–2 per row.

---

## 9. Connection Status Indicators

| State | Class | Meaning |
| --- | --- | --- |
| Online / live | `status-dot status-dot-running` (+ `status-dot-pulse` for live) | Connected, session active, agent running |
| Completed | `status-dot status-dot-success` | Last check succeeded (static) |
| Degraded | `status-dot status-dot-warning` | Expiring ticket, partial reachability |
| Failed | `status-dot status-dot-error` | Unreachable, failed |
| Offline / unknown | `status-dot status-dot-offline` | Never used, not checked |

### Status rules

- Dot + short label together; the dot never carries the meaning alone in a table.
- `status-dot-pulse` is the only allowed continuous animation: a 2 s soft ring.
- Status rows (Agent, Kerberos in the sidebar) use the same tokens as text/icon
  color: `running` for live, `warning` for degraded, `error` for failed.
- Ping latency, when shown, is mono 11 px `secondary`.

---

## 10. Toast / Notification Patterns

Toast = transient confirmation of a completed action. Implementation:
`Toast.svelte` + `notifications.svelte.ts` (`notify(text, type)`).

| Type | Classes | Icon |
| --- | --- | --- |
| Success | `toast toast-success` | `CheckCircle2` → `text-success` |
| Error | `toast toast-error` | `AlertCircle` → `text-error` |
| Info | `toast toast-info` | `Info` → `text-accent` |

### Toast rules

- One toast at a time, bottom-right, auto-dismiss after ~3 s (errors may hold longer).
- Tinted border (`/40`) on the raised surface; never a filled toast.
- 13 px text, icon 16 px, dismiss button always available.
- `role="status"` / `role="alert"` + `aria-live` — toasts are announced.
- Toasts confirm *completed* work. In-progress work uses loading states (section 13);
  persistent problems use alerts (section 14).

---

## 11. Modal / Dialog Patterns

Implementation: `ModalShell.svelte` (focus trap, Escape, backdrop close) + the
`.modal-overlay` / `.modal-panel` primitives.

| Width | Class | Use |
| --- | --- | --- |
| `sm` | `max-w-sm` | Confirms (delete, quit) |
| `md` | `max-w-md` | Forms (connection, env, agent) |
| `lg` | `max-w-3xl` | Rich content (batch exec, snippets) |
| `full` | full-bleed | Workspaces (terminal detach) |

### Modal rules

- Overlay: `bg-overlay` + `backdrop-blur-sm`; panel: `surface` + hairline + `xl`
  radius + `shadow-xl`; entrance 250 ms `ease-out` (translate + fade).
- Title row with the lucide icon at 18 px + `text-base` semibold; close button is
  `btn-icon`.
- Focus moves to the first focusable control; Tab cycles inside; Escape closes;
  focus returns to the trigger on close.
- Buttons bottom-right, primary last; destructive confirms use the error-tinted
  panel treatment from `DeleteConfirm`.
- One modal at a time — no stacked dialogs.

---

## 12. Empty States

Empty states teach the first action. Classes: `empty-state` (+ `-dashed` container),
`empty-state-icon`, `empty-state-title`, `empty-state-desc`, `empty-state-action`.

```svelte
<div class="empty-state empty-state-dashed">
  <div class="empty-state-icon"><Server size={24} /></div>
  <span class="empty-state-title">No SSH Connections</span>
  <span class="empty-state-desc">Add your first remote host or import from OpenSSH config.</span>
  <div class="empty-state-action">
    <button class="btn btn-primary" onclick={onAddHost}><Plus size={14} /> Add Connection</button>
  </div>
</div>
```

### Empty-state rules

- Icon (24 px) inside a 48 px `surface-input` tile — never a giant floating icon.
- Title 13 px semibold, one line; description 11 px muted, at most two lines.
- Exactly one primary action. No empty state without an action.
- Search-with-no-results uses the same pattern with a `Search` icon and a "clear
  filters" ghost action.

---

## 13. Loading States

| Situation | Pattern |
| --- | --- |
| App startup | `AppLoader` — logo tile, title, subtitle, 1 px indeterminate bar |
| View/data fetch | Skeleton rows: `.skeleton` (shimmer, 1.6 s, low contrast) |
| In-place action | `animate-spin` icon inside the button (e.g. `RefreshCw` during ping) |

### Loading rules

- Never block the window with a spinner for data the user can keep reading.
- Skeletons mimic final layout (same row heights), not abstract circles.
- Spinners are 14–16 px, `text-accent`, inside the control that initiated the work.
- No bounce, ping, glow, or gradient loaders — motion only communicates "working".
- `prefers-reduced-motion` disables all of it globally.

---

## 14. Error States

| Situation | Pattern |
| --- | --- |
| Inline failure | `.alert alert-error` — icon + message inside the flow |
| Form field | 11 px `error` text below the field |
| View-level failure | Empty-state-style block with `alert-error` and a Retry `btn-secondary` |
| Destructive confirm | `DeleteConfirm` — error icon tile, tinted panel border |

### Alert classes

`alert` + one of `alert-error`, `alert-warning`, `alert-success`, `alert-info`
(optionally `alert-title` for a bold first line).

### Error rules

- Errors are specific: what failed and what to do next. No generic "Error occurred".
- Error text is `error` (#f87171) at 11–13 px on tinted `error/8–10` surfaces.
- Retry is always available for transient failures; destructive text is never
  softened into a warning.

---

## 15. Keyboard Shortcut Presentation

Presentation class: `.kbd` (neutral) and `.kbd-accent` (highlighted key).

```svelte
<kbd class="kbd kbd-accent">Ctrl + K</kbd>
```

### Rules

- kbd = 10 px mono, 20 px cap, `surface-raised` + hairline + `shadow-sm`; `accent`
  tint marks the *primary* key of a screen.
- The shortcuts guide (`ShortcutsModal`, `?` / `F1` / `Ctrl+/`) is the canonical
  listing: description left, keycap right, grouped by function, `Esc` to close.
- Every global shortcut must appear in the guide; new shortcuts update it in the
  same change.
- Shortcut *affordances* in chrome (e.g. "Press / to focus") are `text-muted`
  11 px with an inline `.kbd`.

---

## 16. Focus & Selection States

### Focus (keyboard)

- Global rule in `base.css`: every interactive element gets a 2 px `accent` outline
  (+2 px offset) on `:focus-visible`. Never fight it with `outline-none`.
- Inputs/selects additionally get `border-accent` + 2 px `accent-muted` ring.
- The search box uses `focus-within` on its wrapper (the box is the control).
- Elements with a custom ring (e.g. `CustomSelect` when open) may suppress the
  outline — that's the only allowed exception.

### Selection (list/rows)

| State | Class |
| --- | --- |
| Row hover | `.row` (hover fill `surface-hover/40`) |
| Row selected | `.row-active` (`accent/10` fill) + `text-primary` + semibold |
| Grid card selected | `border-accent` + `accent/5` fill |

### Selection rules

- Selection is always visible in both list and grid modes; keyboard navigation
  (↑/↓, Enter) moves it.
- Accent-tinted selection (`accent/10`) is the only allowed selection treatment —
  no full-accent rows.
- Focus and selection are distinct: focus ring = *where you are*, tint = *what's
  chosen*. Never render one without the other.

---

## Theme System & Light Mode

Theming is CSS-variable-driven: `tokens.css` declares the canonical `@theme` block
(default = zinc), and `html.theme-*` classes override per theme (zinc, cyberpunk,
oled, slate). `theme.ts` applies the class, syncs the native window background, and
derives the xterm palette from plain `--*` vars (`--bg-terminal`, `--text-primary`,
`--selection-bg`, …) declared in each theme block.

### Light mode

The architecture supports it: every value is a token, so a light theme is a new
`html.theme-*-light` block + one picker entry. It is **not shipped yet** for two
reasons: (1) several screens still use white-alpha fills (`bg-white/5`,
`hover:bg-white/[0.04]`) that vanish on light surfaces — the token sweep (sections
1–16) must land first; (2) `on-accent` contrast needs per-theme tuning. The path:

1. Replace remaining white/black-alpha fills with `surface-hover` / `surface-input`
   tokens (in progress).
2. Add `html.theme-zinc-light` block + `THEME_WINDOW_BG` entry + settings option.
3. Re-tune `on-accent` and shadow opacities for light surfaces.

New code must never introduce a raw alpha fill so the light theme stays one block
away.

---

## Component Class Reference

All primitives live in `components.css` under `@layer components` and are theme-
aware by construction (they reference tokens only):

`btn`, `btn-primary`, `btn-secondary`, `btn-ghost`, `btn-danger`, `btn-icon`,
`btn-sm`, `btn-lg` · `input`, `search-box` · `tag` · `badge`, `badge-neutral`,
`badge-accent`, `badge-success`, `badge-running`, `badge-warning`, `badge-error` ·
`flag`, `flag-accent` · `status-dot`, `status-dot-sm`, `status-dot-running`,
`status-dot-success`, `status-dot-warning`, `status-dot-error`, `status-dot-offline`,
`status-dot-pulse` · `kbd`, `kbd-accent` · `eyebrow`, `table-header`, `link` ·
`panel`, `divider`, `row`, `row-active` · `alert`, `alert-error`, `alert-warning`,
`alert-success`, `alert-info`, `alert-title` · `empty-state`, `empty-state-icon`,
`empty-state-title`, `empty-state-desc`, `empty-state-action`, `empty-state-dashed` ·
`skeleton` · `toast`, `toast-success`, `toast-error`, `toast-info` · `modal-overlay`,
`modal-panel` · utility `scrollbar-none`

### Motion reference

| Token | Value | Use |
| --- | --- | --- |
| `--transition-duration-fast` | 100 ms | Hover/active color changes |
| `--transition-duration-base` | 160 ms | Standard UI transitions |
| `--transition-duration-slow` | 240 ms | Panels, modal content |
| `--ease-out` | `cubic-bezier(0.16,1,0.3,1)` | Entrances (modal, toast, popover) |
| keyframes | `modal-enter` 250 ms, `toast-enter` 300 ms, `popover-enter` 150 ms, `status-pulse` 2 s, `skeleton-shimmer` 1.6 s, `loader-slide` 1.2 s | as named |

---

## Migration Checklist (per screen)

When touching a screen, in order:

1. Replace arbitrary sizes with scale tokens (`text-[11px]` → `text-xs`, …).
2. Replace raw palette colors with semantic tokens.
3. Swap repeated markup for primitives (`btn btn-secondary`, `tag`, `badge`,
   `status-dot`, `kbd`, `alert`, `empty-state`).
4. Remove `outline-none` from interactive elements; keep custom rings only where
   they replace it.
5. Verify: build, `svelte-check`, and a visual pass in every theme.
