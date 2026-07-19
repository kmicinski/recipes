//! Index page: list of all recipes.

use crate::handlers::ReadyInfo;
use crate::mealplan::MealPlan;
use crate::models::Recipe;
use crate::recipes::html_escape;
use crate::templates::base_html;

/// A compact day-by-day strip of the current week's meals, shown when the
/// week's plan is locked in. Links to the full plan page.
fn this_week_strip(plan: &MealPlan) -> String {
    let today = crate::mealplan::today();
    let mut days = String::new();
    for date in plan.week_dates() {
        let meals = plan.meals_on(&date);
        if meals.is_empty() {
            continue;
        }
        let label = chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d")
            .map(|d| d.format("%a %-d").to_string())
            .unwrap_or_else(|_| date.clone());
        let chips: String = meals
            .iter()
            .map(|m| {
                let title = html_escape(&m.title);
                match &m.recipe_key {
                    Some(key) => format!(
                        r#"<a class="week-strip-meal" href="/recipe/{key}">{title}</a>"#,
                        key = html_escape(key),
                        title = title,
                    ),
                    None => format!(r#"<span class="week-strip-meal">{}</span>"#, title),
                }
            })
            .collect();
        days.push_str(&format!(
            r#"<div class="week-strip-day{today_cls}">
                <div class="week-strip-date">{label}</div>
                {chips}
            </div>"#,
            today_cls = if date == today { " week-strip-today" } else { "" },
            label = html_escape(&label),
            chips = chips,
        ));
    }
    format!(
        r#"<div class="week-strip">
            <div class="week-strip-head">
                <h2 class="ready-heading">🍽️ This Week's Meals</h2>
                <a class="week-strip-link" href="/plan/{week}">full plan →</a>
            </div>
            <div class="week-strip-days">{days}</div>
        </div>"#,
        week = html_escape(&plan.week_start),
        days = days,
    )
}

pub fn render_recipe_list(
    recipes: &[Recipe],
    ready_info: &[ReadyInfo],
    this_week: Option<&MealPlan>,
    logged_in: bool,
) -> String {
    let mut html = String::new();

    if let Some(plan) = this_week {
        html.push_str(&this_week_strip(plan));
    }

    // "Ready to Make" and "Almost Ready" sections
    let ready: Vec<&ReadyInfo> = ready_info
        .iter()
        .filter(|r| r.total > 0 && r.have == r.total)
        .collect();
    let almost: Vec<&ReadyInfo> = ready_info
        .iter()
        .filter(|r| {
            let missing = r.total - r.have;
            missing >= 1 && missing <= 2 && r.total > 0
        })
        .collect();

    if !ready.is_empty() {
        html.push_str(r#"<div class="ready-section">"#);
        html.push_str(r#"<h2 class="ready-heading">Ready to Make</h2>"#);
        for info in &ready {
            html.push_str(&format!(
                r#"<div class="ready-item"><a href="/recipe/{key}">{title}</a></div>"#,
                key = html_escape(&info.key),
                title = html_escape(&info.title),
            ));
        }
        html.push_str("</div>");
    }

    if !almost.is_empty() {
        html.push_str(r#"<div class="ready-section">"#);
        html.push_str(r#"<h2 class="ready-heading">Almost Ready</h2>"#);
        for info in &almost {
            let missing_tags: String = info
                .missing
                .iter()
                .map(|m| format!(r#"<span class="missing-tag">{}</span>"#, html_escape(m)))
                .collect();
            html.push_str(&format!(
                r#"<div class="almost-item"><a href="/recipe/{key}">{title}</a> {tags}</div>"#,
                key = html_escape(&info.key),
                title = html_escape(&info.title),
                tags = missing_tags,
            ));
        }
        html.push_str("</div>");
    }

    if recipes.is_empty() {
        html.push_str(r#"<div class="empty-state"><p>No recipes yet.</p>"#);
        if logged_in {
            html.push_str(r#"<p><a href="/new" class="btn">Create your first recipe</a></p>"#);
        }
        html.push_str("</div>");
    } else {
        html.push_str(r#"<ul class="recipe-list">"#);
        for recipe in recipes {
            let tags_html: String = recipe
                .tags
                .iter()
                .map(|t| format!(r#"<span class="tag-badge">{}</span>"#, html_escape(t)))
                .collect();

            html.push_str(&format!(
                r#"<li class="recipe-item">
                    <span class="recipe-main">
                        <a href="/recipe/{key}" class="title">{title}</a>
                        <span class="recipe-tags">{tags}</span>
                    </span>
                    <span class="meta">{modified}</span>
                </li>"#,
                tags = tags_html,
                key = recipe.key,
                title = html_escape(&recipe.title),
                modified = recipe.modified.format("%Y-%m-%d"),
            ));
        }
        html.push_str("</ul>");
    }

    if logged_in {
        html.push_str(r#"<a href="/new" class="fab" title="New recipe">+</a>"#);
    }

    base_html("Recipes", &html, logged_in)
}
