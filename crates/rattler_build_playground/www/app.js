import wasmInit, * as wasmApi from './rattler_build_playground.js';

import {
  EditorView, keymap, lineNumbers, highlightActiveLineGutter, highlightSpecialChars,
  drawSelection, dropCursor, rectangularSelection, crosshairCursor, highlightActiveLine,
} from '@codemirror/view';
import { EditorState } from '@codemirror/state';
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
import { searchKeymap, highlightSelectionMatches } from '@codemirror/search';
import { autocompletion, closeBrackets, closeBracketsKeymap, completionKeymap } from '@codemirror/autocomplete';
import {
  defaultHighlightStyle, syntaxHighlighting, indentOnInput, bracketMatching, foldGutter, foldKeymap,
} from '@codemirror/language';
import { yaml } from '@codemirror/lang-yaml';
import { oneDark } from '@codemirror/theme-one-dark';

// ===== Constants =====

const PINNING_URL = 'https://raw.githubusercontent.com/conda-forge/conda-forge-pinning-feedstock/main/recipe/conda_build_config.yaml';
const PREFERRED_PLATFORMS = ['linux-64', 'osx-arm64', 'osx-64', 'win-64', 'linux-aarch64', 'noarch'];

const DEFAULT_RECIPE = `context:
  name: numpy
  version: "2.2.3"

package:
  name: \${{ name }}
  version: \${{ version }}

source:
  url: https://pypi.io/packages/source/\${{ name[0] }}/\${{ name }}/numpy-\${{ version }}.tar.gz
  sha256: dbdc15f0c81611925f382dfa97b3bd0bc2c1ce19d4fe50482cb0ddc12ba30020

build:
  number: 0
  script:
    - python -m pip install . -vv

requirements:
  host:
    - python
    - pip
    - cython
    - numpy-base
  run:
    - python
    - numpy-base

tests:
  - python:
      imports:
        - numpy
        - numpy.linalg

about:
  homepage: https://numpy.org/
  license: BSD-3-Clause
  summary: The fundamental package for scientific computing with Python.
`;

const DEFAULT_VARIANTS = `python:
  - "3.11"
  - "3.12"
  - "3.13"
`;

const EX_RICH = `context:
  version: "13.9.4"

package:
  name: rich
  version: \${{ version }}

source:
  url: https://pypi.org/packages/source/r/rich/rich-\${{ version }}.tar.gz
  sha256: 439594978a49a09530cff7ebc4b5c7103ef57baf48d5ea3184f21d9a2befa098

build:
  noarch: python
  script: python -m pip install . -vv --no-deps

requirements:
  host:
    - python >=3.8
    - pip
    - poetry-core
  run:
    - python >=3.8
    - markdown-it-py >=2.2.0
    - pygments >=2.13.0,<3.0.0

tests:
  - python:
      imports:
        - rich

about:
  homepage: https://github.com/Textualize/rich
  license: MIT
  summary: Render rich text, tables, progress bars and more to the terminal
`;

const EX_XTENSOR = `context:
  version: "0.25.0"

package:
  name: xtensor
  version: \${{ version }}

source:
  url: https://github.com/xtensor-stack/xtensor/archive/\${{ version }}.tar.gz
  sha256: 32d5d9fd23998c57e746c375a544edf544b74f0a18ad6bc3c38cbba968d5e6c7

build:
  number: 0

requirements:
  build:
    - \${{ compiler('cxx') }}
    - cmake
    - ninja
  host:
    - xtl >=0.7,<0.8
  run:
    - xtl >=0.7,<0.8
  run_constraints:
    - xsimd >=8.0.3,<10

tests:
  - script:
      - if: unix
        then:
          - test -d \${PREFIX}/include/xtensor

about:
  homepage: https://github.com/xtensor-stack/xtensor
  license: BSD-3-Clause
  summary: C++ tensors with broadcasting and lazy computing
`;

const EXAMPLES = [
  { key: 'numpy', label: 'numpy', recipe: DEFAULT_RECIPE },
  { key: 'rich', label: 'rich · noarch', recipe: EX_RICH },
  { key: 'xtensor', label: 'xtensor · C++', recipe: EX_XTENSOR },
];

const GEN_PLACEHOLDERS = { pypi: 'package name', cran: 'R package', cpan: 'Perl distribution' };

// ===== Small helpers =====

function escapeHtml(str) {
  return String(str)
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;').replace(/'/g, '&#39;');
}

// Lucide icon paths (https://lucide.dev) rendered as inline SVG strings.
function lucide(paths, size, sw) {
  return '<svg width="' + (size || 14) + '" height="' + (size || 14) + '" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="' + (sw || 2) + '" stroke-linecap="round" stroke-linejoin="round" style="flex-shrink:0;">' + paths + '</svg>';
}
const ICONS = {
  chevronRight: lucide('<path d="m9 18 6-6-6-6"/>', 13, 2.4),
  arrowLeft: lucide('<path d="m12 19-7-7 7-7"/><path d="M19 12H5"/>', 15, 2),
};

