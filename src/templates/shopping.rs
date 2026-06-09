//! Shopping list builder and results template.

use crate::aisle;
use crate::models::{Recipe, ShoppingItem};
use crate::recipes::{html_escape, js_single_quote_attr_escape};
use crate::shopping::{instacart_search_url, item_key, PublishedTrip, SavedTrip};
use crate::templates::base_html;
use sled::Db;

/// Render the shopping list builder page (two-panel layout).
pub fn render_shopping_page(
    recipes: &[Recipe],
    recent_trips: &[SavedTrip],
    logged_in: bool,
) -> String {
    let mut html = String::new();

    html.push_str("<h1>Shopping List</h1>");

    if recipes.is_empty() {
        html.push_str(
            r#"<div class="empty-state"><p>No recipes yet. Create some recipes first!</p></div>"#,
        );
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
            let recipe_count = trip.recipes.len();
            html.push_str(&format!(
                r#"<div class="trip-row">
                    <span>{date} &middot; {count} items &middot; {recipe_count} recipes</span>
                    <a href="/shopping/trip/{id}" class="btn small secondary">View</a>
                </div>"#,
                date = html_escape(date),
                count = count,
                recipe_count = recipe_count,
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

    function collectSelections() {
        const selections = [];
        document.querySelectorAll('.shop-cb:checked').forEach(cb => {
            const key = cb.dataset.key;
            const qty = parseFloat(document.getElementById('qty-' + key).value) || 1;
            selections.push({ key: key, multiplier: qty });
        });
        return selections;
    }

    async function rebuildList() {
        const selections = collectSelections();

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

    // Auto-select recipe from ?add= query param
    (function() {
        const params = new URLSearchParams(window.location.search);
        const addKey = params.get('add');
        if (addKey) {
            const cb = document.getElementById('sel-' + addKey);
            if (cb && !cb.checked) {
                cb.checked = true;
                rebuildList();
            }
        }
    })();

    async function saveTrip() {
        const selections = collectSelections();
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
                body: JSON.stringify({ items: items, selections: selections })
            });
            if (!resp.ok) { alert('Error saving trip'); return; }
            const data = await resp.json();
            window.location.href = '/shopping/trip/' + encodeURIComponent(data.id);
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
        html.push_str(
            r#"<button class="btn small" onclick="addAllToPantry()">Add all to pantry</button>"#,
        );
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
    let js_name = js_single_quote_attr_escape(&item.name);

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

/// Render a saved trip as an in-store checklist: items grouped by store
/// section, each checkable as you shop (persisted live), with a durable link
/// and a "done shopping" close button.
pub fn render_trip_page(db: &Db, trip: &SavedTrip, logged_in: bool) -> String {
    let mut html = String::new();

    let date = if trip.created_at.len() >= 10 {
        &trip.created_at[..10]
    } else {
        trip.created_at.as_str()
    };
    let js_trip_id = js_single_quote_attr_escape(&trip.id);

    html.push_str(r#"<div class="trip-page">"#);
    html.push_str(&format!(
        r#"<h1>Shopping Trip</h1><div class="trip-date">{}{}</div>"#,
        html_escape(date),
        if trip.closed { " &middot; closed" } else { "" },
    ));

    let need: Vec<&ShoppingItem> = trip.items.iter().filter(|i| !i.in_pantry).collect();
    let have: Vec<&ShoppingItem> = trip.items.iter().filter(|i| i.in_pantry).collect();

    let total = need.len();
    let done = trip.buy_done();

    // ---- Progress + actions ----
    let pct = if total > 0 {
        (done * 100) / total
    } else {
        0
    };
    html.push_str(&format!(
        r#"<div class="trip-progress-wrap">
            <div class="trip-progress"><div class="trip-progress-bar" id="trip-progress-bar" style="width:{pct}%"></div></div>
            <div class="trip-progress-text"><span id="trip-progress-done">{done}</span> of <span id="trip-progress-total">{total}</span> picked up</div>
        </div>"#,
        pct = pct,
        done = done,
        total = total,
    ));

    // Durable link + close/reopen.
    html.push_str(r#"<div class="trip-actions">"#);
    html.push_str(
        r#"<button class="btn small secondary" onclick="copyTripLink(this)">Copy link</button>"#,
    );
    if !need.is_empty() {
        html.push_str(&format!(
            r#"<button class="btn small secondary" onclick="openInstacartCart('{trip_id}', this)">Open Instacart Cart</button>"#,
            trip_id = js_trip_id,
        ));
    }
    if trip.closed {
        html.push_str(&format!(
            r#"<button class="btn small" onclick="reopenTrip('{trip_id}')">Reopen trip</button>"#,
            trip_id = js_trip_id,
        ));
    } else {
        html.push_str(&format!(
            r#"<button class="btn small danger" onclick="closeTrip('{trip_id}')">Done shopping</button>"#,
            trip_id = js_trip_id,
        ));
    }
    html.push_str("</div>");

    // ---- Checklist grouped by store section ----
    if need.is_empty() {
        html.push_str(
            r#"<p style="color:var(--muted)">Nothing to buy — everything's in the pantry.</p>"#,
        );
    } else {
        let need_items: Vec<ShoppingItem> = need.iter().map(|i| (*i).clone()).collect();
        let groups = aisle::group_items(db, &need_items);
        let section_options = aisle::SECTIONS;

        for (section, items) in &groups {
            html.push_str(&format!(
                r#"<div class="aisle-group"><h2 class="aisle-heading">{}</h2><ul class="trip-list">"#,
                html_escape(section)
            ));
            for item in items {
                render_checklist_item(&mut html, item, trip, section, section_options);
            }
            html.push_str("</ul></div>");
        }
    }

    // ---- Already in pantry (unobtrusive) ----
    if !have.is_empty() {
        html.push_str(&format!(
            r#"<h2 class="aisle-heading" style="color:var(--muted)">Already Have ({})</h2>"#,
            have.len()
        ));
        html.push_str(r#"<ul class="trip-list">"#);
        for item in &have {
            html.push_str(&format!(
                r#"<li style="color:var(--muted)">{} &middot; {} {}</li>"#,
                html_escape(&item.name),
                format_multiplier(item.qty),
                html_escape(&item.unit),
            ));
        }
        html.push_str("</ul>");
    }

    // ---- Recipes for this trip ----
    if !trip.recipes.is_empty() {
        html.push_str(&format!(
            "<h2 class=\"aisle-heading\">Recipes for This Trip ({})</h2>",
            trip.recipes.len()
        ));
        html.push_str(r#"<ul class="trip-recipes">"#);
        for recipe in &trip.recipes {
            html.push_str(&format!(
                r#"<li>
                    <a href="/recipe/{key}?from_trip={trip_id}">{title}</a>
                    <span class="trip-recipe-meta">&times;{multiplier}</span>
                </li>"#,
                key = html_escape(&recipe.key),
                trip_id = html_escape(&trip.id),
                title = html_escape(&recipe.title),
                multiplier = format_multiplier(recipe.multiplier),
            ));
        }
        html.push_str("</ul>");
    }

    html.push_str(r#"<div style="margin-top:1.5rem"><a href="/shopping" class="btn secondary">Back to Shopping</a></div>"#);
    html.push_str(&trip_page_script(&js_trip_id));
    html.push_str("</div>");

    base_html("Shopping Trip", &html, logged_in)
}

/// Render one checkable item row inside the in-store checklist.
fn render_checklist_item(
    html: &mut String,
    item: &ShoppingItem,
    trip: &SavedTrip,
    current_section: &str,
    section_options: &[&str],
) {
    let key = item_key(item);
    let checked = trip.is_checked(&key);
    let li_class = if checked {
        "trip-check-row checked"
    } else {
        "trip-check-row"
    };
    let checked_attr = if checked { " checked" } else { "" };
    let instacart_url = instacart_search_url(&item.name);

    let sources = if item.sources.is_empty() {
        String::new()
    } else {
        format!(
            r#"<div class="trip-item-sources">for: {}</div>"#,
            html_escape(&item.sources.join(", "))
        )
    };

    // Per-item section override dropdown (a misfiled item can be corrected
    // once and the override persists across trips).
    let mut section_select = format!(
        r#"<select class="aisle-select" data-name="{name}" onchange="changeSection(this)" title="Move to a different section">"#,
        name = html_escape(&item.name),
    );
    for opt in section_options {
        let selected = if *opt == current_section {
            " selected"
        } else {
            ""
        };
        section_select.push_str(&format!(
            r#"<option value="{v}"{sel}>{v}</option>"#,
            v = html_escape(opt),
            sel = selected,
        ));
    }
    section_select.push_str("</select>");

    html.push_str(&format!(
        r#"<li class="{li_class}">
            <label class="trip-check-label">
                <input type="checkbox" class="trip-check" data-key="{key}"{checked_attr} onchange="toggleTripItem(this)">
                <span class="trip-check-body">
                    <span class="trip-check-name">{name}</span>
                    <span class="trip-check-qty">{qty} {unit}</span>
                    {sources}
                </span>
            </label>
            <span class="trip-check-tools">
                {section_select}
                <a href="{url}" target="_blank" rel="noopener" class="btn small secondary">Search</a>
            </span>
        </li>"#,
        li_class = li_class,
        key = html_escape(&key),
        checked_attr = checked_attr,
        name = html_escape(&item.name),
        qty = format_multiplier(item.qty),
        unit = html_escape(&item.unit),
        sources = sources,
        section_select = section_select,
        url = html_escape(&instacart_url),
    ));
}

/// Client-side behavior for the in-store checklist: live check-off, copy link,
/// close/reopen, per-item section changes, and the Instacart cart.
fn trip_page_script(js_trip_id: &str) -> String {
    format!(
        r#"<script>
    const TRIP_ID = '{trip_id}';

    async function toggleTripItem(cb) {{
        const row = cb.closest('.trip-check-row');
        const wasChecked = cb.checked;
        if (wasChecked) row.classList.add('checked'); else row.classList.remove('checked');
        try {{
            const resp = await fetch('/api/shopping/trip/' + encodeURIComponent(TRIP_ID) + '/check', {{
                method: 'POST',
                headers: {{ 'Content-Type': 'application/json' }},
                body: JSON.stringify({{ key: cb.dataset.key, checked: wasChecked }})
            }});
            if (!resp.ok) throw new Error('save failed');
            const data = await resp.json();
            updateProgress(data.done, data.total);
        }} catch (e) {{
            // Roll back the optimistic toggle so the UI matches the server.
            cb.checked = !wasChecked;
            if (cb.checked) row.classList.add('checked'); else row.classList.remove('checked');
            alert('Could not save — check your connection.');
        }}
    }}

    function updateProgress(done, total) {{
        if (done === undefined || total === undefined) return;
        document.getElementById('trip-progress-done').textContent = done;
        document.getElementById('trip-progress-total').textContent = total;
        const bar = document.getElementById('trip-progress-bar');
        if (bar) bar.style.width = (total > 0 ? Math.round(done * 100 / total) : 0) + '%';
    }}

    function copyTripLink(btn) {{
        const url = window.location.origin + window.location.pathname;
        const done = () => {{ const t = btn.textContent; btn.textContent = 'Copied!'; setTimeout(() => btn.textContent = t, 1500); }};
        if (navigator.clipboard && navigator.clipboard.writeText) {{
            navigator.clipboard.writeText(url).then(done, () => prompt('Copy this link:', url));
        }} else {{
            prompt('Copy this link:', url);
        }}
    }}

    async function closeTrip(tripId) {{
        if (!confirm('Done shopping? This closes the trip and removes the banner.')) return;
        try {{
            const resp = await fetch('/api/shopping/trip/' + encodeURIComponent(tripId) + '/close', {{ method: 'POST' }});
            if (!resp.ok) throw new Error('close failed');
            window.location.href = '/shopping';
        }} catch (e) {{ alert('Could not close trip: ' + e.message); }}
    }}

    async function reopenTrip(tripId) {{
        try {{
            const resp = await fetch('/api/shopping/trip/' + encodeURIComponent(tripId) + '/reopen', {{ method: 'POST' }});
            if (!resp.ok) throw new Error('reopen failed');
            window.location.reload();
        }} catch (e) {{ alert('Could not reopen trip: ' + e.message); }}
    }}

    async function changeSection(sel) {{
        try {{
            const resp = await fetch('/api/shopping/section', {{
                method: 'POST',
                headers: {{ 'Content-Type': 'application/json' }},
                body: JSON.stringify({{ name: sel.dataset.name, section: sel.value }})
            }});
            if (!resp.ok) {{ alert('Could not move item.'); return; }}
            window.location.reload();
        }} catch (e) {{ alert('Error: ' + e.message); }}
    }}

    async function openInstacartCart(tripId, btn) {{
        const oldText = btn.textContent;
        btn.disabled = true;
        btn.textContent = 'Opening...';
        try {{
            const resp = await fetch('/api/instacart/trip/' + encodeURIComponent(tripId), {{ method: 'POST' }});
            if (!resp.ok) {{
                const msg = (await resp.text()) || 'Could not create Instacart cart.';
                alert(msg);
                return;
            }}
            const data = await resp.json();
            if (!data.products_link_url) {{ alert('Instacart did not return a cart URL.'); return; }}
            window.open(data.products_link_url, '_blank', 'noopener');
        }} catch (e) {{
            alert('Instacart error: ' + e.message);
        }} finally {{
            btn.disabled = false;
            btn.textContent = oldText;
        }}
    }}
    </script>"#,
        trip_id = js_trip_id,
    )
}

/// Render a published trip as a durable, self-contained, print-friendly page.
/// Everything needed to shop and cook is embedded — it does not depend on the
/// underlying recipes still existing.
pub fn render_published_trip(trip: &PublishedTrip, logged_in: bool) -> String {
    let mut body = String::new();

    body.push_str(r#"<div class="trip-page">"#);

    let date = if trip.created_at.len() >= 10 {
        &trip.created_at[..10]
    } else {
        trip.created_at.as_str()
    };
    body.push_str(&format!(
        r#"<h1>{title}</h1><div class="trip-date">Published {date}</div>"#,
        title = html_escape(&trip.title),
        date = html_escape(date),
    ));

    if !trip.notes_html.is_empty() {
        body.push_str(&format!(
            r#"<div class="recipe-content trip-notes">{}</div>"#,
            trip.notes_html
        ));
    }

    // ---- Shopping list ----
    let need: Vec<&ShoppingItem> = trip.items.iter().filter(|i| !i.in_pantry).collect();
    let have: Vec<&ShoppingItem> = trip.items.iter().filter(|i| i.in_pantry).collect();

    if !need.is_empty() {
        body.push_str(&format!("<h2>Shopping List — Need to Buy ({})</h2>", need.len()));
        body.push_str(r#"<ul class="trip-list">"#);
        for item in &need {
            let instacart_url = instacart_search_url(&item.name);
            let sources = if item.sources.is_empty() {
                String::new()
            } else {
                format!(
                    r#"<div class="trip-item-sources">for: {}</div>"#,
                    html_escape(&item.sources.join(", "))
                )
            };
            body.push_str(&format!(
                r#"<li class="trip-buy-row">
                    <div class="trip-item-copy">
                        <strong>{name}</strong> &middot; {qty} {unit}
                        {sources}
                    </div>
                    <a href="{url}" target="_blank" rel="noopener" class="btn small secondary">Search Item</a>
                </li>"#,
                name = html_escape(&item.name),
                qty = format_multiplier(item.qty),
                unit = html_escape(&item.unit),
                sources = sources,
                url = html_escape(&instacart_url),
            ));
        }
        body.push_str("</ul>");
    }

    if !have.is_empty() {
        body.push_str(&format!(
            r#"<h2 style="color:var(--muted)">Already in Pantry ({})</h2>"#,
            have.len()
        ));
        body.push_str(r#"<ul class="trip-list">"#);
        for item in &have {
            body.push_str(&format!(
                r#"<li style="color:var(--muted)">{} &middot; {} {}</li>"#,
                html_escape(&item.name),
                format_multiplier(item.qty),
                html_escape(&item.unit),
            ));
        }
        body.push_str("</ul>");
    }

    // ---- Recipes (ingredients + prep) ----
    if !trip.cards.is_empty() {
        body.push_str(&format!("<h2>Recipes ({})</h2>", trip.cards.len()));
        for card in &trip.cards {
            body.push_str(r#"<div class="trip-recipe-card">"#);
            let mult = if (card.multiplier - 1.0).abs() < f64::EPSILON {
                String::new()
            } else {
                format!(
                    r#" <span class="trip-recipe-meta">&times;{}</span>"#,
                    format_multiplier(card.multiplier)
                )
            };
            body.push_str(&format!(
                "<h3>{title}{mult}</h3>",
                title = html_escape(&card.title),
                mult = mult,
            ));
            if !card.tags.is_empty() {
                body.push_str(&format!(
                    r#"<div class="trip-recipe-tags">{}</div>"#,
                    html_escape(&card.tags.join(" · "))
                ));
            }
            if !card.ingredients.is_empty() {
                body.push_str(r#"<ul class="ingredient-list">"#);
                for ing in &card.ingredients {
                    body.push_str(&format!(
                        r#"<li><span class="ingredient-qty">{qty}</span><span class="ingredient-unit">{unit}</span>{name}</li>"#,
                        qty = format_multiplier(ing.qty),
                        unit = html_escape(&ing.unit),
                        name = html_escape(&ing.name),
                    ));
                }
                body.push_str("</ul>");
            }
            body.push_str(&format!(
                r#"<div class="recipe-content">{}</div>"#,
                card.body_html
            ));
            body.push_str("</div>");
        }
    }

    body.push_str(
        r#"<div class="trip-page-actions" style="margin-top:1.5rem;display:flex;gap:0.5rem">
            <button class="btn small" onclick="window.print()">Print</button>
            <a href="/shopping" class="btn small secondary">Back to Shopping</a>
        </div>"#,
    );
    body.push_str("</div>");

    base_html(&trip.title, &body, logged_in)
}

fn format_multiplier(value: f64) -> String {
    if value.fract().abs() < f64::EPSILON {
        format!("{}", value as i64)
    } else {
        format!("{:.2}", value)
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}
