//! CSS styles for the recipes application (Solarized Light theme).

pub const STYLE: &str = r#"
/* Solarized Light Theme */
:root {
    --base03: #002b36;
    --base02: #073642;
    --base01: #586e75;
    --base00: #657b83;
    --base0: #839496;
    --base1: #93a1a1;
    --base2: #eee8d5;
    --base3: #fdf6e3;

    --yellow: #b58900;
    --orange: #cb4b16;
    --red: #dc322f;
    --magenta: #d33682;
    --violet: #6c71c4;
    --blue: #268bd2;
    --cyan: #2aa198;
    --green: #859900;

    --bg: var(--base3);
    --fg: var(--base00);
    --muted: var(--base1);
    --border: var(--base2);
    --link: var(--blue);
    --link-hover: var(--cyan);
    --accent: var(--base2);
    --code-bg: var(--base2);
    --highlight: #f7f2e2;
}

* { box-sizing: border-box; margin: 0; padding: 0; }

body {
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
    line-height: 1.6;
    color: var(--fg);
    background: var(--bg);
}

.container {
    max-width: 900px;
    margin: 0 auto;
    padding: 1rem;
}

a { color: var(--link); text-decoration: none; }
a:hover { color: var(--link-hover); text-decoration: underline; }

h1, h2, h3 { font-weight: 600; margin-top: 1.5em; margin-bottom: 0.5em; }
h1 { font-size: 1.5rem; }

/* Navigation */
.nav-bar {
    position: sticky;
    top: 0;
    background: var(--bg);
    border-bottom: 1px solid var(--border);
    padding: 0.5rem 1rem;
    display: flex;
    gap: 1rem;
    align-items: center;
    flex-wrap: wrap;
    z-index: 100;
}

.nav-bar a, .nav-bar button { font-size: 0.9rem; }
.nav-bar .spacer { flex: 1; }

.nav-bar button {
    background: none;
    border: none;
    color: var(--link);
    cursor: pointer;
    font-family: inherit;
}
.nav-bar button:hover { color: var(--link-hover); text-decoration: underline; }

/* Recipe List */
.recipe-list { list-style: none; }

.recipe-item {
    padding: 0.75rem 0;
    border-bottom: 1px solid var(--border);
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 1rem;
}

.recipe-item:last-child { border-bottom: none; }
.recipe-item .title { font-size: 1rem; }
.recipe-item .meta { font-size: 0.8rem; color: var(--muted); white-space: nowrap; }

.tag-badge {
    font-size: 0.65rem;
    padding: 0.1rem 0.4rem;
    background: var(--accent);
    border-radius: 3px;
    text-transform: lowercase;
    letter-spacing: 0.05em;
    margin-right: 0.3rem;
    color: var(--base01);
}

/* Recipe View */
.recipe-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 1rem;
    flex-wrap: wrap;
    gap: 0.5rem;
}

.recipe-header h1 { margin: 0; flex: 1; }

.recipe-meta {
    background: var(--accent);
    padding: 0.5rem 0.75rem;
    margin-bottom: 1rem;
    border-radius: 4px;
    font-size: 0.85rem;
}

.recipe-meta .meta-row {
    display: flex;
    gap: 0.5rem;
}
.recipe-meta .meta-label {
    font-weight: 600;
    color: var(--base01);
    min-width: 80px;
}

.ingredient-list {
    list-style: none;
    margin: 1rem 0;
}

.ingredient-list li {
    padding: 0.3rem 0;
    border-bottom: 1px solid var(--border);
    font-size: 0.95rem;
}

.ingredient-list li:last-child { border-bottom: none; }

.ingredient-qty {
    font-weight: 600;
    color: var(--base01);
    margin-right: 0.3rem;
}

.ingredient-unit {
    color: var(--muted);
    margin-right: 0.3rem;
}

/* Recipe Content */
.recipe-content { margin-top: 1rem; }
.recipe-content pre {
    background: var(--accent);
    padding: 1rem;
    overflow-x: auto;
    border-radius: 4px;
    margin: 1rem 0;
}
.recipe-content code {
    font-family: "SF Mono", "Consolas", "Liberation Mono", monospace;
    font-size: 0.9em;
}
.recipe-content p code {
    background: var(--accent);
    padding: 0.1rem 0.3rem;
    border-radius: 3px;
}
.recipe-content blockquote {
    border-left: 3px solid var(--border);
    margin: 1rem 0;
    padding-left: 1rem;
    color: var(--muted);
}
.recipe-content ul, .recipe-content ol {
    margin: 1rem 0;
    padding-left: 1.5rem;
}
.recipe-content p { margin: 1rem 0; }