// ===== State =====

const state = {
  platforms: [],
  platform: localStorage.getItem('rbp-platform') || 'linux-64',
  mode: localStorage.getItem('rbp-mode') || 'variants',
  variantFormat: localStorage.getItem('rbp-format') || 'variants',
  variantsView: localStorage.getItem('rbp-variants-view') || 'matrix',
  theme: localStorage.getItem('rbp-theme') || 'light',
  layout: localStorage.getItem('rbp-dir') || 'a',
  genSource: localStorage.getItem('rbp-gensrc') || 'pypi',
  status: '…',
  hasError: false,
  usedVars: [],
  selectedVariant: null,
};

let recipeText = localStorage.getItem('rbp-recipe') || DEFAULT_RECIPE;
let variantsText = localStorage.getItem('rbp-variants') || DEFAULT_VARIANTS;
let outputData = null;
let wasm = null;
let recipeView = null;
let varsView = null;
let pinningCache = null;
let debounceTimer = null;

function lsSet(k, v) { try { localStorage.setItem(k, v); } catch (e) { /* ignore */ } }

// ===== DOM refs =====

const $ = (id) => document.getElementById(id);
const root = document.documentElement;
const platformSelect = $('platform-select');
const recipeMount = $('recipe-mount');
const variantsMount = $('variants-mount');
const outputMount = $('output-mount');
const statusBadge = $('status-badge');
const usedVarsEl = $('used-vars');
const variantsFileTab = $('variants-file-tab');
const genWrap = $('gen-wrap');
const genPanel = $('gen-panel');
const genSourceEl = $('gen-source');
const genPkgEl = $('gen-pkg');
const genVerEl = $('gen-ver');
const genRunEl = $('gen-run');
const pinBtn = $('pin-btn');
const pinLabel = $('pin-label');
const copyBtn = $('copy-btn');
const loadingOverlay = $('loading-overlay');
const errorOverlay = $('error-overlay');

// ===== CodeMirror editors =====

// basicSetup, assembled from the individual packages (matches the bundle that
// the `codemirror` meta-package ships) so it loads through our import map.
const basicSetup = [
  lineNumbers(),
  highlightActiveLineGutter(),
  highlightSpecialChars(),
  history(),
  foldGutter(),
  drawSelection(),
  dropCursor(),
  EditorState.allowMultipleSelections.of(true),
  indentOnInput(),
  syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
  bracketMatching(),
  closeBrackets(),
  autocompletion(),
  rectangularSelection(),
  crosshairCursor(),
  highlightActiveLine(),
  highlightSelectionMatches(),
  keymap.of([
    ...closeBracketsKeymap,
    ...defaultKeymap,
    ...searchKeymap,
    ...historyKeymap,
    ...foldKeymap,
    ...completionKeymap,
    indentWithTab,
  ]),
];

function editorTheme(dark) {
  return EditorView.theme({
    '&': { height: '100%', fontSize: '13px', backgroundColor: 'transparent' },
    '.cm-scroller': { overflow: 'auto', lineHeight: '1.6', padding: '6px 0', fontFamily: "'JetBrains Mono','DM Mono',monospace" },
    '.cm-gutters': { backgroundColor: 'transparent', border: 'none', color: dark ? '#5e656e' : '#b5b7ba' },
    '.cm-activeLine': { backgroundColor: dark ? 'rgba(255,255,255,0.035)' : 'rgba(0,29,56,0.035)' },
    '.cm-activeLineGutter': { backgroundColor: 'transparent', color: dark ? '#b2b7bd' : '#8b8e93' },
    '.cm-lineNumbers .cm-gutterElement': { padding: '0 8px 0 14px' },
    '&.cm-focused': { outline: 'none' },
    '.cm-content': { caretColor: dark ? '#ffd432' : '#001d38' },
    '.cm-cursor, .cm-dropCursor': { borderLeftColor: dark ? '#ffd432' : '#001d38' },
    '&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection': { backgroundColor: dark ? 'rgba(255,212,50,0.2)' : 'rgba(87,115,255,0.16)' },
    '.cm-foldPlaceholder': { backgroundColor: 'transparent', border: 'none', color: dark ? '#b2b7bd' : '#8b8e93' },
  }, { dark });
}

function makeEditor(parent, doc, onChange) {
  const dark = state.theme === 'dark';
  return new EditorView({
    doc,
    parent,
    extensions: [
      basicSetup,
      EditorView.lineWrapping,
      yaml(),
      editorTheme(dark),
      dark ? oneDark : [],
      EditorView.updateListener.of((u) => { if (u.docChanged) onChange(u.state.doc.toString()); }),
    ],
  });
}

