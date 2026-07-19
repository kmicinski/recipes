//! Weekly meal-plan page: a kcal week board of planned meals plus the
//! shopping-trip association panel.
//!
//! The calendar grid is the shared kcal component (see `src/vendor/`,
//! vendored from the mycloud repo's `/srv/apps/shared/kcal/` — edit the
//! canonical copy and run its `sync.sh`, never the vendored files). The page
//! is server-rendered per week; prev/today/next are plain links, and meal
//! edits POST to `/api/plan/*` then reload — same simple model as the rest of
//! the app.

use crate::mealplan::{weekday_name, MealPlan};
use crate::models::Recipe;
use crate::recipes::html_escape;
use crate::shopping::SavedTrip;
use chrono::{NaiveDate, Weekday};

use super::components::base_html;

/// Vendored shared calendar component, inlined since this app serves no
/// static files (everything is baked into the binary).
const KCAL_JS: &str = include_str!("../vendor/kcal.js");
const KCAL_CSS: &str = include_str!("../vendor/kcal.css");

/// Make a JSON blob safe to inline inside a `<script>` element.
fn script_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value)
        .unwrap_or_else(|_| "null".into())
        .replace("</", "<\\/")
}

/// "Jul 13 – Jul 19, 2026" for a week starting on `week_start`.
fn week_label(week_start: &str) -> String {
    let Ok(start) = NaiveDate::parse_from_str(week_start, "%Y-%m-%d") else {
        return week_start.to_string();
    };
    let end = start + chrono::Duration::days(6);
    format!(
        "{} – {}, {}",
        start.format("%b %-d"),
        end.format("%b %-d"),
        end.format("%Y")
    )
}

fn shift_week(week_start: &str, weeks: i64) -> String {
    NaiveDate::parse_from_str(week_start, "%Y-%m-%d")
        .map(|d| (d + chrono::Duration::days(7 * weeks)).format("%Y-%m-%d").to_string())
        .unwrap_or_else(|_| week_start.to_string())
}

