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
    align-items: center;
    gap: 1rem;
}

.recipe-item:last-child { border-bottom: none; }
.recipe-main {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex: 1;
    min-width: 0;
}
.recipe-item .title {
    font-size: 1rem;
    white-space: nowrap;
}
.recipe-tags {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
}
.recipe-tags .tag-badge { margin-right: 0; }
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

.trip-recipes {
    list-style: none;
    margin: 0 0 1rem 0;
}

.trip-recipes li {
    padding: 0.35rem 0;
    border-bottom: 1px solid var(--border);
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 0.5rem;
}

.trip-recipes li:last-child { border-bottom: none; }

.trip-recipe-meta {
    color: var(--muted);
    font-size: 0.85rem;
}

.trip-actions {
    display: flex;
    gap: 0.5rem;
    margin: 0.5rem 0 0.5rem;
    flex-wrap: wrap;
}

.instacart-note {
    color: var(--muted);
    font-size: 0.85rem;
    margin-bottom: 0.5rem;
}

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

.trip-buy-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 0.75rem;
}

.trip-item-copy {
    min-width: 0;
    flex: 1;
}

@media (max-width: 700px) {
    .trip-buy-row {
        flex-direction: column;
        align-items: flex-start;
    }
}

.trip-item-sources {
    color: var(--muted);
    font-size: 0.8rem;
    margin-top: 0.15rem;
}

.trip-notes { margin-bottom: 1.25rem; }

.trip-recipe-card {
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.75rem 1rem;
    margin-bottom: 1rem;
}

.trip-recipe-card h3 {
    font-size: 1.05rem;
    margin: 0 0 0.25rem 0;
}

.trip-recipe-tags {
    color: var(--muted);
    font-size: 0.8rem;
    margin-bottom: 0.5rem;
}

@media print {
    .trip-recipe-card { page-break-inside: avoid; border: none; padding: 0; }
}

/* Active-trip banner (persistent "go to active trip" button) */
.active-trip-banner {
    position: sticky;
    top: 0;
    z-index: 99;
    background: var(--green);
    text-align: center;
    padding: 0.5rem 1rem;
}
.active-trip-banner a {
    color: var(--base3);
    font-weight: 600;
    font-size: 0.95rem;
    text-decoration: none;
}
.active-trip-banner a:hover { text-decoration: underline; color: var(--base3); }

/* Trip checklist: progress bar */
.trip-progress-wrap { margin: 0.5rem 0 1rem; }
.trip-progress {
    height: 8px;
    background: var(--border);
    border-radius: 4px;
    overflow: hidden;
}
.trip-progress-bar {
    height: 100%;
    background: var(--green);
    transition: width 0.2s ease;
}
.trip-progress-text {
    font-size: 0.85rem;
    color: var(--muted);
    margin-top: 0.25rem;
}

/* Trip checklist: aisle groups */
.aisle-group { margin-bottom: 1.25rem; }
.aisle-heading {
    font-size: 0.8rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--base01);
    border-bottom: 2px solid var(--border);
    padding-bottom: 0.2rem;
    margin: 1.25rem 0 0.4rem;
}

/* Trip checklist: checkable rows */
.trip-check-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.45rem 0;
    border-bottom: 1px solid var(--border);
}
.trip-check-row:last-child { border-bottom: none; }

.trip-check-label {
    display: flex;
    align-items: flex-start;
    gap: 0.6rem;
    flex: 1;
    min-width: 0;
    cursor: pointer;
}
.trip-check {
    width: 22px;
    height: 22px;
    margin-top: 0.1rem;
    cursor: pointer;
    flex: none;
    accent-color: var(--green);
}
.trip-check-body { min-width: 0; }
.trip-check-name { font-weight: 500; }
.trip-check-qty { color: var(--base01); font-size: 0.9rem; margin-left: 0.4rem; }

.trip-check-row.checked .trip-check-name,
.trip-check-row.checked .trip-check-qty { text-decoration: line-through; }
.trip-check-row.checked { opacity: 0.55; }

.trip-check-tools {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    flex: none;
}
.aisle-select {
    font-size: 0.75rem;
    padding: 0.15rem 0.25rem;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--bg);
    color: var(--muted);
    font-family: inherit;
    max-width: 9rem;
}

.trip-have-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 0.5rem;
}
.trip-have-name { color: var(--muted); }

@media (max-width: 700px) {
    .trip-check-row { flex-wrap: wrap; }
    .trip-check-tools { width: 100%; padding-left: 1.9rem; }
}

@media print {
    .active-trip-banner, .aisle-select, .trip-progress-wrap { display: none !important; }
}

/* ===== Meal plan (weekly planner) ===== */
/* The week board itself is the shared kcal component (src/vendor/kcal.css,
   inlined by the plan template); map its theme variables to Solarized. */