function mountEditors() {
  recipeView = makeEditor(recipeMount, recipeText, onRecipeChange);
  varsView = makeEditor(variantsMount, variantsText, onVariantsChange);
}

// Theme changes restyle syntax highlighting, so the simplest faithful path is
// to rebuild both editors (preserving their current text).
function rebuildEditors() {
  if (recipeView) { recipeText = recipeView.state.doc.toString(); recipeView.destroy(); recipeView = null; }
  if (varsView) { variantsText = varsView.state.doc.toString(); varsView.destroy(); varsView = null; }
  mountEditors();
}

function onRecipeChange(text) { recipeText = text; lsSet('rbp-recipe', text); scheduleUpdate(); }
function onVariantsChange(text) { variantsText = text; lsSet('rbp-variants', text); scheduleUpdate(); }

function setRecipeText(text) {
  recipeText = text;
  lsSet('rbp-recipe', text);
  if (recipeView) recipeView.dispatch({ changes: { from: 0, to: recipeView.state.doc.length, insert: text } });
  runEval();
}

function scheduleUpdate() {
  clearTimeout(debounceTimer);
  debounceTimer = setTimeout(runEval, 300);
}

// ===== Evaluation =====

function runEval() {
  if (!wasm) return;
  const src = recipeText;
  const vy = variantsText && variantsText.trim() ? variantsText : '{}';
  const plat = state.platform;
  const mode = state.mode;
  const fmt = state.variantFormat;
  const start = performance.now();
  let data;
  try {
    if (mode === 'stage0') {
      const r = JSON.parse(wasm.parse_recipe(src));
      data = { ok: r.ok, kind: 'yaml', result_html: r.result_html, error: r.error };
    } else {
      const r = JSON.parse(wasm.render_variants(src, vy, plat, fmt));
      data = { ok: r.ok, kind: 'variants', summary: r.result && r.result.summary, variants_html: r.result && r.result.variants_html, error: r.error };
    }
  } catch (e) {
    data = { ok: false, kind: mode === 'stage0' ? 'yaml' : 'variants', error: { message: String(e) } };
  }
  const elapsed = (performance.now() - start).toFixed(1);
  outputData = data;
  state.selectedVariant = null;

  let used = [];
  try { const u = JSON.parse(wasm.get_used_variables(src)); if (u.ok && Array.isArray(u.result)) used = u.result; } catch (e) { /* ignore */ }
  state.usedVars = used;

  setStatus(data.ok ? (elapsed + ' ms') : 'error', !data.ok);
  renderUsedVars();
  paintOutput();
}

// ===== Output painters =====

function placeholder(msg) {
  return '<div style="display:flex;flex-direction:column;align-items:center;justify-content:center;gap:14px;height:100%;color:var(--fg3);text-align:center;padding:28px;"><img src="assets/paxton.png" alt="" style="width:78px;height:auto;opacity:.6;"><div style="font:400 13px/1.5 Inter,sans-serif;max-width:300px;">' + msg + '</div></div>';
}

function errorHtml(error) {
  const e = error || { message: 'Unknown error' };
  const loc = (e.line != null) ? '<div style="margin-top:8px;font:12px/1.4 \'JetBrains Mono\',monospace;color:var(--fg3);">at line ' + e.line + ', column ' + (e.column || 0) + '</div>' : '';
  return '<div style="padding:18px;height:100%;overflow:auto;"><div style="display:inline-flex;align-items:center;gap:7px;margin-bottom:10px;font:600 11px/1 Inter,sans-serif;text-transform:uppercase;letter-spacing:.06em;color:var(--bad);"><span style="width:7px;height:7px;border-radius:50%;background:var(--bad);"></span>Evaluation error</div><div style="padding:14px 16px;border:1px solid rgba(255,107,56,.4);background:rgba(255,107,56,.08);border-radius:10px;color:var(--fg1);font:13px/1.55 \'JetBrains Mono\',monospace;white-space:pre-wrap;word-break:break-word;">' + escapeHtml(e.message) + loc + '</div></div>';
}

function yamlSurface(html) {
  return '<div style="height:100%;overflow:auto;background:#1e1e2e;"><pre class="output-yaml" style="margin:0;padding:16px 18px;font:13px/1.55 \'JetBrains Mono\',monospace;white-space:pre;min-width:max-content;">' + (html || '') + '</pre></div>';
}

function badge(text, kind) {
  const styles = {
    green: 'background:rgba(112,192,56,.16);color:#56961f;border:1px solid rgba(112,192,56,.4);',
    flag: 'background:rgba(87,115,255,.14);color:var(--link);border:1px solid rgba(87,115,255,.35);',
    skip: 'background:var(--inset);color:var(--fg3);border:1px solid var(--b2);',
  };
  return '<span style="display:inline-block;font:600 9.5px/1.4 Inter,sans-serif;text-transform:uppercase;letter-spacing:.05em;padding:2px 6px;border-radius:5px;' + (styles[kind] || styles.skip) + '">' + escapeHtml(text) + '</span>';
}

