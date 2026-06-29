import wasmInit, { parse_recipe, evaluate_recipe, get_used_variables, get_platforms, render_variants, get_theme_css, first_variant_values, generate_pypi_recipe, generate_cran_recipe, generate_cpan_recipe } from './rattler_build_playground.js';

import { EditorView, keymap, lineNumbers, highlightActiveLine, highlightActiveLineGutter, drawSelection, placeholder } from '@codemirror/view';
import { EditorState, Compartment } from '@codemirror/state';
import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
import { searchKeymap, highlightSelectionMatches } from '@codemirror/search';
import { yaml } from '@codemirror/lang-yaml';
import { syntaxHighlighting, HighlightStyle, indentOnInput, bracketMatching, indentUnit } from '@codemirror/language';
import { tags as t } from '@lezer/highlight';

// ===== HTML utilities =====

function escapeHtml(str) {
  return str
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

// Marker for pre-escaped / trusted HTML that should not be double-escaped.
class SafeHTML {
  constructor(value) { this.value = value; }
}

// Wrap a string so `html` passes it through without escaping.
function safe(value) { return new SafeHTML(value); }

// Tagged template literal that auto-escapes interpolated values.
// Use safe() to inject trusted HTML without escaping.
function html(strings, ...values) {
  return strings.reduce((result, str, i) => {
    if (i >= values.length) return result + str;
    const val = values[i];
    const escaped = val instanceof SafeHTML ? val.value : escapeHtml(String(val));
    return result + str + escaped;
  }, '');
}

// ===== Constants =====

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

const CONDA_FORGE_PINNING_URL = 'https://raw.githubusercontent.com/conda-forge/conda-forge-pinning-feedstock/main/recipe/conda_build_config.yaml';

// ===== State =====

let wasmReady = false;
let currentMode = 'variants';
let variantFormat = 'variants';
let debounceTimer = null;
let pinningCache = null;

// ===== DOM elements =====

const recipeHost = document.getElementById('recipe-editor');
const variantsHost = document.getElementById('variants-editor');
const outputContainer = document.getElementById('output-container');
const outputBadge = document.getElementById('output-badge');
const platformSelect = document.getElementById('platform-select');
const usedVarsEl = document.getElementById('used-vars');
const loadPinningBtn = document.getElementById('load-pinning-btn');
const generatorSource = document.getElementById('generator-source');
const generatorPackage = document.getElementById('generator-package');
const generatorVersion = document.getElementById('generator-version');
const generatorBtn = document.getElementById('generator-btn');

// ===== CodeMirror editors =====

// Catppuccin Mocha highlight style, mapped onto the tags produced by
// @codemirror/lang-yaml (see its styleTags definition).
const yamlHighlight = HighlightStyle.define([
  { tag: [t.definition(t.propertyName), t.propertyName], color: '#89b4fa' }, // keys
  { tag: [t.string, t.special(t.string), t.attributeValue], color: '#a6e3a1' }, // quoted & block scalars
  { tag: t.content, color: '#cdd6f4' }, // plain scalar values
  { tag: t.keyword, color: '#cba6f7' }, // directives
  { tag: t.lineComment, color: '#6c7086', fontStyle: 'italic' },
  { tag: [t.separator, t.punctuation, t.squareBracket, t.brace, t.meta], color: '#9399b2' },
  { tag: t.labelName, color: '#f9e2af' }, // anchors / aliases
  { tag: t.typeName, color: '#fab387' }, // tags
]);

const editorTheme = EditorView.theme({
  '&': { height: '100%', backgroundColor: 'var(--bg-editor)', color: 'var(--text-primary)' },
  '&.cm-focused': { outline: 'none' },
  '.cm-scroller': { fontFamily: 'var(--font-mono)', lineHeight: '1.5', overflow: 'auto' },
  '.cm-content': { caretColor: 'var(--text-primary)' },
  '.cm-cursor, .cm-dropCursor': { borderLeftColor: 'var(--text-primary)' },
  '.cm-gutters': {
    backgroundColor: 'var(--bg-secondary)',
    color: 'var(--text-muted)',
    border: 'none',
    borderRight: '1px solid var(--border)',
  },
  '.cm-activeLine': { backgroundColor: 'rgba(205, 214, 244, 0.04)' },
  '.cm-activeLineGutter': { backgroundColor: 'var(--bg-panel)', color: 'var(--text-secondary)' },
  '.cm-selectionBackground, &.cm-focused .cm-selectionBackground, .cm-content ::selection': {
    backgroundColor: 'rgba(116, 199, 236, 0.25)',
  },
  '.cm-selectionMatch': { backgroundColor: 'rgba(249, 226, 175, 0.18)' },
  '.cm-matchingBracket, &.cm-focused .cm-matchingBracket': {
    backgroundColor: 'rgba(116, 199, 236, 0.2)',
    outline: '1px solid var(--accent)',
  },
  '.cm-placeholder': { color: 'var(--text-muted)' },
  // Search panel (Ctrl/Cmd-F) styled to match the dark theme.
  '.cm-panels': { backgroundColor: 'var(--bg-secondary)', color: 'var(--text-primary)' },
  '.cm-panels.cm-panels-bottom': { borderTop: '1px solid var(--border)' },
  '.cm-textfield': {
    backgroundColor: 'var(--bg-editor)',
    color: 'var(--text-primary)',
    border: '1px solid var(--border)',
  },
  '.cm-button': {
    backgroundColor: 'var(--bg-panel)',
    color: 'var(--text-primary)',
    border: '1px solid var(--border)',
    backgroundImage: 'none',
  },
}, { dark: true });

// 16px keeps iOS Safari from auto-zooming when an editor is focused; 14px is
// nicer on a desktop. Swapped reactively via a compartment on viewport change.
const fontCompartment = new Compartment();
const mobileMedia = window.matchMedia('(max-width: 768px)');
const fontTheme = () => EditorView.theme({ '.cm-scroller': { fontSize: mobileMedia.matches ? '16px' : '14px' } });

function makeEditor(host, doc, { placeholderText, onChange } = {}) {
  const extensions = [
    lineNumbers(),
    highlightActiveLineGutter(),
    highlightActiveLine(),
    history(),
    drawSelection(),
    indentOnInput(),
    indentUnit.of('  '),
    bracketMatching(),
    highlightSelectionMatches(),
    EditorView.lineWrapping,
    yaml(),
    syntaxHighlighting(yamlHighlight),
    editorTheme,
    fontCompartment.of(fontTheme()),
    keymap.of([indentWithTab, ...defaultKeymap, ...historyKeymap, ...searchKeymap]),
    EditorView.updateListener.of((u) => {
      if (u.docChanged && onChange) onChange(u.state.doc.toString());
    }),
  ];
  if (placeholderText) extensions.push(placeholder(placeholderText));
  return new EditorView({ parent: host, state: EditorState.create({ doc, extensions }) });
}

const recipeEditor = makeEditor(recipeHost, localStorage.getItem('playground-recipe') || DEFAULT_RECIPE, {
  onChange: (text) => {
    localStorage.setItem('playground-recipe', text);
    scheduleUpdate();
  },
});

const variantsEditor = makeEditor(variantsHost, localStorage.getItem('playground-variants') || DEFAULT_VARIANTS, {
  placeholderText: 'python:\n  - "3.11"\n  - "3.12"',
  onChange: (text) => {
    localStorage.setItem('playground-variants', text);
    scheduleUpdate();
  },
});

mobileMedia.addEventListener('change', () => {
  for (const view of [recipeEditor, variantsEditor]) {
    view.dispatch({ effects: fontCompartment.reconfigure(fontTheme()) });
  }
});

const getRecipe = () => recipeEditor.state.doc.toString();
const getVariants = () => variantsEditor.state.doc.toString();

// Replace an editor's whole document (used by the generator and pinning loader).
function setEditorText(view, text) {
  view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text } });
}

