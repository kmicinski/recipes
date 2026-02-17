//! Full-screen structured recipe editor with Monaco and ingredient input.

use crate::models::Recipe;
use crate::recipes::html_escape;
use crate::templates::STYLE;

/// Render the full-screen recipe editor.
/// If `recipe` is Some, we're editing; if None, we're creating.
pub fn render_recipe_editor(recipe: Option<&Recipe>) -> String {
    let is_edit = recipe.is_some();
    let page_title = if is_edit { "Edit Recipe" } else { "New Recipe" };
    let back_href = match recipe {
        Some(r) => format!("/recipe/{}", r.key),
        None => "/".to_string(),
    };

    // Serialize recipe data as JSON for the JS to consume
    let recipe_json = match recipe {
        Some(r) => {
            let ingredients_json: Vec<String> = r
                .ingredients
                .iter()
                .map(|i| {
                    format!(
                        r#"{{"name":"{}","qty":{},"unit":"{}"}}"#,
                        json_escape(&i.name),
                        i.qty,
                        json_escape(&i.unit)
                    )
                })
                .collect();
            let tags_str: Vec<String> = r.tags.iter().map(|t| format!(r#""{}""#, json_escape(t))).collect();
            format!(
                r#"{{"title":"{}","servings":{},"tags":[{}],"ingredients":[{}],"instructions":"{}","key":"{}"}}"#,
                json_escape(&r.title),
                r.servings.unwrap_or(4),
                tags_str.join(","),
                ingredients_json.join(","),
                json_escape(&r.body_markdown),
                json_escape(&r.key),
            )
        }
        None => "{\"title\":\"\",\"servings\":4,\"tags\":[],\"ingredients\":[],\"instructions\":\"## Instructions\\n\\n1. \",\"key\":null}".to_string(),
    };

    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{page_title} - Recipes</title>
    <style>{style}</style>
    <style>{editor_style}</style>
    <script src="https://cdnjs.cloudflare.com/ajax/libs/monaco-editor/0.45.0/min/vs/loader.min.js"></script>
</head>
<body class="editor-body">
    <div class="editor-toolbar">
        <a href="{back_href}" class="editor-back">&larr; Back</a>
        <input type="text" id="recipe-title" class="editor-title-input" placeholder="Recipe Title" autocomplete="off">
        <span id="save-status" class="save-status"></span>
        <button id="save-btn" class="btn" onclick="saveRecipe()">Save</button>
    </div>
    <div class="editor-meta-bar">
        <label>Servings: <input type="number" id="recipe-servings" min="1" value="4" class="meta-input"></label>
        <label>Tags: <input type="text" id="recipe-tags" placeholder="dinner, asian" class="meta-input meta-input-wide"></label>
    </div>
    <div class="editor-panels">
        <div class="editor-left">
            <div class="ingredients-header">
                <h3>Ingredients</h3>
                <span id="ing-count" class="ing-count"></span>
            </div>
            <div id="ingredient-rows" class="ingredient-rows"></div>
            <button type="button" class="btn secondary small" onclick="addIngredientRow()">+ Add ingredient</button>
        </div>
        <div class="editor-right">
            <div id="monaco-editor" class="monaco-container"></div>
        </div>
    </div>

    <datalist id="unit-options">
        <option value="g">
        <option value="kg">
        <option value="ml">
        <option value="l">
        <option value="tsp">
        <option value="tbsp">
        <option value="cup">
        <option value="cups">
        <option value="oz">
        <option value="lb">
        <option value="piece">
        <option value="pieces">
        <option value="clove">
        <option value="cloves">
        <option value="pinch">
        <option value="bunch">
        <option value="can">
        <option value="slice">
        <option value="slices">
    </datalist>

    <script>
    const RECIPE_DATA = {recipe_json};
    let editor = null;

    // --- Ingredient rows ---
    function createIngredientRow(ing) {{
        const row = document.createElement('div');
        row.className = 'ing-row';
        row.innerHTML = `
            <input type="number" class="ing-qty" placeholder="Qty" step="any" min="0" value="${{ing.qty || ''}}">
            <input type="text" class="ing-unit" list="unit-options" placeholder="Unit" value="${{escapeAttr(ing.unit || '')}}">
            <input type="text" class="ing-name" placeholder="Ingredient name" value="${{escapeAttr(ing.name || '')}}">
            <button type="button" class="ing-remove" onclick="removeIngredientRow(this)" title="Remove">&times;</button>
        `;
        // Enter on name field adds new row
        const nameInput = row.querySelector('.ing-name');
        nameInput.addEventListener('keydown', function(e) {{
            if (e.key === 'Enter') {{
                e.preventDefault();
                addIngredientRow();
            }}
        }});
        // Update count on input
        row.querySelectorAll('input').forEach(inp => inp.addEventListener('input', updateIngCount));
        return row;
    }}

    function addIngredientRow(ing) {{
        const container = document.getElementById('ingredient-rows');
        const row = createIngredientRow(ing || {{name:'', qty:'', unit:''}});
        container.appendChild(row);
        updateIngCount();
        // Focus the qty field of the new row
        if (!ing || !ing.name) {{
            row.querySelector('.ing-qty').focus();
        }}
    }}

    function removeIngredientRow(btn) {{
        btn.closest('.ing-row').remove();
        updateIngCount();
    }}

    function updateIngCount() {{
        const rows = document.querySelectorAll('.ing-row');
        let filled = 0;
        rows.forEach(r => {{
            const name = r.querySelector('.ing-name').value.trim();
            if (name) filled++;
        }});
        const el = document.getElementById('ing-count');
        if (filled > 0) {{
            el.textContent = filled + ' ingredient' + (filled !== 1 ? 's' : '');
            el.className = 'ing-count valid';
        }} else {{
            el.textContent = '';
            el.className = 'ing-count';
        }}
    }}

    function getIngredients() {{
        const rows = document.querySelectorAll('.ing-row');
        const result = [];
        rows.forEach(r => {{
            const name = r.querySelector('.ing-name').value.trim();
            if (!name) return;
            result.push({{
                name: name,
                qty: parseFloat(r.querySelector('.ing-qty').value) || 0,
                unit: r.querySelector('.ing-unit').value.trim()
            }});
        }});
        return result;
    }}

    function escapeAttr(s) {{
        return s.replace(/&/g,'&amp;').replace(/"/g,'&quot;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
    }}

    // --- Save ---
    async function saveRecipe() {{
        const title = document.getElementById('recipe-title').value.trim();
        if (!title) {{
            document.getElementById('recipe-title').focus();
            setStatus('Title is required', 'error');
            return;
        }}

        const data = {{
            title: title,
            servings: parseInt(document.getElementById('recipe-servings').value) || 4,
            tags: document.getElementById('recipe-tags').value.split(',').map(t => t.trim()).filter(t => t),
            ingredients: getIngredients(),
            instructions: editor ? editor.getValue() : ''
        }};

        const key = RECIPE_DATA.key;
        const url = key ? '/api/recipe/' + key : '/api/recipe';
        const saveBtn = document.getElementById('save-btn');
        saveBtn.disabled = true;
        setStatus('Saving...', '');

        try {{
            const resp = await fetch(url, {{
                method: 'POST',
                headers: {{ 'Content-Type': 'application/json' }},
                body: JSON.stringify(data)
            }});
            if (resp.ok) {{
                const result = await resp.json();
                if (result.key && !key) {{
                    // New recipe created -- redirect to it
                    window.location.href = '/recipe/' + result.key;
                }} else {{
                    setStatus('Saved', 'ok');
                }}
            }} else {{
                const text = await resp.text();
                setStatus('Error: ' + text, 'error');
            }}
        }} catch(e) {{
            setStatus('Network error', 'error');
        }} finally {{
            saveBtn.disabled = false;
        }}
    }}

    function setStatus(msg, type) {{
        const el = document.getElementById('save-status');
        el.textContent = msg;
        el.className = 'save-status' + (type ? ' save-status-' + type : '');
        if (type === 'ok') {{
            setTimeout(() => {{ el.textContent = ''; el.className = 'save-status'; }}, 2000);
        }}
    }}

    // --- Keyboard shortcuts ---
    document.addEventListener('keydown', function(e) {{
        if ((e.metaKey || e.ctrlKey) && e.key === 's') {{
            e.preventDefault();
            saveRecipe();
        }}
    }});

    // --- Init ---
    document.addEventListener('DOMContentLoaded', function() {{
        // Populate fields
        document.getElementById('recipe-title').value = RECIPE_DATA.title || '';
        document.getElementById('recipe-servings').value = RECIPE_DATA.servings || 4;
        document.getElementById('recipe-tags').value = (RECIPE_DATA.tags || []).join(', ');

        // Populate ingredient rows
        if (RECIPE_DATA.ingredients && RECIPE_DATA.ingredients.length > 0) {{
            RECIPE_DATA.ingredients.forEach(ing => addIngredientRow(ing));
        }} else {{
            addIngredientRow();
        }}

        // Init Monaco
        require.config({{ paths: {{ vs: 'https://cdnjs.cloudflare.com/ajax/libs/monaco-editor/0.45.0/min/vs' }} }});
        require(['vs/editor/editor.main'], function() {{
            monaco.editor.defineTheme('solarized-light', {{
                base: 'vs',
                inherit: true,
                rules: [
                    {{ token: '', foreground: '657b83', background: 'fdf6e3' }},
                    {{ token: 'comment', foreground: '93a1a1', fontStyle: 'italic' }},
                    {{ token: 'keyword', foreground: '859900' }},
                    {{ token: 'string', foreground: '2aa198' }},
                    {{ token: 'number', foreground: 'd33682' }},
                    {{ token: 'type', foreground: 'b58900' }},
                    {{ token: 'function', foreground: '268bd2' }},
                    {{ token: 'variable', foreground: '268bd2' }},
                    {{ token: 'constant', foreground: 'cb4b16' }},
                    {{ token: 'markup.heading', foreground: 'cb4b16', fontStyle: 'bold' }},
                    {{ token: 'markup.bold', fontStyle: 'bold' }},
                    {{ token: 'markup.italic', fontStyle: 'italic' }},
                ],
                colors: {{
                    'editor.background': '#fdf6e3',
                    'editor.foreground': '#657b83',
                    'editor.lineHighlightBackground': '#eee8d5',
                    'editor.selectionBackground': '#eee8d5',
                    'editorCursor.foreground': '#657b83',
                    'editorLineNumber.foreground': '#93a1a1',
                    'editorLineNumber.activeForeground': '#657b83',
                    'editorIndentGuide.background': '#eee8d5',
                    'editorWhitespace.foreground': '#eee8d5',
                }}
            }});

            editor = monaco.editor.create(document.getElementById('monaco-editor'), {{
                value: RECIPE_DATA.instructions || '',
                language: 'markdown',
                theme: 'solarized-light',
                fontSize: 14,
                lineNumbers: 'on',
                wordWrap: 'on',
                minimap: {{ enabled: false }},
                scrollBeyondLastLine: false,
                automaticLayout: true,
                tabSize: 2,
                insertSpaces: true,
                lineHeight: 1.7,
                padding: {{ top: 16, bottom: 16 }},
                fontFamily: '"SF Mono", "Consolas", "Liberation Mono", monospace',
                cursorBlinking: 'smooth',
                smoothScrolling: true,
                renderLineHighlight: 'line',
                folding: false,
            }});

            // Cmd/Ctrl+S in Monaco
            editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, function() {{
                saveRecipe();
            }});
        }});
    }});
    </script>
</body>
</html>"##,
        page_title = html_escape(page_title),
        style = STYLE,
        editor_style = EDITOR_STYLE,
        back_href = html_escape(&back_href),
        recipe_json = recipe_json,
    )
}

/// Escape a string for embedding in JSON.
fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

const EDITOR_STYLE: &str = r#"
.editor-body {
    margin: 0;
    padding: 0;
    height: 100vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
}

.editor-toolbar {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.5rem 1rem;
    border-bottom: 1px solid var(--border);
    background: var(--bg);
    flex-shrink: 0;
}

.editor-back {
    font-size: 0.9rem;
    white-space: nowrap;
}

.editor-title-input {
    flex: 1;
    padding: 0.4rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--bg);
    color: var(--fg);
    font-size: 1.1rem;
    font-weight: 600;
    font-family: inherit;
}
.editor-title-input:focus {
    outline: none;
    border-color: var(--blue);
}

