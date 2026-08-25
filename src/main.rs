//! Recipes application - A recipe management and shopping list webapp.

use axum::{routing::get, Router};
use std::sync::Arc;

use recipes::{auth, handlers, mcp, AppState, CONTENT_DIR};

#[tokio::main]
async fn main() {
    let state = Arc::new(AppState::new());

    let app = Router::new()
        // Core routes
        .route("/", get(handlers::index))
        .route(
            "/login",
            get(handlers::login_page).post(handlers::login_submit),
        )
        .route("/logout", get(handlers::logout))
        // Recipe routes
        .route("/new", get(handlers::new_recipe_page))
        .route("/recipe/{key}", get(handlers::view_recipe))
        .route("/recipe/{key}/edit", get(handlers::edit_recipe))
        .route(
            "/api/recipe",
            axum::routing::post(handlers::create_recipe_api),
        )
        .route(
            "/api/recipe/{key}",
            axum::routing::post(handlers::save_recipe_api).delete(handlers::delete_recipe),
        )
        // Meal plan routes (weekly planner; calendar is the shared kcal component)
        .route("/plan", get(handlers::plan_page))
        .route("/plan/{date}", get(handlers::plan_week_page))
        .route(
            "/api/plan/meal",
            axum::routing::post(handlers::plan_add_meal),
        )
        .route(
            "/api/plan/meal/remove",
            axum::routing::post(handlers::plan_remove_meal),
        )
        .route(
            "/api/plan/trip",
            axum::routing::post(handlers::plan_trip_handler),
        )
        .route(
            "/api/plan/notes",
            axum::routing::post(handlers::plan_set_notes),
        )
        .route(
            "/api/plan/lock",
            axum::routing::post(handlers::plan_set_lock),
        )
        .route(
            "/api/plan/week-start",
            axum::routing::post(handlers::plan_set_week_start),
        )
        .route(
            "/api/plan/store",
            axum::routing::post(handlers::plan_set_store),
        )
        // Hidden recipe book + hot-or-not meal-builder deck
        .route("/book/{id}", get(handlers::view_book_recipe))
        .route(
            "/api/book/candidates",
            axum::routing::post(handlers::book_candidates),
        )
        .route("/api/book/pick", axum::routing::post(handlers::book_pick))
        .route("/api/book/skip", axum::routing::post(handlers::book_skip))
        .route(
            "/api/book/skips/clear",
            axum::routing::post(handlers::book_skips_clear),
        )
        .route(
            "/api/book/{id}/promote",
            axum::routing::post(handlers::promote_book_recipe_api),
        )
        // Shopping routes
        .route("/shopping", get(handlers::shopping_page))
        .route(
            "/api/shopping/build",
            axum::routing::post(handlers::shopping_build),
        )
        .route(
            "/api/shopping/to-pantry",
            axum::routing::post(handlers::shopping_to_pantry),
        )
        .route(
            "/api/shopping/save-trip",
            axum::routing::post(handlers::save_trip_handler),
        )
        .route(
            "/api/instacart/trip/{id}",
            axum::routing::post(handlers::instacart_trip_link_handler),
        )
        // Active-trip checklist: check-off, close, and per-item aisle override
        .route(
            "/api/shopping/trip/{id}/check",
            axum::routing::post(handlers::trip_check_handler),
        )
        .route(
            "/api/shopping/trip/{id}/pantry",
            axum::routing::post(handlers::trip_pantry_handler),
        )
        .route(
            "/api/shopping/trip/{id}/close",
            axum::routing::post(handlers::close_trip_handler),
        )
        .route(
            "/api/shopping/trip/{id}/reopen",
            axum::routing::post(handlers::reopen_trip_handler),
        )
        .route(
            "/api/shopping/section",
            axum::routing::post(handlers::trip_section_handler),
        )
        .route("/api/trip/active", get(handlers::active_trip_handler))
        .route("/shopping/trip/{id}", get(handlers::view_trip_handler))
        // Published trip pages: durable, short-link browsable per-trip "mini sites"
        .route("/t/{slug}", get(handlers::view_published_trip))
        // MCP endpoint (bearer-token auth via RECIPES_MCP_TOKEN; bypasses Authelia in Caddy)
        .route("/mcp", axum::routing::post(mcp::mcp_handler))
        // Pantry routes
        .route("/pantry", get(handlers::pantry_page))
        .route(
            "/api/pantry/toggle",
            axum::routing::post(handlers::pantry_toggle),
        )
        .route(
            "/api/pantry/bulk-add",
            axum::routing::post(handlers::pantry_bulk_add),
        )
        .route(
            "/api/pantry/bulk-remove",
            axum::routing::post(handlers::pantry_bulk_remove),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:7001")
        .await
        .expect("Failed to bind to port 7001");

    println!("Recipes server running at http://0.0.0.0:7001");
    println!("Content directory: {}", CONTENT_DIR);

    if std::env::var("TRUST_PROXY_AUTH").is_ok() {
        println!("Authentication: PROXY (TRUST_PROXY_AUTH set, trusting reverse proxy)");
    } else if auth::is_auth_enabled() {
        println!("Authentication: ENABLED (RECIPES_PASSWORD set)");
    } else {
        println!("Authentication: DISABLED (set RECIPES_PASSWORD env var to enable editing)");
    }

    axum::serve(listener, app).await.expect("Server error");
}
