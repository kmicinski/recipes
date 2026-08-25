//! HTTP route handlers for the recipes application.

use crate::auth::{create_session, is_logged_in, SESSION_COOKIE, SESSION_TTL_HOURS};
use crate::models::{Ingredient, Recipe, RecipeSelection};
use crate::recipes::{generate_key, git_commit, git_rm_commit, serialize_recipe, slugify, unique_recipe_path};
use crate::templates::{base_html, STYLE};
use crate::validate_path_within;
use crate::{book, instacart, mealplan, pantry, shopping, AppState};
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
    // A locked-in plan for the current week gets a "this week's meals" strip.
    let this_week = mealplan::week_of(&state.db, &mealplan::today())
        .ok()
        .map(|week| mealplan::load_plan(&state.db, &week))
        .filter(|p| p.locked && !p.meals.is_empty());
    Html(crate::templates::recipe_list::render_recipe_list(
        &recipes,
        &ready_info,
        this_week.as_ref(),
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
    // Book keys (from e.g. a trip's recipe links) live at /book/{id}.
    if key.starts_with(book::BOOK_ID_PREFIX) {
        let target = match &query.from_trip {
            Some(trip_id) => format!("/book/{}?from_trip={}", key, trip_id),
            None => format!("/book/{}", key),
        };
        return Redirect::to(&target).into_response();
    }
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
        Ok(id) => {
            // A freshly saved trip becomes the active trip, so the "go to active
            // trip" banner appears until shopping is closed out.
            shopping::set_active_trip(&state.db, &id).ok();
            axum::Json(serde_json::json!({ "id": id })).into_response()
        }
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
            &state.db, &trip, logged_in,
        ))
        .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Html(base_html("Not Found", "<h1>Trip not found</h1>", logged_in)),
        )
            .into_response(),
    }
}

/// JSON summary of the current active trip, polled by the header banner on
/// every page. Returns `{ "active": false }` when there is none.
pub async fn active_trip_handler(State(state): State<Arc<AppState>>) -> Response {
    match shopping::active_trip(&state.db) {
        Some(trip) => axum::Json(serde_json::json!({
            "active": true,
            "id": trip.id,
            "done": trip.buy_done(),
            "total": trip.buy_total(),
        }))
        .into_response(),
        None => axum::Json(serde_json::json!({ "active": false })).into_response(),
    }
}

#[derive(Deserialize)]
pub struct TripCheckRequest {
    pub key: String,
    pub checked: bool,
}

