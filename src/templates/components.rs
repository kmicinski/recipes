//! Shared HTML components: navigation bar and base HTML template.

use crate::auth::is_auth_enabled;
use crate::recipes::html_escape;

use super::styles::STYLE;

pub fn nav_bar(logged_in: bool) -> String {
    let auth_link = if logged_in {
        r#"<a href="/logout">Logout</a>"#
    } else if is_auth_enabled() {
        r#"<a href="/login">Login</a>"#
    } else {
        ""
    };

    let edit_links = if logged_in {
        r#"<a href="/new">+ New Recipe</a>"#
    } else {
        ""
    };

    format!(
        r#"<nav class="nav-bar">
            <a href="/">Recipes</a>
            <a href="/shopping">Shopping</a>
            <a href="/pantry">Pantry</a>
            <span class="spacer"></span>
            {edit_links}
            {auth_link}
        </nav>"#,
        edit_links = edit_links,
        auth_link = auth_link,
    )
}

pub fn base_html(title: &str, content: &str, logged_in: bool) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{title}</title>
    <style>{STYLE}</style>
</head>
<body>
    {nav}
    <div class="container">
        {content}
    </div>
</body>
</html>"#,
        title = html_escape(title),
        nav = nav_bar(logged_in),
    )
}
