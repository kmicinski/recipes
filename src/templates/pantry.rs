//! Pantry management page template.

use crate::recipes::{html_escape, js_single_quote_attr_escape};
use crate::templates::base_html;

pub fn render_pantry_page(items: &[String], logged_in: bool) -> String {
    let mut html = String::new();

    html.push_str("<h1>Pantry</h1>");
    html.push_str(r#"<p style="color:var(--muted);font-size:0.9rem">Track what you have on hand. Items in your pantry will show as "already have" on shopping lists.</p>"#);

    // Add form
    html.push_str(r#"<div class="pantry-add-form">
        <input type="text" id="pantry-new" placeholder="Add ingredient..." onkeydown="if(event.key==='Enter'){addToPantry(); event.preventDefault();}">
        <button class="btn" onclick="addToPantry()">Add</button>
    </div>"#);

    if items.is_empty() {
        html.push_str(r#"<div class="empty-state"><p>Your pantry is empty.</p></div>"#);
    } else {
        html.push_str(&format!(
            r#"<p style="margin:1rem 0;font-size:0.85rem;color:var(--muted)">{} items in pantry</p>"#,
            items.len()
        ));
        html.push_str(r#"<ul class="pantry-list" id="pantry-list">"#);
        for item in items {
            let js_name = js_single_quote_attr_escape(item);
            html.push_str(&format!(
                r#"<li class="pantry-item" id="pantry-{escaped}">
                    <span class="pantry-item-name">{name}</span>
                    <button class="btn small danger" onclick="removeFromPantry('{js_name}', this)">Remove</button>
                </li>"#,
                escaped = html_escape(item),
                name = html_escape(item),
                js_name = js_name,
            ));
        }
        html.push_str("</ul>");
    }

    html.push_str(
        r#"<script>
    async function addToPantry() {
        const input = document.getElementById('pantry-new');
        const name = input.value.trim();
        if (!name) return;

        try {
            const resp = await fetch('/api/pantry/toggle', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ name: name })
            });
            if (resp.ok) {
                input.value = '';
                location.reload();
            } else {
                alert('Error adding to pantry');
            }
        } catch (e) { alert('Error: ' + e.message); }
    }

    async function removeFromPantry(name, btn) {
        try {
            const resp = await fetch('/api/pantry/toggle', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ name: name })
            });
            if (resp.ok) {
                const data = await resp.json();
                if (!data.in_pantry) {
                    const li = btn.closest('.pantry-item');
                    li.style.transition = 'opacity 0.3s';
                    li.style.opacity = '0';
                    setTimeout(() => li.remove(), 300);
                }
            }
        } catch (e) { alert('Error: ' + e.message); }
    }
    </script>"#,
    );

    base_html("Pantry", &html, logged_in)
}