function yamlBtn() {
  return '<button style="appearance:none;display:inline-flex;align-items:center;gap:5px;font:600 10.5px/1 Inter,sans-serif;padding:6px 10px;border-radius:8px;border:1px solid var(--brand-alt);background:rgba(255,212,50,.16);color:var(--fg1);white-space:nowrap;">'
    + lucide('<path d="M8 3H7a2 2 0 0 0-2 2v5a2 2 0 0 1-2 2 2 2 0 0 1 2 2v5c0 1.1.9 2 2 2h1"/><path d="M16 3h1a2 2 0 0 1 2 2v5a2 2 0 0 0 2 2 2 2 0 0 0-2 2v5a2 2 0 0 1-2 2h-1"/>', 12, 2)
    + 'YAML<span style="color:var(--brand-alt);display:inline-flex;margin-left:1px;">' + ICONS.chevronRight + '</span></button>';
}

function matrixHtml(summary) {
  if (!summary || !summary.length) return placeholder('No outputs produced — the recipe may be skipped for this platform, or the variant config yields nothing.');
  const keys = [];
  summary.forEach(s => (s.variant || []).forEach(p => { if (!keys.includes(p[0])) keys.push(p[0]); }));
  if (summary.length < 2 || keys.length > 4) return variantCardsHtml(summary, keys);
  const thS = 'text-align:left;padding:10px 14px;font:600 10px/1.2 Inter,sans-serif;letter-spacing:.06em;text-transform:uppercase;color:var(--fg3);border-bottom:1px solid var(--b2);position:sticky;top:0;background:var(--surface);white-space:nowrap;z-index:1;';
  const tdBase = 'padding:10px 14px;border-bottom:1px solid var(--b1);vertical-align:middle;';
  let head = '<tr><th style="' + thS + 'width:34px;">#</th><th style="' + thS + '">Package</th><th style="' + thS + '">Version</th>';
  keys.forEach(k => { head += '<th style="' + thS + '">' + escapeHtml(k) + '</th>'; });
  head += '<th style="' + thS + '">Build string</th><th style="' + thS + '">Run requirements</th><th style="' + thS + 'width:72px;"></th></tr>';
  const rows = summary.map((s, i) => {
    const dim = s.skipped ? 'opacity:.42;' : '';
    let keyCells = '';
    keys.forEach(k => {
      const e = (s.variant || []).find(p => p[0] === k);
      const v = e ? e[1] : '';
      keyCells += '<td style="' + tdBase + '">' + (v ? '<span style="display:inline-block;font:600 11.5px/1 \'JetBrains Mono\',monospace;background:rgba(255,212,50,.18);color:var(--fg1);border:1px solid var(--brand-alt);padding:3px 8px;border-radius:6px;">' + escapeHtml(v) + '</span>' : '<span style="color:var(--fg3);">·</span>') + '</td>';
    });
    const badges = [];
    if (s.noarch) badges.push(badge(s.noarch, 'green'));
    (s.flags || []).forEach(f => badges.push(badge(f, 'flag')));
    if (s.skipped) badges.push(badge('skipped', 'skip'));
    const buildStr = s.build_string ? '<span style="font:500 11.5px/1.35 \'JetBrains Mono\',monospace;color:var(--fg2);word-break:break-all;">' + escapeHtml(s.build_string) + '</span>' : '<span style="color:var(--fg3);">—</span>';
    const run = (s.run_deps && s.run_deps.length) ? '<div style="display:flex;flex-wrap:wrap;gap:4px;">' + s.run_deps.map(d => '<span style="display:inline-block;font:500 11px/1 \'JetBrains Mono\',monospace;background:var(--inset);color:var(--fg2);border:1px solid var(--b1);padding:3px 7px;border-radius:6px;">' + escapeHtml(d) + '</span>').join('') + '</div>' : '<span style="color:var(--fg3);">—</span>';
    return '<tr data-variant-idx="' + i + '" style="cursor:pointer;' + dim + '">' +
      '<td style="' + tdBase + "color:var(--fg3);font:600 11px/1.4 'JetBrains Mono',monospace;\">" + (i + 1) + '</td>' +
      '<td style="' + tdBase + '"><div style="display:flex;align-items:center;gap:6px;flex-wrap:wrap;"><span style="font:600 13px/1.2 Inter,sans-serif;color:var(--fg1);">' + escapeHtml(s.name) + '</span>' + badges.join('') + '</div></td>' +
      '<td style="' + tdBase + "\"><span style=\"font:500 12px/1.4 'JetBrains Mono',monospace;color:var(--fg2);\">" + escapeHtml(s.version) + '</span></td>' +
      keyCells +
      '<td style="' + tdBase + 'max-width:230px;">' + buildStr + '</td>' +
      '<td style="' + tdBase + 'max-width:320px;">' + run + '</td>' +
      '<td style="' + tdBase + 'text-align:right;">' + yamlBtn() + '</td></tr>';
  }).join('');
  const count = summary.length;
  const cap = '<div style="display:flex;align-items:center;gap:8px;flex-wrap:wrap;padding:11px 14px;border-bottom:1px solid var(--b1);background:var(--surface);flex-shrink:0;"><span style="font:600 12px/1 Inter,sans-serif;color:var(--fg1);">' + count + ' build variant' + (count !== 1 ? 's' : '') + '</span><span style="font:500 11px/1 \'JetBrains Mono\',monospace;color:var(--fg3);">' + escapeHtml(state.platform) + '</span><span style="margin-left:auto;font:500 11px/1 Inter,sans-serif;color:var(--fg3);">Select a row to inspect its evaluated recipe →</span></div>';
  return '<div style="height:100%;display:flex;flex-direction:column;">' + cap + '<div style="flex:1;min-height:0;overflow:auto;"><table style="border-collapse:collapse;width:100%;min-width:580px;font-family:Inter,sans-serif;"><thead>' + head + '</thead><tbody>' + rows + '</tbody></table></div></div>';
}