/* Buttons */
.btn {
    padding: 0.5rem 1rem;
    border: 1px solid var(--base1);
    border-radius: 4px;
    background: var(--blue);
    color: var(--base3);
    cursor: pointer;
    font-size: 0.9rem;
    font-family: inherit;
    text-decoration: none;
    display: inline-block;
}

.btn:hover { background: var(--cyan); border-color: var(--cyan); color: var(--base3); text-decoration: none; }
.btn.secondary { background: var(--base2); color: var(--base00); border-color: var(--base1); }
.btn.secondary:hover { background: var(--base3); }
.btn.danger { background: var(--red); border-color: var(--red); }
.btn.danger:hover { background: #b02020; border-color: #b02020; }
.btn.small { padding: 0.25rem 0.5rem; font-size: 0.8rem; }

.mode-toggle {
    display: flex;
    gap: 0;
    border: 1px solid var(--border);
    border-radius: 4px;
    overflow: hidden;
}

.mode-toggle a, .mode-toggle button {
    padding: 0.4rem 1rem;
    border: none;
    background: var(--accent);
    color: var(--fg);
    cursor: pointer;
    font-size: 0.85rem;
    font-family: inherit;
    text-decoration: none;
}

.mode-toggle a:hover, .mode-toggle button:hover {
    background: var(--border);
    text-decoration: none;
}

/* Login Form */
.login-form {
    max-width: 300px;
    margin: 4rem auto;
    padding: 2rem;
    background: var(--accent);
    border-radius: 8px;
}

.login-form h1 {
    margin-top: 0;
    margin-bottom: 1.5rem;
    text-align: center;
}

.login-form input {
    width: 100%;
    padding: 0.75rem;
    margin-bottom: 1rem;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--bg);
    color: var(--fg);
    font-size: 1rem;
}

.login-form button {
    width: 100%;
    padding: 0.75rem;
    background: var(--link);
    color: white;
    border: none;
    border-radius: 4px;
    font-size: 1rem;
    cursor: pointer;
}

.login-form button:hover { background: var(--link-hover); }

.message {
    padding: 0.75rem 1rem;
    border-radius: 4px;
    margin-bottom: 1rem;
}
.message.error { background: #fdf2f2; color: var(--red); border: 1px solid var(--red); }
.message.success { background: #f5f9f5; color: var(--green); border: 1px solid var(--green); }

.back-link {
    display: inline-block;
    margin-bottom: 1rem;
    font-size: 0.9rem;
}

/* Edit Form */
.edit-form { max-width: 100%; }

.form-group {
    margin-bottom: 1rem;
}

.form-group label {
    display: block;
    margin-bottom: 0.25rem;
    font-weight: 600;
    font-size: 0.9rem;
}

.form-group input, .form-group textarea {
    width: 100%;
    padding: 0.5rem 0.75rem;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--bg);
    color: var(--fg);
    font-size: 0.95rem;
    font-family: inherit;
}

.form-group textarea {
    font-family: "SF Mono", "Consolas", "Liberation Mono", monospace;
    font-size: 0.9rem;
    min-height: 400px;
    resize: vertical;
}

.form-actions {
    display: flex;
    gap: 0.5rem;
    margin-top: 1rem;
}

/* Shopping Page */
.shopping-recipes {
    list-style: none;
    margin: 1rem 0;
}

.shopping-recipe-item {
    padding: 0.5rem 0;
    border-bottom: 1px solid var(--border);
    display: flex;
    align-items: center;
    gap: 0.75rem;
}

.shopping-recipe-item:last-child { border-bottom: none; }

.shopping-recipe-item input[type="checkbox"] {
    width: 18px;
    height: 18px;
    cursor: pointer;
}

.shopping-recipe-item input[type="number"] {
    width: 60px;
    padding: 0.25rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--bg);
    color: var(--fg);
    font-size: 0.9rem;
    text-align: center;
}

.shopping-recipe-item label {
    flex: 1;
    cursor: pointer;
}

.shopping-results {
    margin-top: 2rem;
}

.shopping-section h2 {
    font-size: 1.1rem;
    margin-top: 1.5rem;
    margin-bottom: 0.5rem;
}

.shopping-item {
    padding: 0.4rem 0;
    border-bottom: 1px solid var(--border);
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 0.5rem;
}

.shopping-item:last-child { border-bottom: none; }

.shopping-item-info {
    flex: 1;
}

.shopping-item-name { font-weight: 500; }

.shopping-item-qty {
    font-size: 0.9rem;
    color: var(--base01);
}

.shopping-item-sources {
    font-size: 0.75rem;
    color: var(--muted);
}

.shopping-item.have {
    opacity: 0.5;
}