// ===== Rendering =====

function renderOutput(highlightedHtml) {
  outputContainer.innerHTML = html`<pre class="output-yaml">${safe(highlightedHtml)}</pre>`;
}

function renderDepSection(label, deps) {
  if (deps.length === 0) return '';
  const pills = deps.map(d => html`<span class="variant-dep">${d}</span>`).join(' ');
  return html`<div class="variant-dep-section"><span class="variant-dep-label">${label}:</span> ${safe(pills)}</div>`;
}

function renderCard(s) {
  const skippedClass = s.skipped ? ' variant-card-skipped' : '';
  const buildStr = s.build_string ? html`<span class="variant-build-string">${s.build_string}</span>` : '';
  const skippedBadge = s.skipped ? html`<span class="variant-badge variant-badge-skip">skipped</span>` : '';
  const noarchBadge = s.noarch ? html`<span class="variant-badge variant-badge-noarch">${s.noarch}</span>` : '';
  const flagBadges = (s.flags || []).map(f =>
    html`<span class="variant-badge variant-badge-flag">${f}</span>`
  ).join('');

  const contextEntries = s.context ? Object.entries(s.context) : [];
  const contextTable = contextEntries.length === 0 ? '' :
    html`<table class="context-table"><thead><tr><th colspan="2">context</th></tr></thead><tbody>${safe(
      contextEntries.map(([k, v]) => {
        const display = typeof v === 'string' ? v : JSON.stringify(v);
        return html`<tr><td class="context-key">${k}</td><td class="context-value">${display}</td></tr>`;
      }).join('')
    )}</tbody></table>`;

  const variantKeys = s.variant.length === 0 ? '' :
    html`<div class="variant-keys">${safe(
      s.variant.map(([k, v]) =>
        html`<span class="variant-key-pill"><span class="variant-key-name">${k}</span><span class="variant-key-value">${v}</span></span>`
      ).join('')
    )}</div>`;

  const hasDeps = s.build_deps.length > 0 || s.host_deps.length > 0 || s.run_deps.length > 0;
  const depsSection = !hasDeps ? '' :
    html`<div class="variant-deps">${safe(
      renderDepSection('build', s.build_deps) +
      renderDepSection('host', s.host_deps) +
      renderDepSection('run', s.run_deps)
    )}</div>`;

  return html`<div class="variant-card${safe(skippedClass)}">
    <div class="variant-card-header">
      <span class="variant-pkg-name">${s.name}</span>
      <span class="variant-pkg-version">${s.version}</span>
      ${safe(buildStr)}${safe(skippedBadge)}${safe(noarchBadge)}${safe(flagBadges)}
    </div>
    ${safe(contextTable)}${safe(variantKeys)}${safe(depsSection)}
  </div>`;
}

