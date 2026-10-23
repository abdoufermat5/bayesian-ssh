#!/usr/bin/env node
// WCAG contrast regression check for the design tokens.
// Parses src/lib/styles/tokens.css per theme (the @theme block is the
// default Dark theme, html.theme-light overrides it) and
// asserts the contrast rules documented in desktop/DESIGN-SYSTEM.md.
// Exit code 1 on any failure.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const css = readFileSync(join(here, "../src/lib/styles/tokens.css"), "utf8");

function declarations(body) {
  const out = {};
  for (const m of body.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) out[m[1]] = m[2].trim();
  return out;
}
function block(selectorRe) {
  const m = css.match(new RegExp(selectorRe + "\\s*\\{([^}]*)\\}"));
  if (!m) throw new Error(`block not found: ${selectorRe}`);
  return declarations(m[1].replace(/\/\*[\s\S]*?\*\//g, ""));
}

const base = block("@theme");
const THEMES = {
  dark: { label: "Dark", vars: { ...base, ...block("html\\.theme-dark") } },
  light: { label: "Light", vars: { ...base, ...block("html\\.theme-light") } },
};

function parseColor(v) {
  v = v.trim();
  let m = v.match(/^#([0-9a-f]{6})$/i);
  if (m) {
    const n = parseInt(m[1], 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255, 1];
  }
  m = v.match(/^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*(?:,\s*([\d.]+)\s*)?\)$/);
  if (m) return [+m[1], +m[2], +m[3], m[4] === undefined ? 1 : +m[4]];
  throw new Error(`unsupported color: ${v}`);
}
const over = (fg, bg) => {
  const a = fg[3];
  return [0, 1, 2].map((i) => fg[i] * a + bg[i] * (1 - a)).concat(1);
};
const lum = ([r, g, b]) => {
  const f = (c) => ((c /= 255) <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
};
const ratio = (a, b) => {
  const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
};

const rows = [];
let failed = 0;
for (const [id, { label, vars }] of Object.entries(THEMES)) {
  const c = (name) => {
    name = "--color-" + name;
    if (!(name in vars)) throw new Error(`${id}: missing token ${name}`);
    return parseColor(vars[name]);
  };
  const check = (rule, fg, bg, min) => {
    const r = ratio(fg, bg);
    const ok = r >= min;
    if (!ok) failed++;
    rows.push({ theme: label, rule, ratio: r.toFixed(2), min, ok });
  };
  const surfaces = ["surface", "panel", "surface-raised", "surface-input", "surface-hover", "surface-active"];
  const selected = over(c("selected"), c("panel"));
  const selectedRaised = over(c("selected"), c("surface-raised"));

  // Text on surfaces
  for (const s of surfaces) {
    check(`primary on ${s}`, c("primary"), c(s), 4.5);
    check(`secondary on ${s}`, c("secondary"), c(s), 4.5);
    check(`muted on ${s}`, c("muted"), c(s), 4.5);
  }
  check("chrome: muted on chrome", c("muted"), c("chrome"), 4.5);
  check("chrome: secondary on chrome", c("secondary"), c("chrome"), 4.5);
  check("muted on selected row (accent tint on panel)", c("muted"), selected, 4.5);
  check("muted on selected palette item (tint on raised)", c("muted"), selectedRaised, 4.5);
  check("primary on selected row", c("primary"), selected, 4.5);
  check("accent text on panel", c("accent"), c("panel"), 4.5);
  check("accent text on surface", c("accent"), c("surface"), 4.5);
  for (const k of ["success", "warning", "error"]) check(`${k} text on panel`, c(k), c("panel"), 4.5);

  // Control boundaries (WCAG 1.4.11): against the surface they sit on and their own fill
  for (const s of ["surface", "panel", "surface-raised"]) {
    check(`control border on ${s}`, c("border-control"), c(s), 3);
  }
  check("control border on surface-input", c("border-control"), c("surface-input"), 3);
  check("checkbox/switch off border on surface-active", c("border-control"), c("surface-active"), 3);
  check("focus ring on surface", c("border-focus"), c("surface"), 3);
  check("focus ring on surface-raised", c("border-focus"), c("surface-raised"), 3);

  // Buttons
  check("primary button text", c("on-accent"), c("accent-strong"), 4.5);
  check("primary button text (hover)", c("on-accent"), c("accent-strong-hover"), 4.5);
  check("danger button text", c("on-danger"), c("danger-strong"), 4.5);
  check("danger button text (hover)", c("on-danger"), c("danger-strong-hover"), 4.5);
  check("danger-ghost text on panel", c("error"), c("panel"), 4.5);

  // Disabled: legible but dimmer than muted
  check("disabled text on panel", c("disabled-fg"), c("panel"), 3);
  check("disabled text on surface-input", c("disabled-fg"), c("surface-input"), 3);

  check("disabled text on surface-active (disabled buttons)", c("disabled-fg"), c("surface-active"), 3);
  check("disabled text on surface-subtle (disabled fields)", c("disabled-fg"), c("surface-subtle"), 3);
  const hov = over(c("hover-on-floating"), c("surface-raised"));
  check("secondary on menu hover (raised + overlay)", c("secondary"), hov, 4.5);
  check("primary on menu hover (raised + overlay)", c("primary"), hov, 4.5);
  check("muted on menu hover (raised + overlay)", c("muted"), hov, 4.5);
  const errHover = over([...c("error").slice(0, 3), 0.12], c("surface-raised"));
  check("error text on raised", c("error"), c("surface-raised"), 4.5);
  check("danger menu item hover (error 12% on raised)", c("error"), errHover, 4.5);

  // Floating layer must be distinct from the surface behind it
  check("floating layer (surface-raised) vs surface", c("surface-raised"), c("surface"), 1.1);
  check("floating border vs floating", c("border-floating"), c("surface-raised"), 1.5);
}

const w = Math.max(...rows.map((r) => r.rule.length));
console.log(`${"theme".padEnd(9)} ${"rule".padEnd(w)}  ratio  min  result`);
for (const r of rows) {
  console.log(`${r.theme.padEnd(9)} ${r.rule.padEnd(w)}  ${r.ratio.padStart(5)}  ${String(r.min).padStart(3)}  ${r.ok ? "ok" : "FAIL"}`);
}
console.log(`\n${rows.length - failed}/${rows.length} checks passed`);
if (failed) {
  console.error(`${failed} contrast check(s) failed`);
  process.exit(1);
}
