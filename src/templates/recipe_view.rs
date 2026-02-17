//! Single recipe view page.

use crate::models::Recipe;
use crate::recipes::html_escape;
use crate::templates::base_html;
use std::collections::HashSet;

pub fn render_recipe_view(recipe: &Recipe, pantry_items: &HashSet<String>, logged_in: bool) -> String {
    let mut html = String::new();

    html.push_str(r#"<a href="/" class="back-link">&larr; All recipes</a>"#);

    // Header with edit/delete buttons
    html.push_str(r#"<div class="recipe-header">"#);
    html.push_str(&format!("<h1>{}</h1>", html_escape(&recipe.title)));
    if logged_in {
        html.push_str(r#"<div class="mode-toggle">"#);
        html.push_str(&format!(
            r#"<a href="/recipe/{key}/edit">Edit</a>"#,
            key = recipe.key,
        ));
        html.push_str(&format!(
            r#"<button onclick="confirmDelete('{key}', '{title}')">Delete</button>"#,
            key = recipe.key,
            title = html_escape(&recipe.title).replace('\'', "\\'"),
        ));
        html.push_str("</div>");
    }
    html.push_str("</div>");

    // Meta block
    let mut meta_rows = Vec::new();
    if let Some(servings) = recipe.servings {
        meta_rows.push(format!(
            r#"<div class="meta-row"><span class="meta-label">Servings</span><span>{}</span></div>"#,
            servings
        ));
    }
    if !recipe.tags.is_empty() {
        let tags_str: String = recipe
            .tags
            .iter()
            .map(|t| format!(r#"<span class="tag-badge">{}</span>"#, html_escape(t)))
            .collect();
        meta_rows.push(format!(
            r#"<div class="meta-row"><span class="meta-label">Tags</span><span>{}</span></div>"#,
            tags_str
        ));
    }
    if !meta_rows.is_empty() {
        html.push_str(r#"<div class="recipe-meta">"#);
        for row in &meta_rows {
            html.push_str(row);
        }
        html.push_str("</div>");
    }

    // Ingredients with pantry toggle badges
    if !recipe.ingredients.is_empty() {
        html.push_str("<h2>Ingredients</h2>");
        html.push_str(r#"<ul class="ingredient-list">"#);
        for ing in &recipe.ingredients {
            let norm = ing.name.trim().to_lowercase();
            let in_pantry = pantry_items.contains(&norm);
            let js_name = ing.name.replace('\\', "\\\\").replace('\'', "\\'");

            let badge = if in_pantry {
                format!(
                    r#"<button class="pantry-badge have" onclick="togglePantry('{js_name}', this)" title="Click to remove from pantry">Pantry</button>"#,
                    js_name = js_name,
                )
            } else {
                format!(
                    r#"<button class="pantry-badge" onclick="togglePantry('{js_name}', this)" title="Click to add to pantry">+ Pantry</button>"#,
                    js_name = js_name,
                )
            };

            html.push_str(&format!(
                r#"<li><span class="ingredient-qty">{qty}</span><span class="ingredient-unit">{unit}</span>{name} {badge}</li>"#,
                qty = ing.qty,
                unit = html_escape(&ing.unit),
                name = html_escape(&ing.name),
                badge = badge,
            ));
        }
        html.push_str("</ul>");
    }

    // Body
    if !recipe.body_html.is_empty() {
        html.push_str(r#"<div class="recipe-content">"#);
        html.push_str(&recipe.body_html);
        html.push_str("</div>");
    }

    // JS for delete + pantry toggle
    html.push_str(r#"<script>
    async function confirmDelete(key, title) {
        if (!confirm('Delete "' + title + '"?')) return;
        try {
            const resp = await fetch('/api/recipe/' + key, {
                method: 'DELETE',
                headers: { 'Content-Type': 'application/json' }
            });
            if (resp.ok) { window.location.href = '/'; }
            else { alert('Failed to delete: ' + await resp.text()); }
        } catch (e) { alert('Error: ' + e.message); }
    }

    async function togglePantry(name, btn) {
        try {
            const resp = await fetch('/api/pantry/toggle', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ name: name })
            });
            if (!resp.ok) { alert('Error toggling'); return; }
            const data = await resp.json();
            if (data.in_pantry) {
                btn.classList.add('have');
                btn.textContent = 'Pantry';
                btn.title = 'Click to remove from pantry';
            } else {
                btn.classList.remove('have');
                btn.textContent = '+ Pantry';
                btn.title = 'Click to add to pantry';
            }
        } catch (e) { alert('Error: ' + e.message); }
    }
    </script>"#);

    base_html(&recipe.title, &html, logged_in)
}