function renderVariantsOutput(data) {
  const summary = data.summary;

  if (!summary || summary.length === 0) {
    outputContainer.innerHTML = html`<div class="output-placeholder">No outputs produced (recipe may be skipped for this platform)</div>`;
    return;
  }

  const cards = summary.map(renderCard).join('');
  const count = summary.length;
  const plural = count !== 1 ? 's' : '';

  outputContainer.innerHTML = html`<div class="variants-view">
    <div class="variants-grid">${safe(cards)}</div>
    <details class="variants-yaml-details">
      <summary>Full YAML output (${count} variant${plural})</summary>
      <pre class="output-yaml">${safe(data.variants_html)}</pre>
    </details>
  </div>`;
}

function renderError(error) {
  const location = error.line != null
    ? html`<div class="error-location">at line ${error.line}, column ${error.column || 0}</div>`
    : '';
  outputContainer.innerHTML = html`<div class="output-error">${error.message}${safe(location)}</div>`;
}

// ===== Core update loop =====

function scheduleUpdate() {
  clearTimeout(debounceTimer);
  debounceTimer = setTimeout(update, 300);
}

function update() {
  if (!wasmReady) return;

  const yaml = getRecipe();
  const variantYaml = getVariants() || '{}';
  const platform = platformSelect.value;
  const start = performance.now();

  try {
    let resultJson;
    if (currentMode === 'stage0') {
      resultJson = parse_recipe(yaml);
    } else if (currentMode === 'stage1') {
      const varsJson = first_variant_values(variantYaml);
      resultJson = evaluate_recipe(yaml, varsJson, platform);
    } else if (currentMode === 'variants') {
      resultJson = render_variants(yaml, variantYaml, platform, variantFormat);
    } else {
      throw new Error(`Unknown output mode: ${currentMode}`);
    }

    const elapsed = (performance.now() - start).toFixed(1);
    const result = JSON.parse(resultJson);

    if (result.ok) {
      outputBadge.textContent = `${elapsed}ms`;
      outputBadge.style.color = '';
      if (currentMode === 'variants') {
        renderVariantsOutput(result.result);
      } else {
        renderOutput(result.result_html);
      }
    } else {
      outputBadge.textContent = 'error';
      outputBadge.style.color = 'var(--error)';
      renderError(result.error);
    }
  } catch (e) {
    outputBadge.textContent = 'error';
    outputBadge.style.color = 'var(--error)';
    renderError({ message: e.toString() });
  }

  // Update used variables hint
  try {
    const usedJson = get_used_variables(yaml);
    const usedResult = JSON.parse(usedJson);
    if (usedResult.ok && usedResult.result.length > 0) {
      usedVarsEl.innerHTML = html`Used: ${safe(
        usedResult.result.map(v => html`<span>${v}</span>`).join(', ')
      )}`;
    } else {
      usedVarsEl.textContent = '';
    }
  } catch {
    usedVarsEl.textContent = '';
  }
}

