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
            <a href="/plan">Plan</a>
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

/// A persistent "go to active shopping trip" banner. It's empty on the server
/// and filled in by a small client script that polls `/api/trip/active`, so it
/// shows on every page (and reflects live progress) until the trip is closed.
/// It hides itself on the trip's own page.
pub fn active_trip_banner() -> &'static str {
    r#"<div id="active-trip-banner" class="active-trip-banner" hidden></div>
<script>
(function() {
    var el = document.getElementById('active-trip-banner');
    if (!el) return;
    fetch('/api/trip/active', { headers: { 'Accept': 'application/json' } })
        .then(function(r) { return r.ok ? r.json() : null; })
        .then(function(d) {
            if (!d || !d.active) return;
            var path = '/shopping/trip/' + encodeURIComponent(d.id);
            // Don't show the banner while you're already on the trip page.
            if (window.location.pathname === path) return;
            var progress = (d.total > 0) ? (' · ' + d.done + '/' + d.total + ' picked up') : '';
            el.innerHTML = '<a href="' + path + '">🛒 Go to active shopping trip' + progress + ' →</a>';
            el.hidden = false;
        })
        .catch(function() { /* offline: just leave the banner hidden */ });
})();
</script>"#
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
    {banner}
    <div class="container">
        {content}
    </div>
</body>
</html>"#,
        title = html_escape(title),
        nav = nav_bar(logged_in),
        banner = active_trip_banner(),
    )
}
