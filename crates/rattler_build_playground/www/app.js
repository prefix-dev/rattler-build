import wasmInit, { parse_recipe, evaluate_recipe, get_used_variables, get_platforms, render_variants, get_theme_css, highlight_source_yaml, first_variant_values } from './rattler_build_playground.js';

const DEFAULT_RECIPE = `context:
  name: flask
  version: "3.0.0"

package:
  name: \${{ name }}
  version: \${{ version }}

source:
  url: https://pypi.io/packages/source/\${{ name[0] }}/\${{ name }}/flask-\${{ version }}.tar.gz
  sha256: cfadcdb638b609361d29ec22360d6070a77d7463dcb3ab08d2c2f2f168845f58

build:
  number: 0
  script:
    - python -m pip install . -vv
  python:
    entry_points:
      - flask = flask.cli:main

requirements:
  host:
    - python
    - flit-core <4
    - pip
  run:
    - python
    - werkzeug >=3.0.0
    - jinja2 >=3.1.2
    - click >=8.1.3
    - blinker >=1.6.2

tests:
  - python:
      imports:
        - flask
        - flask.json

about:
  homepage: https://palletsprojects.com/p/flask
  license: BSD-3-Clause
  summary: A simple framework for building complex web applications.
`;

const DEFAULT_VARIANTS = ``;

// State
let wasm = null;
let currentMode = 'variants';
let debounceTimer = null;

// DOM elements
const recipeEditor = document.getElementById('recipe-editor');
const variantsEditor = document.getElementById('variants-editor');
const recipeHighlight = document.getElementById('recipe-highlight');
const variantsHighlight = document.getElementById('variants-highlight');
const outputContainer = document.getElementById('output-container');
const outputBadge = document.getElementById('output-badge');
const platformSelect = document.getElementById('platform-select');
const usedVarsEl = document.getElementById('used-vars');

// Editor syntax highlighting
function highlightEditor(textarea, pre) {
  pre.innerHTML = highlight_source_yaml(textarea.value);
}

function syncScroll(textarea, pre) {
  pre.scrollTop = textarea.scrollTop;
  pre.scrollLeft = textarea.scrollLeft;
}

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

// Debounced update on input
recipeEditor.addEventListener('input', () => {
  localStorage.setItem('playground-recipe', recipeEditor.value);
  if (wasm) highlightEditor(recipeEditor, recipeHighlight);
  scheduleUpdate();
});

variantsEditor.addEventListener('input', () => {
  localStorage.setItem('playground-variants', variantsEditor.value);
  if (wasm) highlightEditor(variantsEditor, variantsHighlight);
  scheduleUpdate();
});

// Sync scroll between textarea and highlight overlay
recipeEditor.addEventListener('scroll', () => syncScroll(recipeEditor, recipeHighlight));
variantsEditor.addEventListener('scroll', () => syncScroll(variantsEditor, variantsHighlight));

platformSelect.addEventListener('change', () => {
  localStorage.setItem('playground-platform', platformSelect.value);
  update();
});

// Load conda-forge pinning
const CONDA_FORGE_PINNING_URL = 'https://raw.githubusercontent.com/conda-forge/conda-forge-pinning-feedstock/main/recipe/conda_build_config.yaml';
const loadPinningBtn = document.getElementById('load-pinning-btn');
let pinningCache = null;

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
    if (wasm) highlightEditor(variantsEditor, variantsHighlight);
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

function scheduleUpdate() {
  clearTimeout(debounceTimer);
  debounceTimer = setTimeout(update, 300);
}

function update() {
  if (!wasm) return;

  const yaml = recipeEditor.value;
  const variantYaml = variantsEditor.value || '{}';
  const platform = platformSelect.value;
  const start = performance.now();

  try {
    let resultJson;
    if (currentMode === 'stage0') {
      resultJson = parse_recipe(yaml);
    } else if (currentMode === 'stage1') {
      // For stage1, extract the first value of each variant key
      const varsJson = first_variant_values(variantYaml);
      resultJson = evaluate_recipe(yaml, varsJson, platform);
    } else if (currentMode === 'vars') {
      resultJson = get_used_variables(yaml);
    } else if (currentMode === 'variants') {
      resultJson = render_variants(yaml, variantYaml, platform);
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
      usedVarsEl.innerHTML = 'Used: ' + usedResult.result
        .map(v => `<span>${escapeHtml(v)}</span>`)
        .join(', ');
    } else {
      usedVarsEl.textContent = '';
    }
  } catch {
    usedVarsEl.textContent = '';
  }
}


function renderOutput(html) {
  outputContainer.innerHTML = `<pre class="output-yaml">${html}</pre>`;
}

