//! Index page: list of all recipes.

use crate::handlers::ReadyInfo;
use crate::models::Recipe;
use crate::recipes::html_escape;
use crate::templates::base_html;

pub fn render_recipe_list(recipes: &[Recipe], ready_info: &[ReadyInfo], logged_in: bool) -> String {
    let mut html = String::new();

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
                .map(|m| {
                    format!(
                        r#"<span class="missing-tag">{}</span>"#,
                        html_escape(m)
                    )
                })
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
                    <span>
                        {tags}
                        <a href="/recipe/{key}" class="title">{title}</a>
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
