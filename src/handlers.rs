//! HTTP route handlers for the recipes application.

use crate::auth::{create_session, is_logged_in, SESSION_COOKIE, SESSION_TTL_HOURS};
use crate::models::{Ingredient, Recipe, RecipeSelection};
use crate::recipes::{generate_key, git_commit, git_rm_commit, serialize_recipe};
use crate::templates::{base_html, STYLE};
use crate::validate_path_within;
use crate::{instacart, pantry, shopping, AppState};
use axum::{
    extract::{Path, Query, State},
    http::{header::SET_COOKIE, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
};
use axum_extra::extract::CookieJar;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::sync::Arc;
use subtle::ConstantTimeEq;

/// View-only struct for homepage "Ready to Make" / "Almost Ready" sections.
pub struct ReadyInfo {
    pub key: String,
    pub title: String,
    pub total: usize,
    pub have: usize,
    pub missing: Vec<String>,
}

fn compute_ready_info(recipes: &[Recipe], pantry_items: &HashSet<String>) -> Vec<ReadyInfo> {
    recipes
        .iter()
        .filter(|r| !r.ingredients.is_empty())
        .map(|r| {
            let total = r.ingredients.len();
            let mut have = 0;
            let mut missing = Vec::new();
            for ing in &r.ingredients {
                let norm = ing.name.trim().to_lowercase();
                if pantry_items.contains(&norm) {
                    have += 1;
                } else {
                    missing.push(ing.name.clone());
                }
            }
            ReadyInfo {
                key: r.key.clone(),
                title: r.title.clone(),
                total,
                have,
                missing,
            }
        })
        .collect()
}

// ============================================================================
// Index
// ============================================================================

pub async fn index(State(state): State<Arc<AppState>>, jar: CookieJar) -> Html<String> {
    let logged_in = is_logged_in(&jar);
    let recipes = state.load_recipes();
    let pantry_items: HashSet<String> = pantry::list(&state.db).into_iter().collect();
    let ready_info = compute_ready_info(&recipes, &pantry_items);
    Html(crate::templates::recipe_list::render_recipe_list(
        &recipes,
        &ready_info,
        logged_in,
    ))
}

// ============================================================================
// Recipe View
// ============================================================================

#[derive(Deserialize, Default)]
pub struct RecipeViewQuery {
    pub from_trip: Option<String>,
}

pub async fn view_recipe(
    Path(key): Path<String>,
    Query(query): Query<RecipeViewQuery>,
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> Response {
    let logged_in = is_logged_in(&jar);
    let recipes = state.load_recipes();

    match recipes.into_iter().find(|r| r.key == key) {
        Some(recipe) => {
            let pantry_items: HashSet<String> = pantry::list(&state.db).into_iter().collect();
            let (back_href, back_label) = match query.from_trip {
                Some(trip_id) => (
                    format!("/shopping/trip/{}", trip_id),
                    "Back to Shopping Trip".to_string(),
                ),
                None => ("/".to_string(), "All recipes".to_string()),
            };
            Html(crate::templates::recipe_view::render_recipe_view(
                &recipe,
                &pantry_items,
                logged_in,
                &back_href,
                &back_label,
            ))
            .into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Html(base_html(
                "Not Found",
                "<h1>Recipe not found</h1>",
                logged_in,
            )),
        )
            .into_response(),
    }
}

// ============================================================================
// Recipe Edit
// ============================================================================

pub async fn edit_recipe(
    Path(key): Path<String>,
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> Response {
    let logged_in = is_logged_in(&jar);
    if !logged_in {
        return Redirect::to("/login").into_response();
    }

    let recipes = state.load_recipes();
    match recipes.into_iter().find(|r| r.key == key) {
        Some(recipe) => Html(crate::templates::recipe_edit::render_recipe_editor(Some(
            &recipe,
        )))
        .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Html(base_html(
                "Not Found",
                "<h1>Recipe not found</h1>",
                logged_in,
            )),
        )
            .into_response(),
    }
}

// ============================================================================
// New Recipe
// ============================================================================

pub async fn new_recipe_page(jar: CookieJar) -> Response {
    let logged_in = is_logged_in(&jar);
    if !logged_in {
        return Redirect::to("/login").into_response();
    }
    Html(crate::templates::recipe_edit::render_recipe_editor(None)).into_response()
}

#[derive(Deserialize)]
pub struct RecipeData {
    pub title: String,
    pub servings: Option<u32>,
    pub tags: Vec<String>,
    pub ingredients: Vec<Ingredient>,
    pub instructions: String,
}

pub async fn create_recipe_api(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    axum::Json(data): axum::Json<RecipeData>,
) -> Response {
    if !is_logged_in(&jar) {
        return (StatusCode::UNAUTHORIZED, "Not logged in").into_response();
    }

    let title = if data.title.is_empty() {
        "Untitled".to_string()
    } else {
        data.title.clone()
    };
    let path = unique_recipe_path(&state.content_dir, &title);
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| format!("{}.md", slugify(&title)));

    if let Err(e) = validate_path_within(&state.content_dir, &path) {
        return (StatusCode::BAD_REQUEST, e).into_response();
    }

    let content = serialize_recipe(
        &title,
        data.servings,
        &data.tags,
        &data.ingredients,
        &data.instructions,
    );

    if let Err(e) = fs::write(&path, &content) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Write error: {}", e),
        )
            .into_response();
    }

    let content_dir = state.content_dir.clone();
    let commit_path = path.clone();
    let commit_title = title.clone();
    tokio::task::spawn_blocking(move || {
        git_commit(
            &content_dir,
            &commit_path,
            &format!("Add recipe: {}", commit_title),
        );
    });

    let key = generate_key(&std::path::PathBuf::from(&filename));
    axum::Json(serde_json::json!({ "key": key })).into_response()
}

