import wasmInit, { parse_recipe, evaluate_recipe, get_used_variables, get_platforms, render_variants, get_theme_css, highlight_source_yaml, first_variant_values } from './rattler_build_playground.js';

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

const recipeEditor = document.getElementById('recipe-editor');
const variantsEditor = document.getElementById('variants-editor');
const recipeHighlight = document.getElementById('recipe-highlight');
const variantsHighlight = document.getElementById('variants-highlight');
const outputContainer = document.getElementById('output-container');
const outputBadge = document.getElementById('output-badge');
const platformSelect = document.getElementById('platform-select');
const usedVarsEl = document.getElementById('used-vars');
const loadPinningBtn = document.getElementById('load-pinning-btn');

// ===== Editor helpers =====

function highlightEditor(textarea, pre) {
  // highlight_source_yaml returns pre-escaped HTML from the WASM module.
  // Append '\n' because arborium trims trailing newlines, but the textarea
  // keeps them — without this the <pre> content is shorter.
  pre.innerHTML = highlight_source_yaml(textarea.value) + '\n';
}

// Fallback auto-resize for browsers without CSS field-sizing: content.
// Sizes the textarea to its content so it never scrolls internally;
// the parent .editor-container scrolls both textarea and highlight <pre>.
function autoResize(textarea) {
  if (CSS.supports('field-sizing', 'content')) return;
  textarea.style.height = 'auto';
  textarea.style.height = Math.max(textarea.scrollHeight, textarea.parentElement.clientHeight) + 'px';
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
      ${safe(buildStr)}${safe(skippedBadge)}${safe(noarchBadge)}
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

  const yaml = recipeEditor.value;
  const variantYaml = variantsEditor.value || '{}';
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

// Load saved state or defaults
recipeEditor.value = localStorage.getItem('playground-recipe') || DEFAULT_RECIPE;
variantsEditor.value = localStorage.getItem('playground-variants') || DEFAULT_VARIANTS;

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

// Debounced update on input
recipeEditor.addEventListener('input', () => {
  localStorage.setItem('playground-recipe', recipeEditor.value);
  if (wasmReady) highlightEditor(recipeEditor, recipeHighlight);
  autoResize(recipeEditor);
  scheduleUpdate();
});

variantsEditor.addEventListener('input', () => {
  localStorage.setItem('playground-variants', variantsEditor.value);
  if (wasmReady) highlightEditor(variantsEditor, variantsHighlight);
  autoResize(variantsEditor);
  scheduleUpdate();
});

platformSelect.addEventListener('change', () => {
  localStorage.setItem('playground-platform', platformSelect.value);
  update();
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
    variantsEditor.value = pinningCache;
    localStorage.setItem('playground-variants', pinningCache);
    // conda-forge pinning uses conda_build_config format
    variantFormat = 'conda_build_config';
    localStorage.setItem('playground-variant-format', variantFormat);
    document.querySelectorAll('.variant-format-btn').forEach(b => {
      b.classList.toggle('active', b.dataset.format === variantFormat);
    });
    if (wasmReady) highlightEditor(variantsEditor, variantsHighlight);
    autoResize(variantsEditor);
    scheduleUpdate();
  } catch (e) {
    alert(`Failed to load pinning: ${e.message}`);
  } finally {
    loadPinningBtn.disabled = false;
    loadPinningBtn.textContent = 'conda-forge';
  }
});

// Handle tab key in editors
[recipeEditor, variantsEditor].forEach(editor => {
  editor.addEventListener('keydown', (e) => {
    if (e.key === 'Tab') {
      e.preventDefault();
      const start = editor.selectionStart;
      const end = editor.selectionEnd;
      editor.value = editor.value.substring(0, start) + '  ' + editor.value.substring(end);
      editor.selectionStart = editor.selectionEnd = start + 2;
      editor.dispatchEvent(new Event('input'));
    }
  });
});

// ===== Draggable dividers =====

const MOBILE_BREAKPOINT = 768;

const dividerH = document.getElementById('divider-h');
const panelLeft = document.getElementById('panel-left');
const panelRight = document.getElementById('panel-right');

dividerH.addEventListener('mousedown', (e) => {
  if (window.innerWidth <= MOBILE_BREAKPOINT) return;
  e.preventDefault();
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
    document.removeEventListener('mousemove', onMove);
    document.removeEventListener('mouseup', onUp);
  }

  document.addEventListener('mousemove', onMove);
  document.addEventListener('mouseup', onUp);
});

const dividerV = document.getElementById('divider-v');

dividerV.addEventListener('mousedown', (e) => {
  e.preventDefault();
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
    document.removeEventListener('mousemove', onMove);
    document.removeEventListener('mouseup', onUp);
  }

  document.addEventListener('mousemove', onMove);
  document.addEventListener('mouseup', onUp);
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

    // Initial editor highlighting and sizing
    highlightEditor(recipeEditor, recipeHighlight);
    highlightEditor(variantsEditor, variantsHighlight);
    autoResize(recipeEditor);
    autoResize(variantsEditor);

    // Initial render
    update();
  } catch (e) {
    outputContainer.innerHTML = html`<div class="output-error">Failed to load WASM module: ${e.toString()}</div>`;
  }
}

main();