function renderVariantsOutput(data) {
  const summary = data.summary;

  if (!summary || summary.length === 0) {
    outputContainer.innerHTML = '<div class="output-placeholder">No outputs produced (recipe may be skipped for this platform)</div>';
    return;
  }

  let html = '<div class="variants-view">';

  // Summary cards
  html += '<div class="variants-grid">';
  for (const s of summary) {
    const skippedClass = s.skipped ? ' variant-card-skipped' : '';
    html += `<div class="variant-card${skippedClass}">`;
    html += `<div class="variant-card-header">`;
    html += `<span class="variant-pkg-name">${escapeHtml(s.name)}</span>`;
    html += `<span class="variant-pkg-version">${escapeHtml(s.version)}</span>`;
    if (s.build_string) {
      html += `<span class="variant-build-string">${escapeHtml(s.build_string)}</span>`;
    }
    if (s.skipped) {
      html += `<span class="variant-badge variant-badge-skip">skipped</span>`;
    }
    if (s.noarch) {
      html += `<span class="variant-badge variant-badge-noarch">${escapeHtml(s.noarch)}</span>`;
    }
    html += `</div>`;

    // Context table
    const contextEntries = s.context ? Object.entries(s.context) : [];
    if (contextEntries.length > 0) {
      html += `<table class="context-table">`;
      html += `<thead><tr><th colspan="2">context</th></tr></thead><tbody>`;
      for (const [k, v] of contextEntries) {
        const display = typeof v === 'string' ? v : JSON.stringify(v);
        html += `<tr><td class="context-key">${escapeHtml(k)}</td><td class="context-value">${escapeHtml(display)}</td></tr>`;
      }
      html += `</tbody></table>`;
    }

    // Variant keys
    if (s.variant.length > 0) {
      html += `<div class="variant-keys">`;
      for (const [k, v] of s.variant) {
        html += `<span class="variant-key-pill"><span class="variant-key-name">${escapeHtml(k)}</span><span class="variant-key-value">${escapeHtml(v)}</span></span>`;
      }
      html += `</div>`;
    }

    // Dependencies summary
    const hasDeps = s.build_deps.length > 0 || s.host_deps.length > 0 || s.run_deps.length > 0;
    if (hasDeps) {
      html += `<div class="variant-deps">`;
      if (s.build_deps.length > 0) {
        html += `<div class="variant-dep-section"><span class="variant-dep-label">build:</span> ${s.build_deps.map(d => `<span class="variant-dep">${escapeHtml(d)}</span>`).join(' ')}</div>`;
      }
      if (s.host_deps.length > 0) {
        html += `<div class="variant-dep-section"><span class="variant-dep-label">host:</span> ${s.host_deps.map(d => `<span class="variant-dep">${escapeHtml(d)}</span>`).join(' ')}</div>`;
      }
      if (s.run_deps.length > 0) {
        html += `<div class="variant-dep-section"><span class="variant-dep-label">run:</span> ${s.run_deps.map(d => `<span class="variant-dep">${escapeHtml(d)}</span>`).join(' ')}</div>`;
      }
      html += `</div>`;
    }

    html += `</div>`;
  }
  html += '</div>';

  // Collapsible full YAML
  html += `<details class="variants-yaml-details">`;
  html += `<summary>Full YAML output (${summary.length} variant${summary.length !== 1 ? 's' : ''})</summary>`;
  html += `<pre class="output-yaml">${data.variants_html}</pre>`;
  html += `</details>`;

  html += '</div>';
  outputContainer.innerHTML = html;
}

function renderError(error) {
  let html = `<div class="output-error">${escapeHtml(error.message)}`;
  if (error.line != null) {
    html += `<div class="error-location">at line ${error.line}, column ${error.column || 0}</div>`;
  }
  html += '</div>';
  outputContainer.innerHTML = html;
}

function escapeHtml(str) {
  return str
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

// Draggable horizontal divider
const dividerH = document.getElementById('divider-h');
const panelLeft = document.getElementById('panel-left');
const panelRight = document.getElementById('panel-right');

dividerH.addEventListener('mousedown', (e) => {
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

// Draggable vertical divider
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

// Initialize WASM
async function main() {
  try {
    wasm = await wasmInit();

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

    // Initial editor highlighting
    highlightEditor(recipeEditor, recipeHighlight);
    highlightEditor(variantsEditor, variantsHighlight);

    // Initial render
    update();
  } catch (e) {
    outputContainer.innerHTML = `<div class="output-error">Failed to load WASM module: ${escapeHtml(e.toString())}</div>`;
  }
}

main();