function variantCardsHtml(summary, keys) {
  const count = summary.length;
  const cap = '<div style="display:flex;align-items:center;gap:8px;flex-wrap:wrap;padding:11px 14px;border-bottom:1px solid var(--b1);position:sticky;top:0;background:var(--surface);z-index:2;"><span style="font:600 12px/1 Inter,sans-serif;color:var(--fg1);">' + count + ' build variant' + (count !== 1 ? 's' : '') + '</span><span style="font:500 11px/1 \'JetBrains Mono\',monospace;color:var(--fg3);">' + escapeHtml(state.platform) + '</span><span style="font:500 11px/1 Inter,sans-serif;color:var(--fg3);">· ' + keys.length + ' variant keys</span><span style="margin-left:auto;font:500 11px/1 Inter,sans-serif;color:var(--fg3);">Select a card to inspect →</span></div>';
  const cards = summary.map((s, i) => {
    const dim = s.skipped ? 'opacity:.5;' : '';
    const badges = [];
    if (s.noarch) badges.push(badge(s.noarch, 'green'));
    (s.flags || []).forEach(f => badges.push(badge(f, 'flag')));
    if (s.skipped) badges.push(badge('skipped', 'skip'));
    let kv = '';
    (s.variant || []).forEach(p => {
      kv += '<div style="font:500 11px/1.5 \'JetBrains Mono\',monospace;color:var(--fg3);white-space:nowrap;">' + escapeHtml(p[0]) + '</div>' +
            '<div><span style="display:inline-block;font:600 11px/1.4 \'JetBrains Mono\',monospace;background:rgba(255,212,50,.16);color:var(--fg1);border:1px solid var(--brand-alt);padding:2px 7px;border-radius:6px;word-break:break-all;">' + escapeHtml(p[1]) + '</span></div>';
    });
    const kvBlock = kv ? '<div style="display:grid;grid-template-columns:max-content 1fr;gap:5px 12px;margin:10px 0;">' + kv + '</div>' : '<div style="font:400 11px/1.4 Inter,sans-serif;color:var(--fg3);margin:9px 0;">No variant keys for this output</div>';
    const run = (s.run_deps && s.run_deps.length) ? s.run_deps.map(d => '<span style="display:inline-block;font:500 11px/1 \'JetBrains Mono\',monospace;background:var(--surface);color:var(--fg2);border:1px solid var(--b2);padding:3px 7px;border-radius:6px;margin:0 4px 4px 0;">' + escapeHtml(d) + '</span>').join('') : '<span style="color:var(--fg3);">—</span>';
    const build = s.build_string ? '<div style="font:500 11px/1.4 \'JetBrains Mono\',monospace;color:var(--fg2);word-break:break-all;margin-top:4px;">' + escapeHtml(s.build_string) + '</div>' : '';
    return '<div data-variant-idx="' + i + '" style="border:1px solid var(--b2);border-radius:12px;padding:13px 15px;background:var(--inset);cursor:pointer;' + dim + '">' +
      '<div style="display:flex;align-items:center;gap:7px;flex-wrap:wrap;"><span style="font:600 11px/1.4 \'JetBrains Mono\',monospace;color:var(--fg3);">#' + (i + 1) + '</span><span style="font:600 13.5px/1.2 Inter,sans-serif;color:var(--fg1);">' + escapeHtml(s.name) + '</span><span style="font:500 12px/1.4 \'JetBrains Mono\',monospace;color:var(--fg2);">' + escapeHtml(s.version) + '</span>' + badges.join('') + '</div>' +
      build + kvBlock +
      '<div style="border-top:1px solid var(--b1);margin-top:6px;padding-top:9px;"><div style="font:600 9.5px/1 Inter,sans-serif;text-transform:uppercase;letter-spacing:.05em;color:var(--fg3);margin-bottom:6px;">Run requirements</div><div>' + run + '</div></div>' +
      '<div style="margin-top:11px;display:flex;justify-content:flex-end;">' + yamlBtn() + '</div>' +
      '</div>';
  }).join('');
  return '<div style="height:100%;overflow:auto;">' + cap + '<div style="padding:14px;display:grid;grid-template-columns:repeat(auto-fill,minmax(300px,1fr));gap:12px;align-content:start;">' + cards + '</div></div>';
}

