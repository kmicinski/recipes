//! Hidden-book pages: the `/book/{id}` recipe view and the hot-or-not
//! meal-builder deck overlay mounted on the plan page.
//!
//! Book recipes deliberately have no listing page — the deck, plan chips,
//! and trips are the only ways in. The deck is the app's first swipe UI:
//! plain vanilla JS (buttons + arrow keys + a small touch handler), picks
//! serialized client-side so rapid-fire swipes can't race the plan's
//! read-modify-write on the server.

use crate::book::BookRecipe;
use crate::recipes::{html_escape, js_single_quote_attr_escape};
use crate::templates::base_html;

/// A hidden-book recipe page with the "Add to my recipes" promotion button.
pub fn render_book_page(
    b: &BookRecipe,
    promoted_key: Option<&str>,
    from_trip: Option<&str>,
    logged_in: bool,
) -> String {
    let mut html = String::new();

    let (back_href, back_label) = match from_trip {
        Some(trip_id) => (
            format!("/shopping/trip/{}", trip_id),
            "Back to Shopping Trip".to_string(),
        ),
        None => ("/plan".to_string(), "Meal Plan".to_string()),
    };
    html.push_str(&format!(
        r#"<a href="{href}" class="back-link">&larr; {label}</a>"#,
        href = html_escape(&back_href),
        label = html_escape(&back_label),
    ));

    html.push_str(r#"<div class="recipe-header">"#);
    html.push_str(&format!("<h1>{}</h1>", html_escape(&b.title)));
    html.push_str(r#"<div class="mode-toggle">"#);
    match promoted_key {
        Some(key) => {
            html.push_str(&format!(
                r#"<a class="book-promoted-note" href="/recipe/{key}">✓ In your recipes →</a>"#,
                key = html_escape(key),
            ));
        }
        None => {
            html.push_str(&format!(
                r#"<button class="btn" onclick="promoteBook('{id}', this)">＋ Add to my recipes</button>"#,
                id = js_single_quote_attr_escape(&b.id),
            ));
        }
    }
    html.push_str(r#"<button onclick="window.print()">Print</button>"#);
    html.push_str("</div></div>");

    html.push_str(
        r#"<p class="book-page-note">📖 From the recipe book — not in your collection until you add it.</p>"#,
    );

    // Meta block (matches the recipe view's layout language).
    let mut meta_rows = Vec::new();
    if let Some(servings) = b.servings {
        meta_rows.push(format!(
            r#"<div class="meta-row"><span class="meta-label">Servings</span><span>{}</span></div>"#,
            servings
        ));
    }
    let facets: Vec<&str> = [b.protein.as_str(), b.method.as_str(), b.cuisine.as_str()]
        .into_iter()
        .filter(|f| !f.trim().is_empty())
        .collect();
    if !facets.is_empty() {
        meta_rows.push(format!(
            r#"<div class="meta-row"><span class="meta-label">Style</span><span>{}</span></div>"#,
            html_escape(&facets.join(" · ")),
        ));
    }
    if !b.tags.is_empty() {
        let tags_str: String = b
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

    if !b.ingredients.is_empty() {
        html.push_str("<h2>Ingredients</h2>");
        html.push_str(r#"<ul class="ingredient-list">"#);
        for ing in &b.ingredients {
            html.push_str(&format!(
                r#"<li><span class="ingredient-qty">{qty}</span><span class="ingredient-unit">{unit}</span>{name}</li>"#,
                qty = crate::shopping::format_qty(ing.qty),
                unit = html_escape(&ing.unit),
                name = html_escape(&ing.name),
            ));
        }
        html.push_str("</ul>");
    }

    let body_html = crate::recipes::render_markdown(&b.body_markdown);
    if !body_html.is_empty() {
        html.push_str(r#"<div class="recipe-content">"#);
        html.push_str(&body_html);
        html.push_str("</div>");
    }

    html.push_str(
        r#"<script>
    async function promoteBook(id, btn) {
        btn.disabled = true;
        try {
            const resp = await fetch('/api/book/' + encodeURIComponent(id) + '/promote', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' }
            });
            if (resp.ok) {
                const data = await resp.json();
                window.location.href = '/recipe/' + encodeURIComponent(data.key);
            } else if (resp.status === 401) {
                alert('Log in to add recipes to your collection.');
                btn.disabled = false;
            } else {
                alert('Could not add: ' + await resp.text());
                btn.disabled = false;
            }
        } catch (e) { alert('Error: ' + e.message); btn.disabled = false; }
    }
    </script>"#,
    );

    base_html(&b.title, &html, logged_in)
}

/// The full-screen hot-or-not deck overlay (markup only; hidden until the
/// "Build my week" button opens it). Mounted by the plan page.
pub fn deck_overlay_html() -> &'static str {
    r#"<div id="book-deck" class="book-deck" hidden>
    <div class="book-deck-inner">
        <div id="deck-setup">
            <div class="deck-head">
                <h2>📖 Build my week</h2>
                <button class="btn small secondary" onclick="closeDeck()">✕</button>
            </div>
            <p class="deck-hint">Describe the week — flavors, methods, things to use up.
               Every card is a meal-kit-style recipe: prep once on the weekend, ~20 minutes day-of.
               Use <code>-word</code> to steer away from something.</p>
            <textarea id="deck-prompt" rows="3"
                placeholder="grill-heavy, a couple mexican nights, use up the cabbage, -mushrooms"></textarea>
            <div class="deck-meal-counts">
                <label class="deck-count-label">Breakfasts to prep
                    <select id="deck-breakfast-count"><option selected>0</option><option>1</option><option>2</option><option>3</option><option>4</option><option>5</option><option>6</option><option>7</option></select>
                </label>
                <label class="deck-count-label">Lunches to prep
                    <select id="deck-lunch-count"><option>0</option><option>1</option><option selected>2</option><option>3</option><option>4</option><option>5</option><option>6</option><option>7</option></select>
                </label>
                <label class="deck-count-label">Dinners to prep
                    <select id="deck-dinner-count"><option>0</option><option>1</option><option>2</option><option>3</option><option>4</option><option selected>5</option><option>6</option><option>7</option></select>
                </label>
            </div>
            <div class="deck-setup-row">
                <button class="btn" onclick="dealDeck(this)">Deal the deck →</button>
            </div>
        </div>
        <div id="deck-play" hidden>
            <div class="deck-head">
                <div class="deck-progress" id="deck-progress"></div>
                <button class="btn small secondary" onclick="closeDeck()">✕</button>
            </div>
            <div id="deck-card-zone" class="deck-card-zone"></div>
            <div class="deck-actions">
                <button class="deck-btn deck-btn-not" onclick="deckNot()" title="Left arrow">✕<span> Not</span></button>
                <button class="deck-btn deck-btn-hot" onclick="deckHot()" title="Right arrow">♥<span> Hot</span></button>
            </div>
            <div class="deck-foot" id="deck-picked-line"></div>
        </div>
        <div id="deck-done" hidden>
            <div class="deck-head">
                <h2>🎉 Week built</h2>
                <button class="btn small secondary" onclick="closeDeck()">✕</button>
            </div>
            <div id="deck-summary" class="deck-summary"></div>
            <div class="deck-setup-row">
                <button class="btn" onclick="location.reload()">Done — review the week</button>
                <button class="btn small secondary" id="deck-more-btn" onclick="deckKeepBrowsing()">Keep browsing</button>
            </div>
            <div class="deck-foot">
                <button class="deck-link" onclick="deckReshuffle()">↻ Forget my skips and re-deal</button>
            </div>
        </div>
    </div>
</div>"#
}

/// The deck's client logic. `week_start` is the plan page's canonical week.
pub fn deck_script(week_start: &str) -> String {
    format!(
        r#"<script>
(function() {{
    var DECK_WEEK = '{week}';
    var cands = [], idx = 0, picked = [], target = 0, inflight = false;
    var mealTypes = [], typeIndex = 0, currentType = 'dinner', typePicked = 0;
    var selectedDate = null;

    function desc(s) {{
        return String(s).replace(/[&<>"']/g, function(c) {{
            return {{'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}}[c];
        }});
    }}
    function dpost(url, body) {{
        return fetch(url, {{
            method: 'POST',
            headers: {{ 'Content-Type': 'application/json' }},
            body: JSON.stringify(body),
        }}).then(function(r) {{
            if (!r.ok) return r.text().then(function(t) {{ throw new Error(t || r.statusText); }});
            return r.json();
        }});
    }}
    function show(id) {{
        ['deck-setup', 'deck-play', 'deck-done'].forEach(function(x) {{
            document.getElementById(x).hidden = (x !== id);
        }});
    }}

    window.openDeck = function() {{
        selectedDate = null;
        document.getElementById('book-deck').hidden = false;
        show('deck-setup');
        document.getElementById('deck-prompt').focus();
    }};
    window.openDeckFor = function(date, mealType) {{
        selectedDate = date;
        picked = [];
        mealTypes = [{{ kind: mealType, count: 1 }}];
        typeIndex = 0;
        target = 1;
        document.getElementById('book-deck').hidden = false;
        dealType(null);
    }};
    window.closeDeck = function() {{
        document.getElementById('book-deck').hidden = true;
        if (picked.length > 0) location.reload();
    }};

    window.dealDeck = function(btn) {{
        if (btn) btn.disabled = true;
        mealTypes = ['breakfast', 'lunch', 'dinner'].map(function(kind) {{
            return {{ kind: kind, count: parseInt(document.getElementById('deck-' + kind + '-count').value, 10) || 0 }};
        }}).filter(function(x) {{ return x.count > 0; }});
        target = mealTypes.reduce(function(n, x) {{ return n + x.count; }}, 0);
        if (!target) {{ if (btn) btn.disabled = false; alert('Choose at least one meal to prep.'); return; }}
        picked = []; typeIndex = 0;
        dealType(btn);
    }};

    function dealType(btn) {{
        currentType = mealTypes[typeIndex].kind;
        typePicked = 0;
        var prompt = document.getElementById('deck-prompt').value;
        prompt = currentType + (prompt.trim() ? ' ' + prompt : '');
        dpost('/api/book/candidates', {{
            week_start: DECK_WEEK,
            prompt: prompt,
            limit: 150,
        }}).then(function(d) {{
            if (btn) btn.disabled = false;
            cands = d.candidates || [];
            idx = 0;
            if (cands.length === 0) {{
                alert('No ' + currentType + ' recipes match — try a broader prompt, or re-deal after forgetting skips.');
                return;
            }}
            show('deck-play');
            renderCard();
        }}).catch(function(e) {{ if (btn) btn.disabled = false; alert(e.message); }});
    }};

    function currentCard() {{ return idx < cands.length ? cands[idx] : null; }}

    function renderCard() {{
        var c = currentCard();
        if (!c || typePicked >= mealTypes[typeIndex].count) {{ nextType(); return; }}
        var facets = [c.protein, c.method, c.cuisine].filter(Boolean).join(' · ');
        var tags = (c.tags || []).map(function(t) {{
            return '<span class="tag-badge">' + desc(t) + '</span>';
        }}).join('');
        var ings = (c.ingredients || []).map(function(i) {{ return desc(i.name); }}).join(', ');
        document.getElementById('deck-card-zone').innerHTML =
            '<div class="book-deck-card" id="deck-card">' +
            '<div class="deck-card-title">' + desc(c.title) + '</div>' +
            (facets ? '<div class="deck-card-facets">' + desc(facets) + '</div>' : '') +
            (tags ? '<div class="deck-card-tags">' + tags + '</div>' : '') +
            '<div class="deck-card-ings">' + ings + '</div>' +
            '<a class="deck-card-view" href="/book/' + encodeURIComponent(c.id) + '" target="_blank" rel="noopener">full recipe ↗</a>' +
            '</div>';
        attachSwipe(document.getElementById('deck-card'));
        updateProgress();
    }}
    function updateProgress() {{
        document.getElementById('deck-progress').textContent =
            currentType.charAt(0).toUpperCase() + currentType.slice(1) + ': ' + typePicked +
            ' of ' + mealTypes[typeIndex].count + ' · ' + picked.length + ' of ' + target + ' total';
    }}
    function dayName(dateISO) {{
        return new Date(dateISO + 'T12:00:00').toLocaleDateString('en-US', {{ weekday: 'long' }});
    }}
    function setPickedLine(text) {{
        document.getElementById('deck-picked-line').textContent = text;
    }}

    function advance() {{
        idx++;
        if (typePicked >= mealTypes[typeIndex].count || idx >= cands.length) nextType();
        else renderCard();
    }}
    function nextType() {{
        typeIndex++;
        if (typeIndex >= mealTypes.length) finish();
        else dealType(null);
    }}
    function finish() {{
        show('deck-done');
        var rows = picked.map(function(p) {{
            return '<div class="deck-summary-row"><span class="deck-summary-day">' +
                desc(p.day) + '</span> <span class="meal-kind">' + desc(p.kind) + '</span> ' + desc(p.title) + '</div>';
        }}).join('');
        document.getElementById('deck-summary').innerHTML =
            rows || '<p class="deck-hint">Nothing picked this round.</p>';
        document.getElementById('deck-more-btn').hidden = (idx >= cands.length);
    }}

    // Picks are serialized: a pick in flight ignores further input, so two
    // rapid Hots can't race the plan's read-modify-write server-side.
    window.deckHot = function() {{
        var c = currentCard();
        if (!c || inflight) return;
        inflight = true;
        animateCard(1);
        dpost('/api/book/pick', {{ week_start: DECK_WEEK, book_id: c.id, meal_type: currentType, date: selectedDate }})
            .then(function(d) {{
                inflight = false;
                var day = dayName(d.date);
                picked.push({{ title: c.title, day: day, kind: currentType }});
                typePicked++;
                setPickedLine('♥ ' + c.title + ' → ' + currentType + ' on ' + day);
                advance();
            }})
            .catch(function(e) {{ inflight = false; alert(e.message); renderCard(); }});
    }};
    window.deckNot = function() {{
        var c = currentCard();
        if (!c || inflight) return;
        animateCard(-1);
        // Skips are idempotent; fire-and-forget keeps the deck snappy.
        dpost('/api/book/skip', {{ week_start: DECK_WEEK, book_id: c.id }}).catch(function() {{}});
        advance();
    }};
    window.deckKeepBrowsing = function() {{
        mealTypes.push({{ kind: currentType, count: 1 }});
        typeIndex = mealTypes.length - 1; typePicked = 0; target++;
        show('deck-play');
        renderCard();
    }};
    window.deckReshuffle = function() {{
        dpost('/api/book/skips/clear', {{ week_start: DECK_WEEK }})
            .then(function() {{ show('deck-setup'); dealDeck(null); }})
            .catch(function(e) {{ alert(e.message); }});
    }};

    function animateCard(dir) {{
        var el = document.getElementById('deck-card');
        if (!el) return;
        el.style.transition = 'transform 0.18s ease-out, opacity 0.18s ease-out';
        el.style.transform = 'translateX(' + (dir * 120) + '%) rotate(' + (dir * 12) + 'deg)';
        el.style.opacity = '0';
    }}

    // Keyboard: ← Not, → Hot, Esc close (only while the deck is open).
    document.addEventListener('keydown', function(ev) {{
        if (document.getElementById('book-deck').hidden) return;
        if (document.getElementById('deck-play').hidden) {{
            if (ev.key === 'Escape') closeDeck();
            return;
        }}
        if (ev.key === 'ArrowLeft') {{ ev.preventDefault(); deckNot(); }}
        else if (ev.key === 'ArrowRight') {{ ev.preventDefault(); deckHot(); }}
        else if (ev.key === 'Escape') closeDeck();
    }});

    // Touch swipe on the card: drag horizontally, release past 70px to commit.
    function attachSwipe(el) {{
        if (!el) return;
        var startX = null;
        el.addEventListener('touchstart', function(ev) {{
            if (ev.touches.length === 1) startX = ev.touches[0].clientX;
        }}, {{ passive: true }});
        el.addEventListener('touchmove', function(ev) {{
            if (startX === null) return;
            var dx = ev.touches[0].clientX - startX;
            el.style.transform = 'translateX(' + dx + 'px) rotate(' + (dx / 20) + 'deg)';
        }}, {{ passive: true }});
        el.addEventListener('touchend', function(ev) {{
            if (startX === null) return;
            var dx = ev.changedTouches[0].clientX - startX;
            startX = null;
            if (dx > 70) deckHot();
            else if (dx < -70) deckNot();
            else {{
                el.style.transition = 'transform 0.15s ease-out';
                el.style.transform = '';
            }}
        }});
    }}

    document.getElementById('book-deck').addEventListener('click', function(ev) {{
        if (ev.target === this) closeDeck();
    }});
}})();
</script>"#,
        week = crate::recipes::js_single_quote_escape(week_start),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Ingredient;

    fn sample() -> BookRecipe {
        BookRecipe {
            id: "bk-0001".into(),
            title: "Grilled Chicken Bowls".into(),
            servings: Some(4),
            tags: vec!["grill".into(), "bowl".into()],
            ingredients: vec![Ingredient {
                name: "chicken thighs".into(),
                qty: 1.5,
                unit: "lb".into(),
            }],
            body_markdown: "## Prep ahead\nMarinate.\n\n## Day of (~20 min)\nGrill.".into(),
            protein: "chicken".into(),
            method: "grill".into(),
            cuisine: "greek".into(),
        }
    }

    #[test]
    fn book_page_offers_promotion_when_not_promoted() {
        let html = render_book_page(&sample(), None, None, true);
        assert!(html.contains("Add to my recipes"));
        assert!(html.contains("From the recipe book"));
        assert!(html.contains("chicken · grill · greek"));
        assert!(html.contains("Day of (~20 min)"));
        assert!(!html.contains("In your recipes"));
    }

    #[test]
    fn book_page_links_promoted_recipe() {
        let html = render_book_page(&sample(), Some("abc123"), None, true);
        assert!(html.contains(r#"href="/recipe/abc123""#));
        assert!(html.contains("In your recipes"));
        assert!(!html.contains("Add to my recipes"));
    }

    #[test]
    fn deck_overlay_has_required_elements() {
        let html = deck_overlay_html();
        for id in [
            "book-deck", "deck-setup", "deck-play", "deck-done",
            "deck-prompt", "deck-breakfast-count", "deck-lunch-count", "deck-dinner-count", "deck-card-zone", "deck-progress",
        ] {
            assert!(html.contains(&format!(r#"id="{}""#, id)), "missing #{}", id);
        }
    }

    #[test]
    fn deck_script_embeds_week() {
        let js = deck_script("2026-08-24");
        assert!(js.contains("var DECK_WEEK = '2026-08-24'"));
        assert!(js.contains("/api/book/candidates"));
        assert!(js.contains("/api/book/pick"));
    }
}
