//! Shopping list builder and results template.

use crate::models::{Recipe, ShoppingItem};
use crate::recipes::html_escape;
use crate::shopping::SavedTrip;
use crate::templates::base_html;

/// Render the shopping list builder page (two-panel layout).
pub fn render_shopping_page(recipes: &[Recipe], recent_trips: &[SavedTrip], logged_in: bool) -> String {
    let mut html = String::new();

    html.push_str("<h1>Shopping List</h1>");

    if recipes.is_empty() {
        html.push_str(r#"<div class="empty-state"><p>No recipes yet. Create some recipes first!</p></div>"#);
        return base_html("Shopping List", &html, logged_in);
    }

    html.push_str(r#"<div class="shop-layout">"#);

    // Left panel: recipe selection
    html.push_str(r#"<div class="shop-left">"#);
    html.push_str(r#"<h2>Select Recipes</h2>"#);
    html.push_str(r#"<ul class="shopping-recipes">"#);

    for recipe in recipes {
        let ingredient_count = recipe.ingredients.len();
        if ingredient_count == 0 {
            continue;
        }

        html.push_str(&format!(
            r#"<li class="shopping-recipe-item">
                <input type="checkbox" id="sel-{key}" data-key="{key}" class="shop-cb">
                <label for="sel-{key}">{title}</label>
                <input type="number" id="qty-{key}" min="0.5" step="0.5" value="1" class="shop-qty" title="Multiplier">
            </li>"#,
            key = recipe.key,
            title = html_escape(&recipe.title),
        ));
    }

    html.push_str("</ul>");
    html.push_str("</div>");

    // Right panel: shopping list results
    html.push_str(r#"<div class="shop-right">"#);
    html.push_str(r#"<h2>Shopping List</h2>"#);
    html.push_str(r#"<div id="shopping-results"><p style="color:var(--muted)">Select recipes to build your list.</p></div>"#);

    // Recent trips
    if !recent_trips.is_empty() {
        html.push_str(r#"<div class="recent-trips">"#);
        html.push_str(r#"<h3>Recent Trips</h3>"#);
        for trip in recent_trips {
            let date = &trip.created_at[..10]; // YYYY-MM-DD
            let count = trip.items.len();
            html.push_str(&format!(
                r#"<div class="trip-row">
                    <span>{date} &middot; {count} items</span>
                    <a href="/shopping/trip/{id}" class="btn small secondary">View</a>
                </div>"#,
                date = html_escape(date),
                count = count,
                id = html_escape(&trip.id),
            ));
        }
        html.push_str("</div>");
    }

    html.push_str("</div>"); // shop-right
    html.push_str("</div>"); // shop-layout

    // JavaScript for live updates
    html.push_str(r#"<script>
    let debounceTimer = null;

    function scheduleRebuild() {
        clearTimeout(debounceTimer);
        debounceTimer = setTimeout(rebuildList, 200);
    }

    document.querySelectorAll('.shop-cb').forEach(cb => {
        cb.addEventListener('change', scheduleRebuild);
    });
    document.querySelectorAll('.shop-qty').forEach(inp => {
        inp.addEventListener('input', scheduleRebuild);
    });

    async function rebuildList() {
        const selections = [];
        document.querySelectorAll('.shop-cb:checked').forEach(cb => {
            const key = cb.dataset.key;
            const qty = parseFloat(document.getElementById('qty-' + key).value) || 1;
            selections.push({ key: key, multiplier: qty });
        });

        if (selections.length === 0) {
            document.getElementById('shopping-results').innerHTML =
                '<p style="color:var(--muted)">Select recipes to build your list.</p>';
            return;
        }

        try {
            const resp = await fetch('/api/shopping/build', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ selections: selections })
            });
            if (!resp.ok) return;
            document.getElementById('shopping-results').innerHTML = await resp.text();
        } catch (e) { /* ignore network blips during typing */ }
    }

    async function toggleShoppingItem(name, btn) {
        try {
            const resp = await fetch('/api/pantry/toggle', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ name: name })
            });
            if (!resp.ok) { alert('Error toggling item'); return; }
            const data = await resp.json();
            const item = btn.closest('.shopping-item');
            if (data.in_pantry) {
                item.classList.add('have');
                btn.textContent = 'In pantry';
            } else {
                item.classList.remove('have');
                btn.textContent = 'Not in pantry';
            }
        } catch (e) { alert('Error: ' + e.message); }
    }

    async function addAllToPantry() {
        const names = [];
        document.querySelectorAll('.shopping-item:not(.have) .shopping-item-name').forEach(el => {
            names.push(el.dataset.name);
        });
        if (names.length === 0) return;

        try {
            const resp = await fetch('/api/shopping/to-pantry', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ names: names })
            });
            if (resp.ok) {
                document.querySelectorAll('.shopping-item:not(.have)').forEach(item => {
                    item.classList.add('have');
                    const btn = item.querySelector('.btn');
                    if (btn) btn.textContent = 'In pantry';
                });
            }
        } catch (e) { alert('Error: ' + e.message); }
    }

    async function saveTrip() {
        const items = [];
        document.querySelectorAll('.shopping-item').forEach(el => {
            const nameEl = el.querySelector('.shopping-item-name');
            const qtyEl = el.querySelector('.shopping-item-qty');
            if (!nameEl) return;
            const name = nameEl.dataset.name || nameEl.textContent;
            const qtyText = qtyEl ? qtyEl.textContent.trim() : '';
            const parts = qtyText.split(' ');
            const qty = parseFloat(parts[0]) || 0;
            const unit = parts.slice(1).join(' ');
            const inPantry = el.classList.contains('have');
            const sourcesEl = el.querySelector('.shopping-item-sources');
            const sources = sourcesEl ? sourcesEl.textContent.replace('from: ', '').split(', ') : [];
            items.push({ name, qty, unit, in_pantry: inPantry, sources });
        });

        if (items.length === 0) { alert('No items to save.'); return; }

        try {
            const resp = await fetch('/api/shopping/save-trip', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ items: items })
            });
            if (!resp.ok) { alert('Error saving trip'); return; }
            const data = await resp.json();
            alert('Trip saved!');
            location.reload();
        } catch (e) { alert('Error: ' + e.message); }
    }
    </script>"#);

    base_html("Shopping List", &html, logged_in)
}