function variantDetailHtml(sv) {
  const vars = {};
  let plat = state.platform;
  (sv.variant || []).forEach(p => { if (p[0] === 'target_platform') plat = p[1]; else vars[p[0]] = p[1]; });
  let html = '', err = null;
  try {
    const r = JSON.parse(wasm.evaluate_recipe(recipeText, JSON.stringify(vars), plat));
    if (r.ok) html = r.result_html; else err = r.error;
  } catch (e) { err = { message: String(e) }; }
  const badges = [];
  if (sv.noarch) badges.push(badge(sv.noarch, 'green'));
  (sv.flags || []).forEach(f => badges.push(badge(f, 'flag')));
  if (sv.skipped) badges.push(badge('skipped', 'skip'));
  const back = '<button data-rb-back style="appearance:none;display:inline-flex;align-items:center;gap:6px;font:600 11.5px/1 Inter,sans-serif;padding:7px 11px;border-radius:8px;border:1px solid var(--b2);background:var(--inset);color:var(--fg1);white-space:nowrap;transition:border-color .12s;">' + ICONS.arrowLeft + 'All variants</button>';
  const header =
    '<div style="flex-shrink:0;border-bottom:1px solid var(--b1);background:var(--surface);padding:11px 14px;display:flex;align-items:center;gap:10px;flex-wrap:wrap;">' + back +
      '<span style="font:600 13.5px/1.2 Inter,sans-serif;color:var(--fg1);">' + escapeHtml(sv.name) + '</span>' +
      '<span style="font:500 12px/1.3 \'JetBrains Mono\',monospace;color:var(--fg2);">' + escapeHtml(sv.version) + '</span>' +
      (sv.build_string ? '<span style="font:500 11px/1.3 \'JetBrains Mono\',monospace;color:var(--fg3);">' + escapeHtml(sv.build_string) + '</span>' : '') +
      badges.join('') +
      '<span style="margin-left:auto;font:600 9.5px/1 Inter,sans-serif;text-transform:uppercase;letter-spacing:.05em;color:var(--fg3);">Evaluated recipe</span>' +
    '</div>';
  const body = err ? errorHtml(err) : yamlSurface(html);
  return '<div style="height:100%;display:flex;flex-direction:column;">' + header + '<div style="flex:1;min-height:0;">' + body + '</div></div>';
}

function paintOutput() {
  if (!outputMount) return;
  const d = outputData;
  if (!d) { outputMount.innerHTML = placeholder('Edit the recipe to see rendered output.'); return; }
  if (!d.ok) { outputMount.innerHTML = errorHtml(d.error); return; }
  if (d.kind === 'variants') {
    const sel = state.selectedVariant;
    if (sel != null && d.summary && d.summary[sel]) {
      outputMount.innerHTML = variantDetailHtml(d.summary[sel]);
    } else {
      outputMount.innerHTML = (state.variantsView === 'matrix') ? matrixHtml(d.summary) : yamlSurface(d.variants_html);
    }
  } else {
    outputMount.innerHTML = yamlSurface(d.result_html);
  }
  syncViewSegVisibility();
}

// ===== UI sync helpers =====

function setStatus(text, isError) {
  state.status = text;
  state.hasError = !!isError;
  statusBadge.textContent = text || '…';
  statusBadge.classList.toggle('is-error', !!isError);
}

function renderUsedVars() {
  const vars = state.usedVars || [];
  if (!vars.length) { usedVarsEl.innerHTML = ''; return; }
  let html = '<span class="used-vars-label">Variables used</span>';
  html += vars.map(v => '<span class="used-var">' + escapeHtml(v) + '</span>').join('');
  usedVarsEl.innerHTML = html;
}

function setSegActive(containerId, attr, value) {
  const container = $(containerId);
  if (!container) return;
  container.querySelectorAll('button').forEach(b => {
    b.classList.toggle('is-active', b.dataset[attr] === value);
  });
}