// ===== Event listeners =====

// Tab switching
document.querySelectorAll('.tab-btn').forEach(btn => {
  btn.addEventListener('click', () => {
    document.querySelectorAll('.tab-btn').forEach(b => b.classList.remove('active'));
    btn.classList.add('active');
    currentMode = btn.dataset.mode;
    localStorage.setItem('playground-mode', currentMode);
    update();
  });
});

// Restore active mode
const savedMode = localStorage.getItem('playground-mode');
if (savedMode) {
  currentMode = savedMode;
  document.querySelectorAll('.tab-btn').forEach(btn => {
    btn.classList.toggle('active', btn.dataset.mode === currentMode);
  });
}

// Variant format toggle
document.querySelectorAll('.variant-format-btn').forEach(btn => {
  btn.addEventListener('click', () => {
    document.querySelectorAll('.variant-format-btn').forEach(b => b.classList.remove('active'));
    btn.classList.add('active');
    variantFormat = btn.dataset.format;
    localStorage.setItem('playground-variant-format', variantFormat);
    update();
  });
});

// Restore variant format
const savedFormat = localStorage.getItem('playground-variant-format');
if (savedFormat) {
  variantFormat = savedFormat;
  document.querySelectorAll('.variant-format-btn').forEach(btn => {
    btn.classList.toggle('active', btn.dataset.format === variantFormat);
  });
}

platformSelect.addEventListener('change', () => {
  localStorage.setItem('playground-platform', platformSelect.value);
  update();
});

// Recipe generator (PyPI / CRAN / CPAN)
const GENERATORS = {
  pypi: { fn: generate_pypi_recipe, versioned: true, placeholder: 'package name' },
  cran: { fn: generate_cran_recipe, versioned: false, placeholder: 'R package' },
  cpan: { fn: generate_cpan_recipe, versioned: true, placeholder: 'Perl distribution' },
};

function updateGeneratorPlaceholders() {
  const cfg = GENERATORS[generatorSource.value] || GENERATORS.pypi;
  generatorPackage.placeholder = cfg.placeholder;
  generatorVersion.style.display = cfg.versioned ? '' : 'none';
}

generatorSource.value = localStorage.getItem('playground-gen-source') || 'pypi';
updateGeneratorPlaceholders();

generatorSource.addEventListener('change', () => {
  localStorage.setItem('playground-gen-source', generatorSource.value);
  updateGeneratorPlaceholders();
});

async function runGenerator() {
  const source = generatorSource.value;
  const cfg = GENERATORS[source];
  if (!cfg) return;
  const pkg = generatorPackage.value.trim();
  if (!pkg) {
    generatorPackage.focus();
    return;
  }
  const version = generatorVersion.value.trim() || null;

  generatorBtn.disabled = true;
  const originalText = generatorBtn.textContent;
  generatorBtn.textContent = 'generating...';
  outputBadge.textContent = `fetching ${source}…`;
  outputBadge.style.color = '';
  try {
    const arg2 = cfg.versioned ? version : null;
    const resultJson = await cfg.fn(pkg, arg2);
    const result = JSON.parse(resultJson);
    if (result.ok) {
      // The editor's update listener persists the new text and schedules a render.
      setEditorText(recipeEditor, result.result);
    } else {
      outputBadge.textContent = 'error';
      outputBadge.style.color = 'var(--error)';
      renderError(result.error);
    }
  } catch (e) {
    outputBadge.textContent = 'error';
    outputBadge.style.color = 'var(--error)';
    renderError({ message: e.toString() });
  } finally {
    generatorBtn.disabled = false;
    generatorBtn.textContent = originalText;
  }
}

generatorBtn.addEventListener('click', runGenerator);

[generatorPackage, generatorVersion].forEach(input => {
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      runGenerator();
    }
  });
});

