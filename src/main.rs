//! Recipes application - A recipe management and shopping list webapp.

use axum::{routing::get, Router};
use std::sync::Arc;

use recipes::{auth, handlers, AppState, CONTENT_DIR};

#[tokio::main]
async fn main() {
    let state = Arc::new(AppState::new());

    let app = Router::new()
        // Core routes
        .route("/", get(handlers::index))
        .route("/login", get(handlers::login_page).post(handlers::login_submit))
        .route("/logout", get(handlers::logout))
        // Recipe routes
        .route("/new", get(handlers::new_recipe_page))
        .route("/recipe/{key}", get(handlers::view_recipe))
        .route("/recipe/{key}/edit", get(handlers::edit_recipe))
        .route("/api/recipe", axum::routing::post(handlers::create_recipe_api))
        .route(
            "/api/recipe/{key}",
            axum::routing::post(handlers::save_recipe_api).delete(handlers::delete_recipe),
        )
        // Shopping routes
        .route("/shopping", get(handlers::shopping_page))
        .route("/api/shopping/build", axum::routing::post(handlers::shopping_build))
        .route("/api/shopping/to-pantry", axum::routing::post(handlers::shopping_to_pantry))
        .route("/api/shopping/save-trip", axum::routing::post(handlers::save_trip_handler))
        .route("/shopping/trip/{id}", get(handlers::view_trip_handler))
        // Pantry routes
        .route("/pantry", get(handlers::pantry_page))
        .route("/api/pantry/toggle", axum::routing::post(handlers::pantry_toggle))
        .route("/api/pantry/bulk-add", axum::routing::post(handlers::pantry_bulk_add))
        .route("/api/pantry/bulk-remove", axum::routing::post(handlers::pantry_bulk_remove))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:7001")
        .await
        .expect("Failed to bind to port 7001");

    println!("Recipes server running at http://0.0.0.0:7001");
    println!("Content directory: {}", CONTENT_DIR);

    if auth::is_auth_enabled() {
        println!("Authentication: ENABLED (RECIPES_PASSWORD set)");
    } else {
        println!("Authentication: DISABLED (set RECIPES_PASSWORD env var to enable editing)");
    }

    axum::serve(listener, app).await.expect("Server error");
}