.kcal {
    --kcal-line: var(--border);
    --kcal-text: var(--base01);
    --kcal-muted: var(--base1);
    --kcal-bg: var(--base2);
    --kcal-head-bg: var(--base2);
    --kcal-cell: var(--base3);
    --kcal-cell-2: var(--highlight);
    --kcal-accent: var(--cyan);
    --kcal-on-accent: #fff;
    --kcal-radius: 8px;
    --kcal-week-min: 170px;
}

.plan-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 0.5rem;
    margin-bottom: 0.75rem;
}
.plan-header h1 { margin: 0.5em 0 0; }
.plan-week-label { color: var(--base01); font-weight: 600; }
.plan-nav { display: flex; gap: 0.35rem; margin-top: 0.9rem; }
.plan-nav .btn { text-decoration: none; }

.meal-chip {
    display: flex;
    align-items: center;
    gap: 0.25rem;
    font-size: 0.8rem;
    line-height: 1.35;
    padding: 0.15rem 0.4rem;
    border-radius: 5px;
    background: color-mix(in srgb, var(--chip) 12%, transparent);
    border-left: 3px solid var(--chip);
    flex: none;
    min-width: 0;
}
.meal-chip-title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.meal-chip-title a { color: var(--base01); }
.meal-mult { color: var(--muted); font-size: 0.7rem; flex: none; }
.meal-remove {
    flex: none;
    border: none;
    background: none;
    color: var(--muted);
    cursor: pointer;
    font-size: 0.85rem;
    line-height: 1;
    padding: 0 0.1rem;
}
.meal-remove:hover { color: var(--red); }

.meal-add-btn {
    width: 100%;
    border: 1px dashed var(--border);
    background: none;
    border-radius: 6px;
    padding: 0.25rem;
    color: var(--muted);
    cursor: pointer;
    font-size: 0.75rem;
}
.meal-add-btn:hover { color: var(--base01); border-color: var(--base1); }

/* Shopping-trip association panel */
.plan-trip-panel {
    margin-top: 1.25rem;
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 0.9rem 1rem 1rem;
    background: var(--highlight);
}
.plan-trip-panel h2 { margin-top: 0; font-size: 1.05rem; }
.plan-trip-hint { color: var(--muted); font-size: 0.85rem; margin: 0.25rem 0 0.6rem; }
.plan-trip-linked {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    flex-wrap: wrap;
}
.plan-trip-link { font-weight: 600; }
.plan-trip-recent { margin-top: 0.9rem; }
.plan-trip-row {
    display: flex;
    justify-content: space-between;
    gap: 0.5rem;
    width: 100%;
    text-align: left;
    border: 1px solid var(--border);
    background: var(--bg);
    border-radius: 6px;
    padding: 0.4rem 0.6rem;
    margin-top: 0.35rem;
    cursor: pointer;
    font-size: 0.85rem;
    color: var(--fg);
}
.plan-trip-row:hover { border-color: var(--base1); }
.plan-trip-meta { color: var(--muted); }

/* Add-a-meal picker overlay */
.meal-picker {
    position: fixed;
    inset: 0;
    background: rgba(0, 43, 54, 0.35);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
    padding: 1rem;
}
.meal-picker[hidden] { display: none; }
.meal-picker-card {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 1rem;
    width: 100%;
    max-width: 26rem;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
}
.meal-picker-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
}
.meal-picker-head h2 { margin: 0; font-size: 1rem; }
.meal-picker-card input[type="text"],
.meal-picker-card input[type="number"] {
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.4rem 0.6rem;
    font-family: inherit;
    font-size: 0.9rem;
    background: var(--base3);
    color: var(--fg);
}
.meal-picker-mult {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    color: var(--muted);
    font-size: 0.85rem;
}
.meal-picker-mult input { width: 5rem; }
.meal-recipe-list {
    flex: 1;
    min-height: 8rem;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
}
.meal-recipe-row {
    text-align: left;
    border: 1px solid var(--border);
    background: var(--base3);
    color: var(--fg);
    border-radius: 6px;
    padding: 0.4rem 0.6rem;
    cursor: pointer;
    font-size: 0.9rem;
}
.meal-recipe-row:hover { border-color: var(--cyan); }
.meal-picker-empty { color: var(--muted); font-size: 0.85rem; padding: 0.5rem 0; }
.meal-picker-custom { display: flex; gap: 0.4rem; }
.meal-picker-custom input { flex: 1; }

@media print {
    .nav-bar, .fab, .btn, .mode-toggle, .back-link, .pantry-badge { display: none !important; }
    .container { max-width: 100%; padding: 0; }
    body { font-size: 12pt; line-height: 1.5; }
    h1 { font-size: 16pt; }
    h2 { font-size: 13pt; }
    .recipe-header { margin-bottom: 0.5rem; }
    .ingredient-list li { page-break-inside: avoid; }
    .recipe-content { page-break-before: avoid; }
}
"#;