// ============================================================================
// Save Recipe (API)
// ============================================================================

pub async fn save_recipe_api(
    Path(key): Path<String>,
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    axum::Json(data): axum::Json<RecipeData>,
) -> Response {
    if !is_logged_in(&jar) {
        return (StatusCode::UNAUTHORIZED, "Not logged in").into_response();
    }

    let recipes = state.load_recipes();
    let recipe = match recipes.into_iter().find(|r| r.key == key) {
        Some(r) => r,
        None => return (StatusCode::NOT_FOUND, "Recipe not found").into_response(),
    };

    if let Err(e) = validate_path_within(&state.content_dir, &recipe.path) {
        return (StatusCode::BAD_REQUEST, e).into_response();
    }

    let title = if data.title.is_empty() {
        recipe.title.clone()
    } else {
        data.title.clone()
    };
    let content = serialize_recipe(
        &title,
        data.servings,
        &data.tags,
        &data.ingredients,
        &data.instructions,
    );

    if let Err(e) = fs::write(&recipe.path, &content) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Write error: {}", e),
        )
            .into_response();
    }

    let content_dir = state.content_dir.clone();
    let commit_path = recipe.path.clone();
    let commit_title = title.clone();
    tokio::task::spawn_blocking(move || {
        git_commit(
            &content_dir,
            &commit_path,
            &format!("Update recipe: {}", commit_title),
        );
    });

    axum::Json(serde_json::json!({ "ok": true })).into_response()
}

// ============================================================================
// Delete Recipe (API)
// ============================================================================