.save-status {
    font-size: 0.85rem;
    color: var(--muted);
    white-space: nowrap;
}
.save-status-ok { color: var(--green); }
.save-status-error { color: var(--red); }

.editor-meta-bar {
    display: flex;
    align-items: center;
    gap: 1.5rem;
    padding: 0.4rem 1rem;
    border-bottom: 1px solid var(--border);
    background: var(--accent);
    font-size: 0.85rem;
    flex-shrink: 0;
}

.meta-input {
    padding: 0.25rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: 3px;
    background: var(--bg);
    color: var(--fg);
    font-size: 0.85rem;
    font-family: inherit;
    width: 60px;
}
.meta-input-wide { width: 200px; }

.editor-panels {
    display: flex;
    flex: 1;
    min-height: 0;
    overflow: hidden;
}

.editor-left {
    width: 35%;
    min-width: 280px;
    border-right: 1px solid var(--border);
    padding: 0.75rem;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
}

.editor-right {
    flex: 1;
    min-width: 0;
}

.monaco-container {
    width: 100%;
    height: 100%;
}

.ingredients-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
}

.ingredients-header h3 {
    margin: 0;
    font-size: 0.95rem;
    color: var(--base01);
}

.ing-count {
    font-size: 0.8rem;
    color: var(--muted);
}
.ing-count.valid { color: var(--green); }