/// Render the shopping list results as an HTML fragment (returned via AJAX).
pub fn render_shopping_results(items: &[ShoppingItem]) -> String {
    if items.is_empty() {
        return r#"<p style="color:var(--muted)">No ingredients needed.</p>"#.to_string();
    }

    let need: Vec<&ShoppingItem> = items.iter().filter(|i| !i.in_pantry).collect();
    let have: Vec<&ShoppingItem> = items.iter().filter(|i| i.in_pantry).collect();

    let mut html = String::new();

    // Action buttons
    html.push_str(r#"<div style="margin-bottom:1rem;display:flex;gap:0.5rem">"#);
    if !need.is_empty() {
        html.push_str(r#"<button class="btn small" onclick="addAllToPantry()">Add all to pantry</button>"#);
    }
    html.push_str(r#"<button class="btn small secondary" onclick="saveTrip()">Save Trip</button>"#);
    html.push_str("</div>");

    // Need to buy
    if !need.is_empty() {
        html.push_str(&format!(
            r#"<div class="shopping-section"><h2>Need to Buy ({})</h2>"#,
            need.len()
        ));
        for item in &need {
            render_shopping_item(&mut html, item);
        }
        html.push_str("</div>");
    }

    // Already have
    if !have.is_empty() {
        html.push_str(&format!(
            r#"<div class="shopping-section"><h2 style="color:var(--muted)">Already Have ({})</h2>"#,
            have.len()
        ));
        for item in &have {
            render_shopping_item(&mut html, item);
        }
        html.push_str("</div>");
    }

    html
}

fn render_shopping_item(html: &mut String, item: &ShoppingItem) {
    let class = if item.in_pantry {
        "shopping-item have"
    } else {
        "shopping-item"
    };
    let btn_label = if item.in_pantry {
        "In pantry"
    } else {
        "Not in pantry"
    };
    let sources = if item.sources.is_empty() {
        String::new()
    } else {
        format!(
            r#"<div class="shopping-item-sources">from: {}</div>"#,
            html_escape(&item.sources.join(", "))
        )
    };

    let escaped_name = html_escape(&item.name);
    let js_name = item.name.replace('\\', "\\\\").replace('\'', "\\'");

    html.push_str(&format!(
        r#"<div class="{class}">
            <div class="shopping-item-info">
                <span class="shopping-item-name" data-name="{name}">{name}</span>
                <span class="shopping-item-qty">{qty} {unit}</span>
                {sources}
            </div>
            <button class="btn small secondary" onclick="toggleShoppingItem('{js_name}', this)">{btn_label}</button>
        </div>"#,
        class = class,
        name = escaped_name,
        qty = item.qty,
        unit = html_escape(&item.unit),
        sources = sources,
        js_name = js_name,
        btn_label = btn_label,
    ));
}

/// Render a saved trip as a print-friendly page.
pub fn render_trip_page(trip: &SavedTrip, logged_in: bool) -> String {
    let mut html = String::new();

    let date = &trip.created_at[..10];
    html.push_str(r#"<div class="trip-page">"#);
    html.push_str(&format!(
        r#"<h1>Shopping Trip</h1><div class="trip-date">{}</div>"#,
        html_escape(date)
    ));

    let need: Vec<&ShoppingItem> = trip.items.iter().filter(|i| !i.in_pantry).collect();
    let have: Vec<&ShoppingItem> = trip.items.iter().filter(|i| i.in_pantry).collect();

    if !need.is_empty() {
        html.push_str(&format!("<h2>Need to Buy ({})</h2>", need.len()));
        html.push_str(r#"<ul class="trip-list">"#);
        for item in &need {
            html.push_str(&format!(
                "<li><strong>{}</strong> &middot; {} {}</li>",
                html_escape(&item.name),
                item.qty,
                html_escape(&item.unit),
            ));
        }
        html.push_str("</ul>");
    }

    if !have.is_empty() {
        html.push_str(&format!(
            r#"<h2 style="color:var(--muted)">Already Have ({})</h2>"#,
            have.len()
        ));
        html.push_str(r#"<ul class="trip-list">"#);
        for item in &have {
            html.push_str(&format!(
                r#"<li style="color:var(--muted)">{} &middot; {} {}</li>"#,
                html_escape(&item.name),
                item.qty,
                html_escape(&item.unit),
            ));
        }
        html.push_str("</ul>");
    }

    html.push_str(r#"<div style="margin-top:1.5rem"><a href="/shopping" class="btn secondary">Back to Shopping</a></div>"#);
    html.push_str("</div>");

    base_html("Shopping Trip", &html, logged_in)
}