// Load conda-forge pinning
loadPinningBtn.addEventListener('click', async () => {
  loadPinningBtn.disabled = true;
  loadPinningBtn.textContent = 'loading...';
  try {
    if (!pinningCache) {
      const resp = await fetch(CONDA_FORGE_PINNING_URL);
      if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
      pinningCache = await resp.text();
    }
    // The editor's update listener persists the new text and schedules a render.
    setEditorText(variantsEditor, pinningCache);
    // conda-forge pinning uses conda_build_config format
    variantFormat = 'conda_build_config';
    localStorage.setItem('playground-variant-format', variantFormat);
    document.querySelectorAll('.variant-format-btn').forEach(b => {
      b.classList.toggle('active', b.dataset.format === variantFormat);
    });
    scheduleUpdate();
  } catch (e) {
    alert(`Failed to load pinning: ${e.message}`);
  } finally {
    loadPinningBtn.disabled = false;
    loadPinningBtn.textContent = 'insert conda-forge pinning';
  }
});

// ===== Draggable dividers =====

const MOBILE_BREAKPOINT = 768;

const dividerH = document.getElementById('divider-h');
const panelLeft = document.getElementById('panel-left');
const panelRight = document.getElementById('panel-right');

dividerH.addEventListener('pointerdown', (e) => {
  // On mobile the panels stack vertically and scroll with the page, so the
  // horizontal (left/right) resize doesn't apply.
  if (window.innerWidth <= MOBILE_BREAKPOINT) return;
  e.preventDefault();
  dividerH.setPointerCapture(e.pointerId);
  dividerH.classList.add('active');
  const startX = e.clientX;
  const startLeftWidth = panelLeft.offsetWidth;
  const totalWidth = panelLeft.parentElement.offsetWidth;

  function onMove(e) {
    const dx = e.clientX - startX;
    const newLeft = ((startLeftWidth + dx) / totalWidth) * 100;
    const clamped = Math.max(20, Math.min(80, newLeft));
    panelLeft.style.flex = `0 0 ${clamped}%`;
    panelRight.style.flex = `1`;
  }

  function onUp() {
    dividerH.classList.remove('active');
    dividerH.removeEventListener('pointermove', onMove);
    dividerH.removeEventListener('pointerup', onUp);
  }

  dividerH.addEventListener('pointermove', onMove);
  dividerH.addEventListener('pointerup', onUp);
});

const dividerV = document.getElementById('divider-v');

dividerV.addEventListener('pointerdown', (e) => {
  e.preventDefault();
  dividerV.setPointerCapture(e.pointerId);
  dividerV.classList.add('active');
  const startY = e.clientY;
  const parent = dividerV.parentElement;
  const recipePanel = dividerV.previousElementSibling;
  const varsPanel = dividerV.nextElementSibling;
  const startRecipeHeight = recipePanel.offsetHeight;
  const totalHeight = parent.offsetHeight;

  function onMove(e) {
    const dy = e.clientY - startY;
    const newRecipe = ((startRecipeHeight + dy) / totalHeight) * 100;
    const clamped = Math.max(20, Math.min(80, newRecipe));
    recipePanel.style.flex = `0 0 ${clamped}%`;
    varsPanel.style.flex = '1';
  }

  function onUp() {
    dividerV.classList.remove('active');
    dividerV.removeEventListener('pointermove', onMove);
    dividerV.removeEventListener('pointerup', onUp);
  }

  dividerV.addEventListener('pointermove', onMove);
  dividerV.addEventListener('pointerup', onUp);
});

// ===== Initialize WASM =====

async function main() {
  try {
    await wasmInit();
    wasmReady = true;

    // Inject arborium syntax-highlighting theme CSS
    const style = document.createElement('style');
    style.textContent = get_theme_css();
    document.head.appendChild(style);

    // Populate platform dropdown
    const platforms = JSON.parse(get_platforms());
    const savedPlatform = localStorage.getItem('playground-platform') || 'linux-64';
    for (const p of platforms) {
      const opt = document.createElement('option');
      opt.value = p;
      opt.textContent = p;
      if (p === savedPlatform) opt.selected = true;
      platformSelect.appendChild(opt);
    }

    // Initial render
    update();
  } catch (e) {
    outputContainer.innerHTML = html`<div class="output-error">Failed to load WASM module: ${e.toString()}</div>`;
  }
}

main();