/// Check or uncheck one item on a trip. Persists immediately so progress
/// survives a page refresh.
pub async fn trip_check_handler(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<TripCheckRequest>,
) -> Response {
    match shopping::set_item_checked(&state.db, &id, &body.key, body.checked) {
        Ok(true) => {
            let trip = shopping::load_trip(&state.db, &id);
            let (done, total) = trip
                .map(|t| (t.buy_done(), t.buy_total()))
                .unwrap_or((0, 0));
            axum::Json(serde_json::json!({
                "ok": true,
                "done": done,
                "total": total,
            }))
            .into_response()
        }
        Ok(false) => (StatusCode::NOT_FOUND, "Trip not found").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

#[derive(Deserialize)]
pub struct TripPantryRequest {
    pub key: String,
    pub in_pantry: bool,
}

/// Move a trip item into (or back out of) the pantry instead of buying it.
/// Updates the trip and the global pantry, then reports refreshed progress.
pub async fn trip_pantry_handler(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<TripPantryRequest>,
) -> Response {
    match shopping::set_item_in_pantry(&state.db, &id, &body.key, body.in_pantry) {
        Ok(true) => {
            let (done, total) = shopping::load_trip(&state.db, &id)
                .map(|t| (t.buy_done(), t.buy_total()))
                .unwrap_or((0, 0));
            axum::Json(serde_json::json!({ "ok": true, "done": done, "total": total }))
                .into_response()
        }
        Ok(false) => (StatusCode::NOT_FOUND, "Item not found on trip").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

/// Close a trip (shopping done) — clears the active banner.
pub async fn close_trip_handler(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    match shopping::close_trip(&state.db, &id) {
        Ok(true) => axum::Json(serde_json::json!({ "ok": true })).into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "Trip not found").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

/// Reopen a previously closed trip and make it active again.
pub async fn reopen_trip_handler(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Response {
    match shopping::reopen_trip(&state.db, &id) {
        Ok(true) => axum::Json(serde_json::json!({ "ok": true })).into_response(),
        Ok(false) => (StatusCode::NOT_FOUND, "Trip not found").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

#[derive(Deserialize)]
pub struct TripSectionRequest {
    pub name: String,
    pub section: String,
}

/// Override the store section an item is filed under. Persists across trips
/// (keyed by ingredient name), so a correction sticks.
pub async fn trip_section_handler(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<TripSectionRequest>,
) -> Response {
    match crate::aisle::set_override(&state.db, &body.name, &body.section) {
        Ok(()) => axum::Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
    }
}

/// View a published trip page by its short slug (durable, browsable per-trip).
pub async fn view_published_trip(
    Path(slug): Path<String>,
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> Response {
    let logged_in = is_logged_in(&jar);
    match shopping::load_published(&state.db, &slug) {
        Some(trip) => {
            Html(crate::templates::shopping::render_published_trip(&trip, logged_in)).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Html(base_html(
                "Not Found",
                "<h1>Trip not found</h1><p>This published trip link may have expired or been deleted.</p>",
                logged_in,
            )),
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
// Meal plan
// ============================================================================

fn render_plan_for_week(state: &AppState, week_start: &str, logged_in: bool) -> Html<String> {
    let plan = mealplan::load_plan(&state.db, week_start);
    let recipes = state.load_recipes();
    let book_recipes = state.load_book();
    let linked = plan
        .trip_id
        .as_deref()
        .and_then(|id| shopping::load_trip(&state.db, id));
    let recent = shopping::list_trips(&state.db);
    // Shop-with-Claude block: the linked trip's items are the stable snapshot;
    // without a trip, aggregate live from the plan's meals (book meals too).
    let store = mealplan::preferred_store(&state.db);
    let selections = mealplan::plan_selections(&plan);
    let shop_items = match &linked {
        Some(trip) => trip.items.clone(),
        None if !selections.is_empty() => {
            let all = book::augment(&recipes, &book_recipes, &selections);
            shopping::build_shopping_list(&selections, &all, &state.db)
        }
        None => Vec::new(),
    };
    let shop_block = if shop_items.is_empty() {
        None
    } else {
        Some(shopping::claude_shop_block(store, &plan.week_start, &shop_items))
    };
    Html(crate::templates::mealplan::render_plan_page(
        &plan,
        &recipes,
        linked.as_ref(),
        &recent,
        mealplan::week_start_day(&state.db),
        store,
        shop_block.as_deref(),
        book_recipes.len(),
        logged_in,
    ))
}

/// This week's meal plan.
pub async fn plan_page(State(state): State<Arc<AppState>>, jar: CookieJar) -> Response {
    let logged_in = is_logged_in(&jar);
    let week = mealplan::week_of(&state.db, &mealplan::today())
        .expect("today is always a valid date");
    render_plan_for_week(&state, &week, logged_in).into_response()
}

/// The meal plan for the week containing `{date}`. Canonical URLs use the
/// week's first day; any other day redirects there.
pub async fn plan_week_page(
    Path(date): Path<String>,
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> Response {
    let logged_in = is_logged_in(&jar);
    match mealplan::week_of(&state.db, &date) {
        Ok(week) if week == date => render_plan_for_week(&state, &week, logged_in).into_response(),
        Ok(week) => Redirect::to(&format!("/plan/{}", week)).into_response(),
        Err(_) => (
            StatusCode::NOT_FOUND,
            Html(base_html("Not Found", "<h1>Bad date</h1>", logged_in)),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct AddMealRequest {
    pub date: String,
    #[serde(default)]
    pub recipe_key: Option<String>,
    #[serde(default)]
    pub book_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub multiplier: Option<f64>,
    #[serde(default)]
    pub meal_type: Option<String>,
}

pub async fn plan_add_meal(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<AddMealRequest>,
) -> Response {
    let recipes = state.load_recipes();
    let book = state.load_book();
    match mealplan::add_meal_entry_typed(
        &state.db,
        &recipes,
        &book,
        &body.date,
        body.recipe_key.as_deref(),
        body.book_id.as_deref(),
        body.title.as_deref(),
        body.multiplier.unwrap_or(1.0),
        body.meal_type.as_deref().unwrap_or("dinner"),
    ) {
        Ok(plan) => axum::Json(serde_json::json!({
            "ok": true,
            "week_start": plan.week_start,
        }))
        .into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
    }
}

#[derive(Deserialize)]
pub struct RemoveMealRequest {
    pub date: String,
    pub meal_id: String,
}

pub async fn plan_remove_meal(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<RemoveMealRequest>,
) -> Response {
    match mealplan::remove_meal(&state.db, &body.date, &body.meal_id) {
        Ok(Some(_)) => axum::Json(serde_json::json!({ "ok": true })).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "No such meal").into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
    }
}

#[derive(Deserialize)]
pub struct PlanNotesRequest {
    pub week_start: String,
    pub notes: String,
}

/// Replace the week's brainstorm notes.
pub async fn plan_set_notes(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<PlanNotesRequest>,
) -> Response {
    match mealplan::set_notes(&state.db, &body.week_start, &body.notes) {
        Ok(plan) => axum::Json(serde_json::json!({
            "ok": true,
            "week_start": plan.week_start,
        }))
        .into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
    }
}

#[derive(Deserialize)]
pub struct PlanLockRequest {
    pub week_start: String,
    pub locked: bool,
}

/// Lock in (or unlock) the week's plan.
pub async fn plan_set_lock(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<PlanLockRequest>,
) -> Response {
    match mealplan::set_locked(&state.db, &body.week_start, body.locked) {
        Ok(plan) => axum::Json(serde_json::json!({
            "ok": true,
            "week_start": plan.week_start,
            "locked": plan.locked,
        }))
        .into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
    }
}

#[derive(Deserialize)]
pub struct WeekStartRequest {
    pub day: String,
}

/// Change the first-day-of-week setting (re-buckets all stored plans).
pub async fn plan_set_week_start(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<WeekStartRequest>,
) -> Response {
    let day = match mealplan::parse_weekday(&body.day) {
        Ok(d) => d,
        Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
    };
    match mealplan::set_week_start_day(&state.db, day) {
        Ok(plans) => axum::Json(serde_json::json!({
            "ok": true,
            "week_start_day": mealplan::weekday_name(day),
            "plans": plans,
        }))
        .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

#[derive(Deserialize)]
pub struct PlanTripRequest {
    pub week_start: String,
    pub action: String,
    #[serde(default)]
    pub trip_id: Option<String>,
}

/// Associate a shopping trip with a week's plan: `build` creates a trip from
/// the plan's recipes (and makes it active), `link` attaches an existing
/// trip, `unlink` detaches.
pub async fn plan_trip_handler(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<PlanTripRequest>,
) -> Response {
    match body.action.as_str() {
        "build" => {
            let recipes = state.load_recipes();
            let book = state.load_book();
            match mealplan::build_trip_for_week(&state.db, &recipes, &book, &body.week_start) {
                Ok(trip_id) => {
                    axum::Json(serde_json::json!({ "ok": true, "trip_id": trip_id }))
                        .into_response()
                }
                Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
            }
        }
        "link" => {
            let Some(trip_id) = body.trip_id.as_deref() else {
                return (StatusCode::BAD_REQUEST, "link requires trip_id").into_response();
            };
            match mealplan::link_trip(&state.db, &body.week_start, Some(trip_id)) {
                Ok(_) => axum::Json(serde_json::json!({ "ok": true })).into_response(),
                Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
            }
        }
        "unlink" => match mealplan::link_trip(&state.db, &body.week_start, None) {
            Ok(_) => axum::Json(serde_json::json!({ "ok": true })).into_response(),
            Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
        },
        other => (
            StatusCode::BAD_REQUEST,
            format!("unknown action: {}", other),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct PlanStoreRequest {
    pub store: String,
}

/// Set the household's preferred Instacart store (Shop-with-Claude block).
pub async fn plan_set_store(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<PlanStoreRequest>,
) -> Response {
    let store = match shopping::parse_store(&body.store) {
        Ok(s) => s,
        Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
    };
    match mealplan::set_preferred_store(&state.db, store) {
        Ok(()) => axum::Json(serde_json::json!({
            "ok": true,
            "store": shopping::store_name(store),
        }))
        .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

// ============================================================================
// Hidden recipe book & meal-builder deck
// ============================================================================

/// A hidden-book recipe page. Not linked from any listing — reachable only
/// from the deck, plan chips, and trips that include the recipe.
pub async fn view_book_recipe(
    Path(id): Path<String>,
    Query(query): Query<RecipeViewQuery>,
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> Response {
    let logged_in = is_logged_in(&jar);
    let book_recipes = state.load_book();
    let Some(b) = book_recipes.iter().find(|b| b.id == id) else {
        return (
            StatusCode::NOT_FOUND,
            Html(base_html(
                "Not Found",
                "<h1>Book recipe not found</h1>",
                logged_in,
            )),
        )
            .into_response();
    };
    // "Already promoted" only counts if the mapped recipe still exists.
    let promoted = book::promoted_key(&state.db, &id).filter(|key| {
        state.load_recipes().iter().any(|r| &r.key == key)
    });
    Html(crate::templates::book::render_book_page(
        b,
        promoted.as_deref(),
        query.from_trip.as_deref(),
        logged_in,
    ))
    .into_response()
}

#[derive(Deserialize)]
pub struct BookCandidatesRequest {
    pub week_start: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Deal the hot-or-not deck: rank the hidden book against the week prompt,
/// excluding this week's skips and already-planned book recipes.
pub async fn book_candidates(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<BookCandidatesRequest>,
) -> Response {
    let week = match mealplan::week_of(&state.db, &body.week_start) {
        Ok(w) => w,
        Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
    };
    let book_recipes = state.load_book();
    let plan = mealplan::load_plan(&state.db, &week);
    let mut excluded: HashSet<String> = book::skips(&state.db, &week).into_iter().collect();
    for meal in &plan.meals {
        if let Some(id) = &meal.book_id {
            excluded.insert(id.clone());
        }
    }
    let limit = body.limit.unwrap_or(150).min(300);
    let ranked = book::rank(&body.prompt, &book_recipes, &excluded, limit);
    let candidates: Vec<serde_json::Value> = ranked
        .iter()
        .map(|b| {
            serde_json::json!({
                "id": b.id,
                "title": b.title,
                "tags": b.tags,
                "servings": b.servings,
                "protein": b.protein,
                "method": b.method,
                "cuisine": b.cuisine,
                "ingredients": b.ingredients.iter().map(|i| serde_json::json!({
                    "name": i.name, "qty": i.qty, "unit": i.unit,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    axum::Json(serde_json::json!({
        "ok": true,
        "week_start": week,
        "candidates": candidates,
    }))
    .into_response()
}

#[derive(Deserialize)]
pub struct BookPickRequest {
    pub week_start: String,
    pub book_id: String,
    #[serde(default)]
    pub multiplier: Option<f64>,
    #[serde(default)]
    pub meal_type: Option<String>,
    #[serde(default)]
    pub date: Option<String>,
}

/// "Hot": plan the book recipe on the week's emptiest day.
pub async fn book_pick(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<BookPickRequest>,
) -> Response {
    let week = match mealplan::week_of(&state.db, &body.week_start) {
        Ok(w) => w,
        Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
    };
    let book_recipes = state.load_book();
    let plan = mealplan::load_plan(&state.db, &week);
    let meal_type = match mealplan::normalize_meal_type(body.meal_type.as_deref().unwrap_or("dinner")) {
        Ok(t) => t,
        Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
    };
    let date = if let Some(date) = body.date.as_deref() {
        match mealplan::week_of(&state.db, date) {
            Ok(date_week) if date_week == week => date.to_string(),
            Ok(_) => return (StatusCode::BAD_REQUEST, "Chosen day is outside this week").into_response(),
            Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
        }
    } else {
        book::assign_date_for_type(&plan, meal_type)
    };
    match mealplan::add_meal_entry_typed(
        &state.db,
        &[],
        &book_recipes,
        &date,
        None,
        Some(&body.book_id),
        None,
        body.multiplier.unwrap_or(1.0),
        meal_type,
    ) {
        Ok(plan) => axum::Json(serde_json::json!({
            "ok": true,
            "date": date,
            "week_start": plan.week_start,
            "meal_type": meal_type,
        }))
        .into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
    }
}

#[derive(Deserialize)]
pub struct BookSkipRequest {
    pub week_start: String,
    pub book_id: String,
}

/// "Not": remember the skip so re-dealing the deck doesn't repeat it.
pub async fn book_skip(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<BookSkipRequest>,
) -> Response {
    let week = match mealplan::week_of(&state.db, &body.week_start) {
        Ok(w) => w,
        Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
    };
    match book::add_skip(&state.db, &week, &body.book_id) {
        Ok(()) => axum::Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

#[derive(Deserialize)]
pub struct BookSkipsClearRequest {
    pub week_start: String,
}

/// Reshuffle: forget the week's skips.
pub async fn book_skips_clear(
    State(state): State<Arc<AppState>>,
    axum::Json(body): axum::Json<BookSkipsClearRequest>,
) -> Response {
    let week = match mealplan::week_of(&state.db, &body.week_start) {
        Ok(w) => w,
        Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
    };
    match book::clear_skips(&state.db, &week) {
        Ok(()) => axum::Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

/// "Add to my recipes": promote a book recipe into the git-backed collection.
pub async fn promote_book_recipe_api(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> Response {
    if !is_logged_in(&jar) {
        return (StatusCode::UNAUTHORIZED, "Not logged in").into_response();
    }
    let state = state.clone();
    let result = tokio::task::spawn_blocking(move || {
        let recipes = state.load_recipes();
        let book_recipes = state.load_book();
        book::promote(&state.content_dir, &state.db, &recipes, &book_recipes, &id, None)
    })
    .await;
    match result {
        Ok(Ok(res)) => axum::Json(serde_json::json!(res)).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, e).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("join error: {}", e)).into_response(),
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

#[cfg(test)]
mod tests {
    use crate::recipes::{slugify, unique_recipe_path};

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