.shopping-item.have .shopping-item-name {
    text-decoration: line-through;
}

/* Pantry Page */
.pantry-list {
    list-style: none;
    margin: 1rem 0;
}

.pantry-item {
    padding: 0.4rem 0;
    border-bottom: 1px solid var(--border);
    display: flex;
    justify-content: space-between;
    align-items: center;
}

.pantry-item:last-child { border-bottom: none; }

.pantry-item-name {
    font-size: 0.95rem;
}

.pantry-add-form {
    display: flex;
    gap: 0.5rem;
    margin: 1rem 0;
}

.pantry-add-form input {
    flex: 1;
    padding: 0.5rem 0.75rem;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--bg);
    color: var(--fg);
    font-size: 0.95rem;
}

.empty-state {
    text-align: center;
    color: var(--muted);
    padding: 3rem 1rem;
    font-size: 0.95rem;
}

/* Floating Action Button */
.fab {
    position: fixed;
    bottom: 2rem;
    right: 2rem;
    width: 56px;
    height: 56px;
    border-radius: 50%;
    background: var(--blue);
    color: var(--base3);
    border: none;
    font-size: 1.75rem;
    line-height: 56px;
    text-align: center;
    cursor: pointer;
    box-shadow: 0 2px 8px rgba(0,0,0,0.2);
    text-decoration: none;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: background 0.15s, box-shadow 0.15s, transform 0.15s;
    z-index: 200;
}
.fab:hover {
    background: var(--cyan);
    color: var(--base3);
    text-decoration: none;
    box-shadow: 0 4px 16px rgba(0,0,0,0.25);
    transform: scale(1.05);
}

/* Pantry badge on ingredients */
.pantry-badge {
    font-size: 0.65rem;
    padding: 0.1rem 0.4rem;
    border: 1px solid var(--border);
    border-radius: 3px;
    background: var(--bg);
    color: var(--muted);
    cursor: pointer;
    font-family: inherit;
    margin-left: 0.4rem;
    vertical-align: middle;
}

.pantry-badge:hover {
    border-color: var(--base1);
    background: var(--accent);
}

.pantry-badge.have {
    background: var(--green);
    color: var(--base3);
    border-color: var(--green);
}

.pantry-badge.have:hover {
    opacity: 0.8;
}

/* Ready to Make / Almost Ready sections */
.ready-section {
    margin-bottom: 1.5rem;
    padding: 0.75rem;
    background: var(--accent);
    border-radius: 6px;
}

.ready-heading {
    font-size: 0.85rem;
    font-weight: 600;
    color: var(--base01);
    margin: 0 0 0.4rem 0;
    text-transform: uppercase;
    letter-spacing: 0.05em;
}

.ready-item {
    padding: 0.2rem 0;
    font-size: 0.9rem;
}

.ready-item a { font-weight: 500; }

.almost-item {
    padding: 0.2rem 0;
    font-size: 0.9rem;
    display: flex;
    align-items: center;
    gap: 0.4rem;
    flex-wrap: wrap;
}

.almost-item a { font-weight: 500; }

.missing-tag {
    font-size: 0.7rem;
    padding: 0.1rem 0.35rem;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 3px;
    color: var(--muted);
}

/* Shopping two-panel layout */
.shop-layout {
    display: flex;
    gap: 2rem;
    align-items: flex-start;
}

.shop-left {
    flex: 0 0 35%;
    min-width: 0;
}

.shop-right {
    flex: 1;
    min-width: 0;
}

@media (max-width: 700px) {
    .shop-layout {
        flex-direction: column;
    }
    .shop-left { flex: none; width: 100%; }
}

.shop-left h2, .shop-right h2 {
    font-size: 1rem;
    margin-top: 0;
    margin-bottom: 0.5rem;
}

.recent-trips { margin-top: 1.5rem; }

.recent-trips h3 {
    font-size: 0.85rem;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: 0.5rem;
}

.trip-row {
    padding: 0.3rem 0;
    border-bottom: 1px solid var(--border);
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 0.85rem;
}

.trip-row:last-child { border-bottom: none; }

/* Trip page (print-friendly) */
.trip-page h1 { font-size: 1.3rem; margin-bottom: 0.25rem; }
.trip-date { color: var(--muted); font-size: 0.85rem; margin-bottom: 1rem; }

.trip-list {
    list-style: none;
    margin: 0;
}

.trip-list li {
    padding: 0.35rem 0;
    border-bottom: 1px solid var(--border);
    font-size: 0.95rem;
}

.trip-list li:last-child { border-bottom: none; }

@media print {
    .nav-bar, .fab, .btn { display: none !important; }
    .container { max-width: 100%; }
}
"#;