// The Matrix/YAML view toggle only applies to the Variants mode and when no
// single variant is being inspected.
function syncViewSegVisibility() {
  const show = state.mode === 'variants' && state.selectedVariant == null;
  $('view-seg').style.display = show ? '' : 'none';
}

// ===== Handlers =====

function selectVariant(i) { state.selectedVariant = i; paintOutput(); }
function clearVariant() { state.selectedVariant = null; paintOutput(); }

function setMode(m) {
  state.mode = m;
  lsSet('rbp-mode', m);
  setSegActive('mode-seg', 'mode', m);
  runEval();
}

function setVariantsView(v) {
  state.variantsView = v;
  lsSet('rbp-variants-view', v);
  setSegActive('view-seg', 'view', v);
  state.selectedVariant = null;
  paintOutput();
}

function setVariantFormat(f) {
  state.variantFormat = f;
  lsSet('rbp-format', f);
  setSegActive('format-seg', 'format', f);
  variantsFileTab.textContent = f === 'conda_build_config' ? 'conda_build_config' : 'variants.yaml';
  runEval();
}

function setTheme(t) {
  if (t === state.theme) return;
  state.theme = t;
  lsSet('rbp-theme', t);
  root.dataset.theme = t;
  rebuildEditors();
}

function setLayout(d) {
  state.layout = d;
  lsSet('rbp-dir', d);
  root.dataset.layout = d;
  setSegActive('layout-seg', 'layout', d);
  measureEditorsSoon();
}

function measureEditorsSoon() {
  setTimeout(() => {
    if (recipeView) recipeView.requestMeasure();
    if (varsView) varsView.requestMeasure();
  }, 40);
}

function loadExample(key) {
  const ex = EXAMPLES.find(e => e.key === key);
  if (ex) { setRecipeText(ex.recipe); closeGen(); }
}

function openGen() { genPanel.hidden = false; }
function closeGen() { genPanel.hidden = true; }
function toggleGen() { genPanel.hidden ? openGen() : closeGen(); }

function updateGenPlaceholders() {
  genPkgEl.placeholder = GEN_PLACEHOLDERS[state.genSource] || GEN_PLACEHOLDERS.pypi;
  genVerEl.hidden = state.genSource === 'cran';
}

async function runGenerator() {
  if (!wasm) return;
  const src = state.genSource;
  const pkg = genPkgEl.value.trim();
  if (!pkg) { genPkgEl.focus(); return; }
  const ver = genVerEl.value.trim() || null;
  genRunEl.disabled = true;
  genRunEl.textContent = 'generating…';
  setStatus('fetching ' + src + '…', false);
  try {
    let resJson;
    if (src === 'pypi') resJson = await wasm.generate_pypi_recipe(pkg, ver);
    else if (src === 'cran') resJson = await wasm.generate_cran_recipe(pkg, null);
    else resJson = await wasm.generate_cpan_recipe(pkg, ver);
    const r = JSON.parse(resJson);
    if (r.ok) {
      closeGen();
      genPkgEl.value = '';
      genVerEl.value = '';
      setRecipeText(r.result);
    } else {
      outputData = { ok: false, kind: state.mode === 'stage0' ? 'yaml' : 'variants', error: r.error };
      setStatus('error', true);
      paintOutput();
    }
  } catch (e) {
    outputData = { ok: false, kind: 'yaml', error: { message: String(e) } };
    setStatus('error', true);
    paintOutput();
  } finally {
    genRunEl.disabled = false;
    genRunEl.innerHTML = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 15V3"/><path d="m7 10 5 5 5-5"/><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/></svg>Generate recipe';
  }
}

async function loadPinning() {
  pinBtn.disabled = true;
  pinLabel.textContent = 'loading…';
  try {
    if (!pinningCache) {
      const resp = await fetch(PINNING_URL);
      if (!resp.ok) throw new Error('HTTP ' + resp.status);
      pinningCache = await resp.text();
    }
    variantsText = pinningCache;
    lsSet('rbp-variants', pinningCache);
    if (varsView) varsView.dispatch({ changes: { from: 0, to: varsView.state.doc.length, insert: pinningCache } });
    setVariantFormat('conda_build_config');
  } catch (e) {
    setStatus('pinning failed', true);
  } finally {
    pinBtn.disabled = false;
    pinLabel.textContent = '+ conda-forge pinning';
  }
}

function copyOutput() {
  const pre = outputMount.querySelector('pre.output-yaml');
  let text = '';
  if (pre) text = pre.innerText;
  else { const tbl = outputMount.querySelector('table'); text = tbl ? tbl.innerText : outputMount.innerText; }
  try {
    navigator.clipboard.writeText(text);
    setStatus('copied', false);
    setTimeout(() => { if (state.status === 'copied') runEval(); }, 1200);
  } catch (e) { /* ignore */ }
}

// ===== Wiring =====