pub async fn delete_recipe(
    Path(key): Path<String>,
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> Response {
    if !is_logged_in(&jar) {
        return (StatusCode::UNAUTHORIZED, "Not logged in").into_response();
    }

    let recipes = state.load_recipes();
    let recipe = match recipes.into_iter().find(|r| r.key == key) {
        Some(r) => r,
        None => return (StatusCode::NOT_FOUND, "Recipe not found").into_response(),
    };

    if let Err(e) = validate_path_within(&state.content_dir, &recipe.path) {
        return (StatusCode::BAD_REQUEST, e).into_response();
    }

    let content_dir = state.content_dir.clone();
    let commit_path = recipe.path.clone();
    let commit_title = recipe.title.clone();
    tokio::task::spawn_blocking(move || {
        git_rm_commit(
            &content_dir,
            &commit_path,
            &format!("Delete recipe: {}", commit_title),
        );
    });

    (StatusCode::OK, "Deleted").into_response()
}

// ============================================================================
// Shopping
// ============================================================================

pub async fn shopping_page(State(state): State<Arc<AppState>>, jar: CookieJar) -> Html<String> {
    let logged_in = is_logged_in(&jar);
    let recipes = state.load_recipes();
    let recent_trips = shopping::list_trips(&state.db);
    Html(crate::templates::shopping::render_shopping_page(
        &recipes,
        &recent_trips,
        logged_in,
    ))
}

#[derive(Deserialize)]
pub struct ShoppingBuildRequest {
    pub selections: Vec<RecipeSelection>,
}

pub async fn shopping_build(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<ShoppingBuildRequest>,
) -> Html<String> {
    let recipes = state.load_recipes();
    let items = shopping::build_shopping_list(&body.selections, &recipes, &state.db);
    Html(crate::templates::shopping::render_shopping_results(&items))
}

#[derive(Deserialize)]
pub struct ShoppingToPantryRequest {
    pub names: Vec<String>,
}

pub async fn shopping_to_pantry(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<ShoppingToPantryRequest>,
) -> Response {
    pantry::bulk_add(&state.db, &body.names).ok();
    (StatusCode::OK, "OK").into_response()
}

// ============================================================================
// Shopping Trips
// ============================================================================

#[derive(Deserialize)]
pub struct SaveTripRequest {
    pub items: Vec<crate::models::ShoppingItem>,
    #[serde(default)]
    pub selections: Vec<RecipeSelection>,
}

pub async fn save_trip_handler(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<SaveTripRequest>,
) -> Response {
    let recipes = state.load_recipes();
    let trip_recipes = shopping::resolve_trip_recipes(&body.selections, &recipes);
    match shopping::save_trip(&state.db, &body.items, &trip_recipes) {
        Ok(id) => axum::Json(serde_json::json!({ "id": id })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

pub async fn view_trip_handler(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> Response {
    let logged_in = is_logged_in(&jar);
    match shopping::load_trip(&state.db, &id) {
        Some(trip) => Html(crate::templates::shopping::render_trip_page(
            &trip, logged_in,
        ))
        .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Html(base_html("Not Found", "<h1>Trip not found</h1>", logged_in)),
        )
            .into_response(),
    }
}

#[derive(Serialize)]
pub struct InstacartTripLinkResponse {
    pub products_link_url: String,
    pub cached: bool,
}

pub async fn instacart_trip_link_handler(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let mut trip = match shopping::load_trip(&state.db, &id) {
        Some(t) => t,
        None => return (StatusCode::NOT_FOUND, "Trip not found").into_response(),
    };

    let fingerprint = instacart::trip_payload_fingerprint(&trip);
    if let (Some(url), Some(cached_fp)) = (
        trip.instacart_products_link_url.clone(),
        trip.instacart_products_link_fingerprint.as_deref(),
    ) {
        if cached_fp == fingerprint {
            return axum::Json(InstacartTripLinkResponse {
                products_link_url: url,
                cached: true,
            })
            .into_response();
        }
    }

    match instacart::create_products_link_for_trip(&trip).await {
        Ok(url) => {
            trip.instacart_products_link_url = Some(url.clone());
            trip.instacart_products_link_fingerprint = Some(fingerprint);
            if let Err(e) = shopping::save_trip_record(&state.db, &trip) {
                return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
            }
            axum::Json(InstacartTripLinkResponse {
                products_link_url: url,
                cached: false,
            })
            .into_response()
        }
        Err(err) => {
            let status = if err.is_not_configured() {
                StatusCode::SERVICE_UNAVAILABLE
            } else if matches!(err, instacart::InstacartError::InvalidTrip(_)) {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::BAD_GATEWAY
            };
            (status, err.as_message()).into_response()
        }
    }
}

// ============================================================================
// Pantry
// ============================================================================

pub async fn pantry_page(State(state): State<Arc<AppState>>, jar: CookieJar) -> Html<String> {
    let logged_in = is_logged_in(&jar);
    let items = pantry::list(&state.db);
    Html(crate::templates::pantry::render_pantry_page(
        &items, logged_in,
    ))
}

#[derive(Deserialize)]
pub struct PantryToggleRequest {
    pub name: String,
}

pub async fn pantry_toggle(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<PantryToggleRequest>,
) -> Response {
    match pantry::toggle(&state.db, &body.name) {
        Ok(in_pantry) => axum::Json(serde_json::json!({ "in_pantry": in_pantry })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

#[derive(Deserialize)]
pub struct PantryBulkRequest {
    pub names: Vec<String>,
}

pub async fn pantry_bulk_add(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<PantryBulkRequest>,
) -> Response {
    match pantry::bulk_add(&state.db, &body.names) {
        Ok(()) => (StatusCode::OK, "OK").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

pub async fn pantry_bulk_remove(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<PantryBulkRequest>,
) -> Response {
    match pantry::bulk_remove(&state.db, &body.names) {
        Ok(()) => (StatusCode::OK, "OK").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

// ============================================================================
// Auth
// ============================================================================

pub async fn login_page(jar: CookieJar) -> Response {
    if is_logged_in(&jar) {
        return Redirect::to("/").into_response();
    }
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Login - Recipes</title>
    <style>{}</style>
</head>
<body>
    <div class="login-form">
        <h1>Recipes</h1>
        <form method="post" action="/login">
            <input type="password" name="password" placeholder="Password" autofocus>
            <button type="submit">Login</button>
        </form>
    </div>
</body>
</html>"#,
        STYLE
    );
    Html(html).into_response()
}

#[derive(Deserialize)]
pub struct LoginForm {
    pub password: String,
}

pub async fn login_submit(axum::Form(form): axum::Form<LoginForm>) -> Response {
    let secret = match crate::auth::get_secret_key() {
        Some(s) => s,
        None => return Redirect::to("/").into_response(),
    };

    let input_bytes = form.password.as_bytes();
    if input_bytes.len() != secret.len() || input_bytes.ct_eq(&secret).unwrap_u8() != 1 {
        let html = format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Login - Recipes</title>
    <style>{}</style>
</head>
<body>
    <div class="login-form">
        <h1>Recipes</h1>
        <div class="message error">Invalid password</div>
        <form method="post" action="/login">
            <input type="password" name="password" placeholder="Password" autofocus>
            <button type="submit">Login</button>
        </form>
    </div>
</body>
</html>"#,
            STYLE
        );
        return Html(html).into_response();
    }

    match create_session() {
        Some(token) => {
            let cookie = format!(
                "{}={}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}",
                SESSION_COOKIE,
                token,
                SESSION_TTL_HOURS * 3600
            );
            let mut headers = HeaderMap::new();
            headers.insert(SET_COOKIE, cookie.parse().unwrap());
            (headers, Redirect::to("/")).into_response()
        }
        None => (StatusCode::INTERNAL_SERVER_ERROR, "Session creation failed").into_response(),
    }
}

pub async fn logout() -> Response {
    let cookie = format!(
        "{}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
        SESSION_COOKIE
    );
    let mut headers = HeaderMap::new();
    headers.insert(SET_COOKIE, cookie.parse().unwrap());
    (headers, Redirect::to("/")).into_response()
}

// ============================================================================
// Helpers
// ============================================================================

/// Simple slug generation from a title.
fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<&str>>()
        .join("-")
}

fn unique_recipe_path(content_dir: &std::path::Path, title: &str) -> std::path::PathBuf {
    let base_slug = {
        let s = slugify(title);
        if s.is_empty() {
            "untitled".to_string()
        } else {
            s
        }
    };

    for suffix in 0..1000 {
        let filename = if suffix == 0 {
            format!("{}.md", base_slug)
        } else {
            format!("{}-{}.md", base_slug, suffix)
        };
        let candidate = content_dir.join(filename);
        if !candidate.exists() {
            return candidate;
        }
    }

    content_dir.join(format!(
        "{}-{}.md",
        base_slug,
        chrono::Utc::now().timestamp()
    ))
}

#[cfg(test)]
mod tests {
    use super::{slugify, unique_recipe_path};

    #[test]
    fn test_slugify_empty_title() {
        assert_eq!(slugify("   "), "");
    }

    #[test]
    fn test_unique_recipe_path_adds_suffix_when_needed() {
        let dir = tempfile::tempdir().unwrap();
        let first = unique_recipe_path(dir.path(), "Tea");
        std::fs::write(&first, "hello").unwrap();

        let second = unique_recipe_path(dir.path(), "Tea");
        let first_name = first.file_name().unwrap().to_string_lossy().to_string();
        let second_name = second.file_name().unwrap().to_string_lossy().to_string();

        assert_eq!(first_name, "tea.md");
        assert_eq!(second_name, "tea-1.md");
    }
}