/// The shopping panel: the linked trip if there is one, otherwise the two
/// ways to associate one (build from this plan, or link a recent trip).
fn trip_panel_html(plan: &MealPlan, linked: Option<&SavedTrip>, recent: &[SavedTrip]) -> String {
    let mut html = String::new();
    html.push_str(r#"<div class="plan-trip-panel">"#);
    html.push_str(r#"<h2>Shopping for this week</h2>"#);

    match linked {
        Some(trip) => {
            let date = trip.created_at.get(..10).unwrap_or(&trip.created_at);
            let status = if trip.closed {
                "closed".to_string()
            } else {
                format!("{}/{} picked up", trip.buy_done(), trip.buy_total())
            };
            html.push_str(&format!(
                r#"<div class="plan-trip-linked">
                    <a class="plan-trip-link" href="/shopping/trip/{id}">🛒 Trip from {date} · {status} →</a>
                    <button class="btn small secondary" onclick="planUnlinkTrip()">Unlink</button>
                </div>"#,
                id = html_escape(&trip.id),
                date = html_escape(date),
                status = html_escape(&status),
            ));
        }
        None => {
            html.push_str(
                r#"<p class="plan-trip-hint">No shopping trip is associated with this plan yet.</p>
                <button class="btn" onclick="planBuildTrip(this)">🛒 Build shopping trip from this week</button>"#,
            );
            let others: Vec<&SavedTrip> = recent.iter().filter(|t| Some(t.id.as_str()) != plan.trip_id.as_deref()).collect();
            if !others.is_empty() {
                html.push_str(r#"<div class="plan-trip-recent"><div class="plan-trip-hint">…or link a trip you already made:</div>"#);
                for trip in others.iter().take(5) {
                    let date = trip.created_at.get(..10).unwrap_or(&trip.created_at);
                    html.push_str(&format!(
                        r#"<button class="plan-trip-row" onclick="planLinkTrip('{id}')">
                            <span>{date}</span>
                            <span class="plan-trip-meta">{n} item{s}{closed}</span>
                        </button>"#,
                        id = crate::recipes::js_single_quote_attr_escape(&trip.id),
                        date = html_escape(date),
                        n = trip.items.len(),
                        s = if trip.items.len() == 1 { "" } else { "s" },
                        closed = if trip.closed { " · closed" } else { "" },
                    ));
                }
                html.push_str("</div>");
            }
        }
    }
    html.push_str("</div>");
    html
}

/// The "week starts on" selector, current day preselected.
fn week_start_select_html(current: Weekday) -> String {
    let mut html = String::from(
        r#"<select class="plan-week-start-sel" title="First day of the week" onchange="setWeekStart(this.value)">"#,
    );
    for day in [
        Weekday::Mon,
        Weekday::Tue,
        Weekday::Wed,
        Weekday::Thu,
        Weekday::Fri,
        Weekday::Sat,
        Weekday::Sun,
    ] {
        let name = weekday_name(day);
        let mut label: String = name.chars().take(3).collect();
        if let Some(c) = label.get_mut(..1) {
            c.make_ascii_uppercase();
        }
        html.push_str(&format!(
            r#"<option value="{name}"{sel}>Week starts {label}</option>"#,
            name = name,
            sel = if day == current { " selected" } else { "" },
            label = label,
        ));
    }
    html.push_str("</select>");
    html
}

/// The brainstorm-notes panel (draft mode) or the locked confirmation bar.
/// Locked plans keep their notes reachable in a collapsed details block.
fn notes_html(plan: &MealPlan) -> (String, String) {
    let rendered = if plan.notes.trim().is_empty() {
        r#"<p class="plan-notes-empty">Nothing sketched yet — jot ideas for the week here,
           or have Claude scaffold it over MCP (<code>set_plan_notes</code>).</p>"#
            .to_string()
    } else {
        crate::recipes::render_markdown(&plan.notes)
    };

    if plan.locked {
        let bar = r#"<div class="plan-locked-bar">
            <span>✓ Plan locked in — this week's meals are set.</span>
            <button class="btn small secondary" onclick="lockPlan(false)">Unlock</button>
        </div>"#
            .to_string();
        let details = if plan.notes.trim().is_empty() {
            String::new()
        } else {
            format!(
                r#"<details class="plan-notes-details"><summary>Brainstorm notes</summary>
                <div class="plan-notes-body">{rendered}</div></details>"#,
                rendered = rendered,
            )
        };
        (bar, details)
    } else {
        let panel = format!(
            r#"<div class="plan-notes-panel">
    <div class="plan-notes-head">
        <h2>🧠 This week's brainstorm</h2>
        <div class="plan-notes-btns">
            <button class="btn small secondary" onclick="editNotes()">Edit</button>
            <button class="btn small" onclick="lockPlan(true)">✓ Lock in plan</button>
        </div>
    </div>
    <div id="notes-view" class="plan-notes-body">{rendered}</div>
    <div id="notes-editor" hidden>
        <textarea id="notes-text" rows="8" placeholder="Mon: something light… fish twice this week… big batch of chili for leftovers…"></textarea>
        <div class="plan-notes-btns">
            <button class="btn small" onclick="saveNotes(this)">Save</button>
            <button class="btn small secondary" onclick="cancelNotes()">Cancel</button>
        </div>
    </div>
</div>"#,
            rendered = rendered,
        );
        (String::new(), panel)
    }
}

pub fn render_plan_page(
    plan: &MealPlan,
    recipes: &[Recipe],
    linked_trip: Option<&SavedTrip>,
    recent_trips: &[SavedTrip],
    week_start_day: Weekday,
    logged_in: bool,
) -> String {
    // Meals bucketed by date for the calendar.
    let mut by_date: std::collections::BTreeMap<&str, Vec<&crate::mealplan::PlannedMeal>> =
        Default::default();
    for meal in &plan.meals {
        by_date.entry(meal.date.as_str()).or_default().push(meal);
    }
    let meals_json = script_json(&by_date);

    // Recipe picker data, alphabetical.
    let mut picker: Vec<serde_json::Value> = recipes
        .iter()
        .map(|r| serde_json::json!({ "key": r.key, "title": r.title }))
        .collect();
    picker.sort_by(|a, b| {
        a["title"].as_str().unwrap_or("").to_lowercase()
            .cmp(&b["title"].as_str().unwrap_or("").to_lowercase())
    });
    let recipes_json = script_json(&picker);

    let week = html_escape(&plan.week_start);
    let label = html_escape(&week_label(&plan.week_start));
    let prev = shift_week(&plan.week_start, -1);
    let next = shift_week(&plan.week_start, 1);
    let (locked_bar, notes_block) = notes_html(plan);
    let notes_json = script_json(&plan.notes);

    let content = format!(
        r#"<style>{kcal_css}</style>
<div class="plan-page">
    <div class="plan-header">
        <div>
            <h1>Meal Plan</h1>
            <div class="plan-week-label">{label}</div>
        </div>
        <div class="plan-nav">
            <a class="btn small secondary" href="/plan/{prev}">‹</a>
            <a class="btn small secondary" href="/plan">Today</a>
            <a class="btn small secondary" href="/plan/{next}">›</a>
            {week_start_select}
        </div>
    </div>
    {locked_bar}
    {notes_block}
    <div id="plan-cal"></div>
    {trip_panel}
</div>

<div id="meal-picker" class="meal-picker" hidden>
    <div class="meal-picker-card">
        <div class="meal-picker-head">
            <h2 id="meal-picker-title">Add a meal</h2>
            <button class="btn small secondary" onclick="closePicker()">✕</button>
        </div>
        <input id="meal-search" type="text" placeholder="Search recipes…" autocomplete="off">
        <div class="meal-picker-mult">Servings ×
            <input id="meal-mult" type="number" min="0.25" step="0.25" value="1">
        </div>
        <div id="meal-recipe-list" class="meal-recipe-list"></div>
        <div class="meal-picker-custom">
            <input id="meal-custom" type="text" placeholder="…or free text (leftovers, pizza out)">
            <button class="btn small" onclick="addCustomMeal()">Add</button>
        </div>
    </div>
</div>

<script>{kcal_js}</script>
<script>
(function() {{
    var WEEK = '{week}';
    var MEALS = {meals_json};
    var RECIPES = {recipes_json};
    var pickerDate = null;

    function esc(s) {{
        return String(s).replace(/[&<>"']/g, function(c) {{
            return {{'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}}[c];
        }});
    }}

    // Stable pastel hue per meal from its recipe key (or title for free text),
    // so the same dish is the same color week after week.
    function hueOf(meal) {{
        var s = meal.recipe_key || meal.title, h = 5381;
        for (var i = 0; i < s.length; i++) h = ((h << 5) + h + s.charCodeAt(i)) >>> 0;
        return h % 360;
    }}

    function mealChip(meal) {{
        var mult = meal.multiplier && meal.multiplier !== 1
            ? '<span class="meal-mult">×' + esc(meal.multiplier) + '</span>' : '';
        var title = meal.recipe_key
            ? '<a href="/recipe/' + encodeURIComponent(meal.recipe_key) + '">' + esc(meal.title) + '</a>'
            : esc(meal.title);
        return '<div class="meal-chip" style="--chip:hsl(' + hueOf(meal) + ' 45% 45%)">' +
            '<span class="meal-chip-title">' + title + '</span>' + mult +
            '<button class="meal-remove" title="Remove" ' +
            'onclick="removeMeal(\'' + esc(meal.date) + '\',\'' + esc(meal.id) + '\')">×</button></div>';
    }}

    KCal.mount(document.getElementById('plan-cal'), {{
        view: 'week',
        weekStart: {ws_num},
        cursor: WEEK,
        header: false,
        renderChip: mealChip,
        dayFooter: function(date) {{
            return '<button class="meal-add-btn" onclick="openPicker(\'' + esc(date) + '\')">＋ add</button>';
        }},
        onDayClick: function(date) {{ openPicker(date); }},
    }}).setData(MEALS);

    function post(url, body) {{
        return fetch(url, {{
            method: 'POST',
            headers: {{ 'Content-Type': 'application/json' }},
            body: JSON.stringify(body),
        }}).then(function(r) {{
            if (!r.ok) return r.text().then(function(t) {{ throw new Error(t || r.statusText); }});
            return r.json();
        }});
    }}

    window.openPicker = function(date) {{
        pickerDate = date;
        var d = KCal.date.parseISO(date);
        document.getElementById('meal-picker-title').textContent =
            'Add a meal — ' + d.toLocaleDateString('en-US', {{ weekday: 'long', month: 'short', day: 'numeric' }});
        document.getElementById('meal-search').value = '';
        document.getElementById('meal-custom').value = '';
        document.getElementById('meal-mult').value = '1';
        renderRecipeList('');
        document.getElementById('meal-picker').hidden = false;
        document.getElementById('meal-search').focus();
    }};
    window.closePicker = function() {{ document.getElementById('meal-picker').hidden = true; }};

    function renderRecipeList(filter) {{
        var q = filter.trim().toLowerCase();
        var rows = RECIPES.filter(function(r) {{
            return !q || r.title.toLowerCase().indexOf(q) !== -1;
        }}).map(function(r) {{
            return '<button class="meal-recipe-row" onclick="addRecipeMeal(\'' + esc(r.key) + '\')">' +
                esc(r.title) + '</button>';
        }});
        document.getElementById('meal-recipe-list').innerHTML =
            rows.join('') || '<div class="meal-picker-empty">No recipes match.</div>';
    }}
    document.getElementById('meal-search').addEventListener('input', function() {{
        renderRecipeList(this.value);
    }});
    document.getElementById('meal-picker').addEventListener('click', function(ev) {{
        if (ev.target === this) closePicker();
    }});

    function currentMult() {{
        var m = parseFloat(document.getElementById('meal-mult').value);
        return (isFinite(m) && m > 0) ? m : 1;
    }}

    window.addRecipeMeal = function(key) {{
        post('/api/plan/meal', {{ date: pickerDate, recipe_key: key, multiplier: currentMult() }})
            .then(function() {{ location.reload(); }})
            .catch(function(e) {{ alert('Could not add meal: ' + e.message); }});
    }};
    window.addCustomMeal = function() {{
        var title = document.getElementById('meal-custom').value.trim();
        if (!title) return;
        post('/api/plan/meal', {{ date: pickerDate, title: title, multiplier: currentMult() }})
            .then(function() {{ location.reload(); }})
            .catch(function(e) {{ alert('Could not add meal: ' + e.message); }});
    }};
    window.removeMeal = function(date, id) {{
        post('/api/plan/meal/remove', {{ date: date, meal_id: id }})
            .then(function() {{ location.reload(); }})
            .catch(function(e) {{ alert('Could not remove meal: ' + e.message); }});
    }};

    window.planBuildTrip = function(btn) {{
        btn.disabled = true;
        post('/api/plan/trip', {{ week_start: WEEK, action: 'build' }})
            .then(function(d) {{ location.href = '/shopping/trip/' + encodeURIComponent(d.trip_id); }})
            .catch(function(e) {{ btn.disabled = false; alert(e.message); }});
    }};
    window.planLinkTrip = function(tripId) {{
        post('/api/plan/trip', {{ week_start: WEEK, action: 'link', trip_id: tripId }})
            .then(function() {{ location.reload(); }})
            .catch(function(e) {{ alert(e.message); }});
    }};
    window.planUnlinkTrip = function() {{
        post('/api/plan/trip', {{ week_start: WEEK, action: 'unlink' }})
            .then(function() {{ location.reload(); }})
            .catch(function(e) {{ alert(e.message); }});
    }};

    // ---- brainstorm notes / lock / week-start setting ----
    var NOTES = {notes_json};
    window.editNotes = function() {{
        document.getElementById('notes-view').hidden = true;
        var ed = document.getElementById('notes-editor');
        ed.hidden = false;
        var ta = document.getElementById('notes-text');
        ta.value = NOTES;
        ta.focus();
    }};
    window.cancelNotes = function() {{
        document.getElementById('notes-editor').hidden = true;
        document.getElementById('notes-view').hidden = false;
    }};
    window.saveNotes = function(btn) {{
        btn.disabled = true;
        post('/api/plan/notes', {{ week_start: WEEK, notes: document.getElementById('notes-text').value }})
            .then(function() {{ location.reload(); }})
            .catch(function(e) {{ btn.disabled = false; alert('Could not save notes: ' + e.message); }});
    }};
    window.lockPlan = function(locked) {{
        post('/api/plan/lock', {{ week_start: WEEK, locked: locked }})
            .then(function() {{ location.reload(); }})
            .catch(function(e) {{ alert(e.message); }});
    }};
    window.setWeekStart = function(day) {{
        post('/api/plan/week-start', {{ day: day }})
            .then(function() {{ location.href = '/plan'; }})
            .catch(function(e) {{ alert(e.message); location.reload(); }});
    }};
}})();
</script>"#,
        kcal_css = KCAL_CSS,
        kcal_js = KCAL_JS,
        week = week,
        label = label,
        prev = html_escape(&prev),
        next = html_escape(&next),
        week_start_select = week_start_select_html(week_start_day),
        locked_bar = locked_bar,
        notes_block = notes_block,
        notes_json = notes_json,
        ws_num = week_start_day.num_days_from_sunday(),
        meals_json = meals_json,
        recipes_json = recipes_json,
        trip_panel = trip_panel_html(plan, linked_trip, recent_trips),
    );

    base_html("Meal Plan", &content, logged_in)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn week_label_formats_range() {
        assert_eq!(week_label("2026-07-13"), "Jul 13 – Jul 19, 2026");
        assert_eq!(week_label("2025-12-29"), "Dec 29 – Jan 4, 2026");
    }

    #[test]
    fn shift_week_moves_by_whole_weeks() {
        assert_eq!(shift_week("2026-07-13", 1), "2026-07-20");
        assert_eq!(shift_week("2026-07-13", -1), "2026-07-06");
    }

    #[test]
    fn script_json_defuses_closing_tags() {
        let v = serde_json::json!({ "t": "</script><script>alert(1)</script>" });
        let s = script_json(&v);
        assert!(!s.contains("</script>"));
    }
}