.ingredient-rows {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
}

.ing-row {
    display: flex;
    gap: 0.3rem;
    align-items: center;
}

.ing-qty {
    width: 60px;
    padding: 0.3rem 0.4rem;
    border: 1px solid var(--border);
    border-radius: 3px;
    background: var(--bg);
    color: var(--fg);
    font-size: 0.85rem;
    font-family: inherit;
    text-align: right;
}

.ing-unit {
    width: 65px;
    padding: 0.3rem 0.4rem;
    border: 1px solid var(--border);
    border-radius: 3px;
    background: var(--bg);
    color: var(--fg);
    font-size: 0.85rem;
    font-family: inherit;
}

.ing-name {
    flex: 1;
    padding: 0.3rem 0.4rem;
    border: 1px solid var(--border);
    border-radius: 3px;
    background: var(--bg);
    color: var(--fg);
    font-size: 0.85rem;
    font-family: inherit;
}

.ing-qty:focus, .ing-unit:focus, .ing-name:focus {
    outline: none;
    border-color: var(--blue);
}

.ing-remove {
    background: none;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: 1.1rem;
    padding: 0 0.3rem;
    line-height: 1;
}
.ing-remove:hover { color: var(--red); }

/* Mobile: stack panels vertically */
@media (max-width: 700px) {
    .editor-panels {
        flex-direction: column;
    }
    .editor-left {
        width: 100%;
        min-width: 0;
        border-right: none;
        border-bottom: 1px solid var(--border);
        max-height: 40vh;
    }
    .editor-right {
        min-height: 300px;
    }
    .meta-input-wide { width: 140px; }
}
"#;