function wireEvents() {
  platformSelect.addEventListener('change', () => {
    state.platform = platformSelect.value;
    lsSet('rbp-platform', state.platform);
    runEval();
  });

  $('theme-toggle').addEventListener('click', () => setTheme(state.theme === 'dark' ? 'light' : 'dark'));

  $('layout-seg').addEventListener('click', (e) => {
    const btn = e.target.closest('button[data-layout]');
    if (btn) setLayout(btn.dataset.layout);
  });
  $('mode-seg').addEventListener('click', (e) => {
    const btn = e.target.closest('button[data-mode]');
    if (btn) setMode(btn.dataset.mode);
  });
  $('view-seg').addEventListener('click', (e) => {
    const btn = e.target.closest('button[data-view]');
    if (btn) setVariantsView(btn.dataset.view);
  });
  $('format-seg').addEventListener('click', (e) => {
    const btn = e.target.closest('button[data-format]');
    if (btn) setVariantFormat(btn.dataset.format);
  });

  $('gen-toggle').addEventListener('click', toggleGen);
  genSourceEl.addEventListener('change', () => {
    state.genSource = genSourceEl.value;
    lsSet('rbp-gensrc', state.genSource);
    updateGenPlaceholders();
  });
  genRunEl.addEventListener('click', runGenerator);
  [genPkgEl, genVerEl].forEach(input => input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') { e.preventDefault(); runGenerator(); }
  }));
  $('gen-examples').addEventListener('click', (e) => {
    const btn = e.target.closest('button[data-example]');
    if (btn) loadExample(btn.dataset.example);
  });

  pinBtn.addEventListener('click', loadPinning);
  copyBtn.addEventListener('click', copyOutput);
  $('retry-btn').addEventListener('click', boot);

  // Close generator popover on outside click.
  document.addEventListener('mousedown', (e) => {
    if (!genPanel.hidden && !genWrap.contains(e.target)) closeGen();
  });

  // Output interactions: select a variant row/card, or go back.
  outputMount.addEventListener('click', (e) => {
    if (e.target.closest('[data-rb-back]')) { clearVariant(); return; }
    const row = e.target.closest('[data-variant-idx]');
    if (row) { const i = parseInt(row.getAttribute('data-variant-idx'), 10); if (!isNaN(i)) selectVariant(i); }
  });

  window.addEventListener('resize', measureEditorsSoon);
}

function renderExampleChips() {
  $('gen-examples').innerHTML = EXAMPLES
    .map(e => '<button type="button" class="gen-chip" data-example="' + e.key + '">' + escapeHtml(e.label) + '</button>')
    .join('');
}

function populatePlatforms() {
  let list = (state.platforms || []).slice();
  if (!list.includes(state.platform)) list = [state.platform].concat(list);
  platformSelect.innerHTML = list
    .map(p => '<option value="' + escapeHtml(p) + '"' + (p === state.platform ? ' selected' : '') + '>' + escapeHtml(p) + '</option>')
    .join('');
}

function orderPlatforms(list) {
  const set = list.filter(p => p !== 'unknown');
  const head = PREFERRED_PLATFORMS.filter(p => set.includes(p));
  const rest = set.filter(p => !head.includes(p)).sort();
  return head.concat(rest);
}

function injectThemeCss() {
  if (injectThemeCss._done || !wasm) return;
  try {
    const s = document.createElement('style');
    s.setAttribute('data-rb-theme', '');
    s.textContent = wasm.get_theme_css();
    document.head.appendChild(s);
    injectThemeCss._done = true;
  } catch (e) { /* ignore */ }
}

// ===== Boot =====

function applyInitialState() {
  root.dataset.theme = state.theme;
  root.dataset.layout = state.layout;
  setSegActive('layout-seg', 'layout', state.layout);
  setSegActive('mode-seg', 'mode', state.mode);
  setSegActive('view-seg', 'view', state.variantsView);
  setSegActive('format-seg', 'format', state.variantFormat);
  variantsFileTab.textContent = state.variantFormat === 'conda_build_config' ? 'conda_build_config' : 'variants.yaml';
  genSourceEl.value = state.genSource;
  updateGenPlaceholders();
}

async function boot() {
  loadingOverlay.hidden = false;
  errorOverlay.hidden = true;
  try {
    await wasmInit();
    wasm = wasmApi;
    let platforms = [];
    try { platforms = JSON.parse(wasm.get_platforms()); } catch (e) { /* ignore */ }
    state.platforms = orderPlatforms(platforms);

    injectThemeCss();
    populatePlatforms();
    if (!recipeView) mountEditors();
    runEval();
    loadingOverlay.hidden = true;
  } catch (e) {
    loadingOverlay.hidden = true;
    errorOverlay.hidden = false;
    $('error-message').textContent = String(e && e.message ? e.message : e);
  }
}

applyInitialState();
renderExampleChips();
wireEvents();
boot();
