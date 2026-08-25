//! Authenticated MCP (Model Context Protocol) server for remote recipe access.
//!
//! Exposes the recipes app over JSON-RPC 2.0 at `POST /mcp` so a remote Claude
//! Code client can browse recipes, assemble shopping lists, and inspect the
//! pantry with a bearer token. The token is sourced from the
//! `RECIPES_MCP_TOKEN` env var; if unset the endpoint is disabled (503).
//!
//! Transport: a minimal subset of MCP "Streamable HTTP" — every JSON-RPC
//! request/response is exchanged over a single POST. No SSE.
//!
//! Tools exposed: list_recipes, read_recipe, search_recipes, create_recipe,
//! update_recipe, delete_recipe, build_shopping_list, list_pantry, set_pantry.

use crate::models::{Recipe, RecipeSelection};
use crate::recipes::{generate_key, git_commit, git_rm_commit, serialize_recipe, validate_filename};
use crate::{pantry, shopping, validate_path_within, AppState};

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

const PROTOCOL_VERSION: &str = "2025-06-18";
const SERVER_NAME: &str = "recipes-server";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Public base URL used to build short trip links handed back to clients.
const PUBLIC_BASE_URL: &str = "https://recipes.kmicinski.com";

// ============================================================================
// JSON-RPC envelopes
// ============================================================================

#[derive(Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

fn ok(id: Value, result: Value) -> Response {
    axum::Json(JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: Some(result),
        error: None,
    })
    .into_response()
}

fn err(id: Value, code: i64, message: impl Into<String>) -> Response {
    axum::Json(JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(JsonRpcError {
            code,
            message: message.into(),
            data: None,
        }),
    })
    .into_response()
}

// ============================================================================
// Bearer-token auth (constant-time compare)
// ============================================================================

fn check_bearer(state: &AppState, headers: &HeaderMap) -> Result<(), Response> {
    let token = match &state.mcp_token {
        Some(t) => t.as_bytes(),
        None => {
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                "MCP disabled: RECIPES_MCP_TOKEN is not set",
            )
                .into_response());
        }
    };
    let provided = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .unwrap_or("");
    if provided.as_bytes().len() != token.len() {
        return Err((StatusCode::UNAUTHORIZED, "invalid bearer token").into_response());
    }
    let mut diff: u8 = 0;
    for (a, b) in provided.as_bytes().iter().zip(token.iter()) {
        diff |= a ^ b;
    }
    if diff != 0 {
        return Err((StatusCode::UNAUTHORIZED, "invalid bearer token").into_response());
    }
    Ok(())
}

// ============================================================================
// Top-level handler
// ============================================================================

pub async fn mcp_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: String,
) -> Response {
    if let Err(resp) = check_bearer(&state, &headers) {
        return resp;
    }

    let req: JsonRpcRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => return err(Value::Null, -32700, format!("parse error: {}", e)),
    };

    // Notifications (no id) — ack and return without a body.
    if req.id.is_none() {
        return (StatusCode::ACCEPTED, "").into_response();
    }
    let id = req.id.clone().unwrap_or(Value::Null);

    match req.method.as_str() {
        "initialize" => ok(id, handle_initialize(req.params)),
        "ping" => ok(id, json!({})),
        "tools/list" => ok(id, json!({ "tools": tool_catalog() })),
        "tools/call" => match handle_tools_call(state, req.params).await {
            Ok(v) => ok(id, v),
            Err(msg) => ok(id, tool_error(&msg)),
        },
        "resources/list" => ok(id, json!({ "resources": [] })),
        "prompts/list" => ok(id, json!({ "prompts": [] })),
        m => err(id, -32601, format!("method not found: {}", m)),
    }
}

fn handle_initialize(_params: Value) -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION },
    })
}

// ============================================================================
// Tool catalog
// ============================================================================

fn tool_catalog() -> Vec<Value> {
    vec![
        json!({
            "name": "list_recipes",
            "description": "List recipes. Returns key, title, tags, servings, and ingredient_count for each. Optional filters: query (substring of title), tag (exact tag match), limit (default 200), offset.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "tag": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 1000},
                    "offset": {"type": "integer", "minimum": 0}
                }
            }
        }),
        json!({
            "name": "read_recipe",
            "description": "Read a single recipe by key. Returns title, tags, servings, the structured ingredient list (name/qty/unit), and the markdown instructions body.",
            "inputSchema": {
                "type": "object",
                "properties": { "key": {"type": "string"} },
                "required": ["key"]
            }
        }),
        json!({
            "name": "search_recipes",
            "description": "Substring search across recipe titles, tags, ingredient names, and instructions. Returns matching recipe summaries.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 200}
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "build_shopping_list",
            "description": "Assemble an aggregated shopping list from a set of recipe selections. Each selection is {key, multiplier}. Ingredients are summed across recipes by (name, unit), annotated with which recipes they come from and whether they are already in the pantry. Returns to_buy (not in pantry) and have (in pantry) groups.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "selections": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "key": {"type": "string"},
                                "multiplier": {"type": "number"}
                            },
                            "required": ["key"]
                        }
                    }
                },
                "required": ["selections"]
            }
        }),
        json!({
            "name": "publish_trip",
            "description": "Publish a durable, browsable shopping-trip web page from a set of recipe selections and return a SHORT link (https://recipes.kmicinski.com/t/<slug>). The page embeds the aggregated/pantry-annotated shopping list AND each recipe's ingredients and prep steps, snapshotted so the link keeps working even if recipes change later. Use this to hand the user a per-trip page they can browse and print. `selections` is [{key, multiplier}]. Optional `title` (defaults to 'Shopping Trip') and `notes` (markdown intro shown at the top).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "selections": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "key": {"type": "string"},
                                "multiplier": {"type": "number"}
                            },
                            "required": ["key"]
                        }
                    },
                    "title": {"type": "string"},
                    "notes": {"type": "string"}
                },
                "required": ["selections"]
            }
        }),
        json!({
            "name": "list_trips",
            "description": "List published trip pages (most recent first). Returns slug, title, url, created_at, recipe_count, and item_count for each.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "delete_trip",
            "description": "Delete a published trip page by slug. Requires `confirm: true`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slug": {"type": "string"},
                    "confirm": {"type": "boolean"}
                },
                "required": ["slug", "confirm"]
            }
        }),
        json!({
            "name": "list_pantry",
            "description": "List all ingredient names currently marked as in the pantry (binary have/don't-have state), sorted alphabetically.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "set_pantry",
            "description": "Add or remove an ingredient from the pantry. `in_pantry: true` adds it, `false` removes it. Names are normalized (trimmed, lowercased).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "in_pantry": {"type": "boolean"}
                },
                "required": ["name", "in_pantry"]
            }
        }),
        json!({
            "name": "create_recipe",
            "description": "Create a new recipe markdown file. `filename` must end in .md and be a single segment (no path components). Provide title, optional servings, optional tags (array), ingredients (array of {name, qty, unit}), and the markdown instructions body. Commits to git. Returns the generated key.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "filename": {"type": "string"},
                    "title": {"type": "string"},
                    "servings": {"type": "integer", "minimum": 1},
                    "tags": {"type": "array", "items": {"type": "string"}},
                    "ingredients": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": {"type": "string"},
                                "qty": {"type": "number"},
                                "unit": {"type": "string"}
                            },
                            "required": ["name"]
                        }
                    },
                    "body": {"type": "string"}
                },
                "required": ["filename", "title", "ingredients", "body"]
            }
        }),
        json!({
            "name": "update_recipe",
            "description": "Overwrite an existing recipe (by key) with new title/servings/tags/ingredients/body. Commits to git.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": {"type": "string"},
                    "title": {"type": "string"},
                    "servings": {"type": "integer", "minimum": 1},
                    "tags": {"type": "array", "items": {"type": "string"}},
                    "ingredients": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": {"type": "string"},
                                "qty": {"type": "number"},
                                "unit": {"type": "string"}
                            },
                            "required": ["name"]
                        }
                    },
                    "body": {"type": "string"}
                },
                "required": ["key", "title", "ingredients", "body"]
            }
        }),
        json!({
            "name": "delete_recipe",
            "description": "Delete a recipe by key. Requires `confirm: true`. Commits the deletion to git.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": {"type": "string"},
                    "confirm": {"type": "boolean"}
                },
                "required": ["key", "confirm"]
            }
        }),
        json!({
            "name": "get_meal_plan",
            "description": "The weekly meal plan for the week containing `week_of` (default: today). Weeks start on the configured `week_start_day` (see set_week_start_day; default Monday). Includes the brainstorm notes, the locked flag, each day's planned meals, and the associated shopping trip, if any.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "week_of": {"type": "string", "description": "Any date in the week, YYYY-MM-DD (default today)"}
                }
            }
        }),
        json!({
            "name": "set_plan_notes",
            "description": "Replace the brainstorm notes (markdown) for the week containing `week_of` (default: today). This is the scratchpad for sketching the week's food before meals are locked in — ideas, constraints, what to reuse across days.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "week_of": {"type": "string", "description": "Any date in the week, YYYY-MM-DD (default today)"},
                    "notes": {"type": "string", "description": "The full replacement notes (markdown). Empty string clears them."}
                },
                "required": ["notes"]
            }
        }),
        json!({
            "name": "lock_plan",
            "description": "Lock in (locked: true) or reopen (false) the plan for the week containing `week_of` (default: today). A locked plan is final: the app surfaces the week's meals prominently (plan page + home-page strip) instead of the brainstorm.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "week_of": {"type": "string", "description": "Any date in the week, YYYY-MM-DD (default today)"},
                    "locked": {"type": "boolean"}
                },
                "required": ["locked"]
            }
        }),
        json!({
            "name": "set_week_start_day",
            "description": "Set which weekday the planning week starts on (household-wide). Re-buckets every stored plan so each meal lands in the correct new week. Use when the household's shopping rhythm starts on e.g. Saturday rather than Monday.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "day": {"type": "string", "description": "monday…sunday (3-letter abbreviations accepted)"}
                },
                "required": ["day"]
            }
        }),
        json!({
            "name": "build_plan_trip",
            "description": "Build a shopping trip from the recipe-backed meals of the week containing `week_of` (default: today), make it the active in-store checklist, and link it to the plan. Returns the aggregated list split into to_buy and already_have (pantry). Fails if the week has no recipe-based meals.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "week_of": {"type": "string", "description": "Any date in the week, YYYY-MM-DD (default today)"}
                }
            }
        }),
        json!({
            "name": "plan_meal",
            "description": "Add a meal to the weekly plan on a specific date. Provide `recipe_key` for a recipe from the collection (its title is snapshotted), `book_id` for a hidden-book recipe (bk-…), or a free-text `title` (e.g. 'leftovers'). `meal_type` is breakfast, lunch, or dinner (default dinner); `multiplier` scales servings when a shopping trip is built.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "date": {"type": "string", "description": "YYYY-MM-DD"},
                    "recipe_key": {"type": "string"},
                    "book_id": {"type": "string", "description": "Hidden-book recipe id (bk-…)"},
                    "title": {"type": "string"},
                    "multiplier": {"type": "number", "default": 1},
                    "meal_type": {"type": "string", "enum": ["breakfast", "lunch", "dinner"], "default": "dinner"}
                },
                "required": ["date"]
            }
        }),
        json!({
            "name": "browse_book",
            "description": "Browse the HIDDEN recipe book — a large corpus of pre-generated meal-kit-style recipes (minimal prep/cleanup, ~20 min day-of with weekend batch prep) that are NOT part of the user's collection and never appear in list_recipes/search_recipes. Ranks the book against an optional free-text `query` (week-prompt style; `-token` excludes). Returns summaries with id (bk-…), title, tags, facets (protein/method/cuisine), and ingredient_count. Book recipes can be planned via plan_meal(book_id) and only join the collection via promote_book_recipe.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 200}
                }
            }
        }),
        json!({
            "name": "read_book_recipe",
            "description": "Read one hidden-book recipe by id (bk-…): title, tags, facets, servings, structured ingredients, and the markdown body (prep-ahead + day-of sections).",
            "inputSchema": {
                "type": "object",
                "properties": { "id": {"type": "string"} },
                "required": ["id"]
            }
        }),
        json!({
            "name": "promote_book_recipe",
            "description": "Promote a hidden-book recipe into the user's collection: writes the markdown file, commits to git, and rewrites any planned meals from the book id to the new recipe key. Idempotent — promoting twice returns the existing recipe. Optional `filename` (single .md segment); defaults to a slug of the title.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "filename": {"type": "string"}
                },
                "required": ["id"]
            }
        }),
        json!({
            "name": "remove_meal",
            "description": "Remove a planned meal by its id (see get_meal_plan) from the week containing `date`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "date": {"type": "string", "description": "The meal's date, YYYY-MM-DD"},
                    "meal_id": {"type": "string"}
                },
                "required": ["date", "meal_id"]
            }
        }),
    ]
}

// ============================================================================
// Tool result helpers
// ============================================================================

fn tool_text(value: &Value) -> Value {
    let text = serde_json::to_string_pretty(value).unwrap_or_else(|_| "<unserializable>".into());
    json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": value,
        "isError": false
    })
}

fn tool_error(msg: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": msg }],
        "isError": true
    })
}

// ============================================================================
// Tool dispatch
// ============================================================================

async fn handle_tools_call(state: Arc<AppState>, params: Value) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'name'".to_string())?
        .to_string();
    let args = params.get("arguments").cloned().unwrap_or(json!({}));

    match name.as_str() {
        "list_recipes" => Ok(tool_text(&tool_list_recipes(state, args)?)),
        "read_recipe" => Ok(tool_text(&tool_read_recipe(state, args)?)),
        "search_recipes" => Ok(tool_text(&tool_search_recipes(state, args)?)),
        "build_shopping_list" => Ok(tool_text(&tool_build_shopping_list(state, args)?)),
        "publish_trip" => Ok(tool_text(&tool_publish_trip(state, args)?)),
        "list_trips" => Ok(tool_text(&tool_list_trips(state)?)),
        "delete_trip" => Ok(tool_text(&tool_delete_trip(state, args)?)),
        "list_pantry" => Ok(tool_text(&tool_list_pantry(state)?)),
        "set_pantry" => Ok(tool_text(&tool_set_pantry(state, args)?)),
        "get_meal_plan" => Ok(tool_text(&tool_get_meal_plan(state, args)?)),
        "plan_meal" => Ok(tool_text(&tool_plan_meal(state, args)?)),
        "remove_meal" => Ok(tool_text(&tool_remove_meal(state, args)?)),
        "set_plan_notes" => Ok(tool_text(&tool_set_plan_notes(state, args)?)),
        "lock_plan" => Ok(tool_text(&tool_lock_plan(state, args)?)),
        "set_week_start_day" => Ok(tool_text(&tool_set_week_start_day(state, args)?)),
        "build_plan_trip" => Ok(tool_text(&tool_build_plan_trip(state, args)?)),
        "browse_book" => Ok(tool_text(&tool_browse_book(state, args)?)),
        "read_book_recipe" => Ok(tool_text(&tool_read_book_recipe(state, args)?)),
        "promote_book_recipe" => Ok(tool_text(&tool_promote_book_recipe(state, args)?)),
        "create_recipe" => Ok(tool_text(&tool_create_recipe(state, args)?)),
        "update_recipe" => Ok(tool_text(&tool_update_recipe(state, args)?)),
        "delete_recipe" => Ok(tool_text(&tool_delete_recipe(state, args)?)),
        other => Err(format!("unknown tool: {}", other)),
    }
}

// ============================================================================
// Tool implementations
// ============================================================================

fn recipe_summary(r: &Recipe) -> Value {
    json!({
        "key": r.key,
        "title": r.title,
        "tags": r.tags,
        "servings": r.servings,
        "ingredient_count": r.ingredients.len(),
        "modified": r.modified.to_rfc3339(),
    })
}

fn ingredients_json(r: &Recipe) -> Vec<Value> {
    r.ingredients
        .iter()
        .map(|i| json!({ "name": i.name, "qty": i.qty, "unit": i.unit }))
        .collect()
}

fn tool_list_recipes(state: Arc<AppState>, args: Value) -> Result<Value, String> {
    let recipes = state.load_recipes();
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase());
    let tag = args
        .get("tag")
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase());
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(200) as usize;
    let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

    let filtered: Vec<&Recipe> = recipes
        .iter()
        .filter(|r| match &query {
            Some(q) => r.title.to_lowercase().contains(q),
            None => true,
        })
        .filter(|r| match &tag {
            Some(t) => r.tags.iter().any(|tag| tag.to_lowercase() == *t),
            None => true,
        })
        .collect();

    let total = filtered.len();
    let page: Vec<Value> = filtered
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(recipe_summary)
        .collect();

    Ok(json!({
        "total": total,
        "offset": offset,
        "limit": limit,
        "recipes": page,
    }))
}

fn tool_read_recipe(state: Arc<AppState>, args: Value) -> Result<Value, String> {
    let key = args
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'key'".to_string())?;
    let map = state.recipes_map();
    let r = map
        .get(key)
        .ok_or_else(|| format!("recipe not found: {}", key))?;
    Ok(json!({
        "key": r.key,
        "title": r.title,
        "tags": r.tags,
        "servings": r.servings,
        "ingredients": ingredients_json(r),
        "body": r.body_markdown,
        "path": r.path.to_string_lossy(),
        "modified": r.modified.to_rfc3339(),
    }))
}

fn tool_search_recipes(state: Arc<AppState>, args: Value) -> Result<Value, String> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'query'".to_string())?
        .to_lowercase();
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
    let recipes = state.load_recipes();

    let mut hits: Vec<Value> = Vec::new();
    for r in &recipes {
        let in_title = r.title.to_lowercase().contains(&query);
        let in_tags = r.tags.iter().any(|t| t.to_lowercase().contains(&query));
        let in_ings = r
            .ingredients
            .iter()
            .any(|i| i.name.to_lowercase().contains(&query));
        let in_body = r.body_markdown.to_lowercase().contains(&query);
        if in_title || in_tags || in_ings || in_body {
            let mut matched_in = Vec::new();
            if in_title {
                matched_in.push("title");
            }
            if in_tags {
                matched_in.push("tags");
            }
            if in_ings {
                matched_in.push("ingredients");
            }
            if in_body {
                matched_in.push("body");
            }
            let mut summary = recipe_summary(r);
            summary["matched_in"] = json!(matched_in);
            hits.push(summary);
        }
        if hits.len() >= limit {
            break;
        }
    }
    Ok(json!({ "query": query, "results": hits }))
}

fn tool_build_shopping_list(state: Arc<AppState>, args: Value) -> Result<Value, String> {
    let selections = parse_selections(&args)?;

    let recipes = state.load_recipes();

    // Report any keys that didn't resolve to a known recipe.
    let known: std::collections::HashSet<&str> = recipes.iter().map(|r| r.key.as_str()).collect();
    let unknown: Vec<String> = selections
        .iter()
        .map(|s| s.key.clone())
        .filter(|k| !known.contains(k.as_str()))
        .collect();

    let resolved = shopping::resolve_trip_recipes(&selections, &recipes);
    let items = shopping::build_shopping_list(&selections, &recipes, &state.db);

    let to_buy: Vec<Value> = items
        .iter()
        .filter(|i| !i.in_pantry)
        .map(shopping_item_json)
        .collect();
    let have: Vec<Value> = items
        .iter()
        .filter(|i| i.in_pantry)
        .map(shopping_item_json)
        .collect();

    Ok(json!({
        "recipes": resolved.iter().map(|t| json!({
            "key": t.key, "title": t.title, "multiplier": t.multiplier
        })).collect::<Vec<_>>(),
        "unknown_keys": unknown,
        "to_buy": to_buy,
        "have": have,
        "to_buy_count": to_buy.len(),
        "have_count": have.len(),
    }))
}

fn shopping_item_json(i: &crate::models::ShoppingItem) -> Value {
    json!({
        "name": i.name,
        "qty": i.qty,
        "unit": i.unit,
        "in_pantry": i.in_pantry,
        "sources": i.sources,
    })
}

/// Parse a `selections` argument into RecipeSelection values.
fn parse_selections(args: &Value) -> Result<Vec<RecipeSelection>, String> {
    let sel_val = args
        .get("selections")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "missing 'selections' array".to_string())?;
    let mut selections = Vec::new();
    for s in sel_val {
        let key = s
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "each selection needs a 'key'".to_string())?
            .to_string();
        let multiplier = s.get("multiplier").and_then(|v| v.as_f64()).unwrap_or(1.0);
        selections.push(RecipeSelection { key, multiplier });
    }
    Ok(selections)
}

fn tool_publish_trip(state: Arc<AppState>, args: Value) -> Result<Value, String> {
    let selections = parse_selections(&args)?;
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("Shopping Trip")
        .to_string();
    let notes_html = args
        .get("notes")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(crate::recipes::render_markdown)
        .unwrap_or_default();

    let recipes = state.load_recipes();
    let recipe_map: std::collections::HashMap<&str, &Recipe> =
        recipes.iter().map(|r| (r.key.as_str(), r)).collect();

    // Report unknown keys so the caller can correct them.
    let unknown: Vec<String> = selections
        .iter()
        .map(|s| s.key.clone())
        .filter(|k| !recipe_map.contains_key(k.as_str()))
        .collect();
    if !unknown.is_empty() {
        return Err(format!("unknown recipe keys: {}", unknown.join(", ")));
    }

    let items = shopping::build_shopping_list(&selections, &recipes, &state.db);

    // Deduped recipe list with summed multipliers, snapshotted into durable cards.
    let resolved = shopping::resolve_trip_recipes(&selections, &recipes);
    let cards: Vec<shopping::RecipeCard> = resolved
        .iter()
        .filter_map(|tr| {
            recipe_map.get(tr.key.as_str()).map(|r| shopping::RecipeCard {
                key: r.key.clone(),
                title: r.title.clone(),
                tags: r.tags.clone(),
                multiplier: tr.multiplier,
                ingredients: r.ingredients.clone(),
                body_html: r.body_html.clone(),
            })
        })
        .collect();

    let created_at = chrono::Utc::now().to_rfc3339();
    let slug = shopping::publish_trip(&state.db, &title, &notes_html, &items, &cards, &created_at)?;
    let url = format!("{}/t/{}", PUBLIC_BASE_URL, slug);

    let to_buy = items.iter().filter(|i| !i.in_pantry).count();
    let have = items.iter().filter(|i| i.in_pantry).count();

    Ok(json!({
        "slug": slug,
        "url": url,
        "title": title,
        "recipe_count": cards.len(),
        "to_buy_count": to_buy,
        "have_count": have,
    }))
}

fn tool_list_trips(state: Arc<AppState>) -> Result<Value, String> {
    let trips = shopping::list_published(&state.db);
    let out: Vec<Value> = trips
        .iter()
        .map(|t| {
            json!({
                "slug": t.slug,
                "title": t.title,
                "url": format!("{}/t/{}", PUBLIC_BASE_URL, t.slug),
                "created_at": t.created_at,
                "recipe_count": t.cards.len(),
                "item_count": t.items.len(),
            })
        })
        .collect();
    Ok(json!({ "count": out.len(), "trips": out }))
}

fn tool_delete_trip(state: Arc<AppState>, args: Value) -> Result<Value, String> {
    let slug = args
        .get("slug")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'slug'".to_string())?;
    let confirm = args.get("confirm").and_then(|v| v.as_bool()).unwrap_or(false);
    if !confirm {
        return Err("delete requires confirm=true".into());
    }
    let existed = shopping::delete_published(&state.db, slug)?;
    Ok(json!({ "ok": true, "slug": slug, "existed": existed }))
}

fn meal_json(m: &crate::mealplan::PlannedMeal) -> Value {
    json!({
        "id": m.id,
        "date": m.date,
        "title": m.title,
        "recipe_key": m.recipe_key,
        "book_id": m.book_id,
        "multiplier": m.multiplier,
    })
}

fn plan_json(state: &AppState, plan: &crate::mealplan::MealPlan) -> Value {
    let days: Vec<Value> = plan
        .week_dates()
        .into_iter()
        .map(|date| {
            let meals: Vec<Value> = plan.meals_on(&date).into_iter().map(meal_json).collect();
            json!({ "date": date, "meals": meals })
        })
        .collect();
    let trip = plan
        .trip_id
        .as_deref()
        .and_then(|id| shopping::load_trip(&state.db, id))
        .map(|t| {
            json!({
                "id": t.id,
                "created_at": t.created_at,
                "closed": t.closed,
                "picked_up": t.buy_done(),
                "to_buy": t.buy_total(),
            })
        });
    json!({
        "week_start": plan.week_start,
        "week_start_day": crate::mealplan::weekday_name(crate::mealplan::week_start_day(&state.db)),
        "notes": plan.notes,
        "locked": plan.locked,
        "days": days,
        "trip": trip,
    })
}

fn tool_get_meal_plan(state: Arc<AppState>, args: Value) -> Result<Value, String> {
    let week_of = args
        .get("week_of")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(crate::mealplan::today);
    let week = crate::mealplan::week_of(&state.db, &week_of)?;
    let plan = crate::mealplan::load_plan(&state.db, &week);
    Ok(plan_json(&state, &plan))
}

fn tool_set_plan_notes(state: Arc<AppState>, args: Value) -> Result<Value, String> {
    let week_of = args
        .get("week_of")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(crate::mealplan::today);
    let notes = args
        .get("notes")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'notes'".to_string())?;
    let plan = crate::mealplan::set_notes(&state.db, &week_of, notes)?;
    Ok(plan_json(&state, &plan))
}

fn tool_lock_plan(state: Arc<AppState>, args: Value) -> Result<Value, String> {
    let week_of = args
        .get("week_of")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(crate::mealplan::today);
    let locked = args
        .get("locked")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| "missing 'locked'".to_string())?;
    let plan = crate::mealplan::set_locked(&state.db, &week_of, locked)?;
    Ok(plan_json(&state, &plan))
}

fn tool_set_week_start_day(state: Arc<AppState>, args: Value) -> Result<Value, String> {
    let day = args
        .get("day")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'day'".to_string())?;
    let day = crate::mealplan::parse_weekday(day)?;
    let plans = crate::mealplan::set_week_start_day(&state.db, day)?;
    Ok(json!({
        "week_start_day": crate::mealplan::weekday_name(day),
        "plans_rebucketed": plans,
    }))
}

fn tool_build_plan_trip(state: Arc<AppState>, args: Value) -> Result<Value, String> {
    let week_of = args
        .get("week_of")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(crate::mealplan::today);
    let recipes = state.load_recipes();
    let book = state.load_book();
    let trip_id = crate::mealplan::build_trip_for_week(&state.db, &recipes, &book, &week_of)?;
    let trip = shopping::load_trip(&state.db, &trip_id)
        .ok_or_else(|| "trip vanished after save".to_string())?;
    let (to_buy, have): (Vec<_>, Vec<_>) = trip.items.iter().partition(|i| !i.in_pantry);
    let item_json = |i: &&crate::models::ShoppingItem| {
        json!({ "name": i.name, "qty": i.qty, "unit": i.unit, "sources": i.sources })
    };
    Ok(json!({
        "trip_id": trip_id,
        "trip_url": format!("/shopping/trip/{}", trip.id),
        "recipes": trip.recipes.iter().map(|r| json!({
            "key": r.key, "title": r.title, "multiplier": r.multiplier,
        })).collect::<Vec<_>>(),
        "to_buy": to_buy.iter().map(item_json).collect::<Vec<_>>(),
        "already_have": have.iter().map(item_json).collect::<Vec<_>>(),
    }))
}

fn tool_plan_meal(state: Arc<AppState>, args: Value) -> Result<Value, String> {
    let date = args
        .get("date")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'date'".to_string())?;
    let recipe_key = args.get("recipe_key").and_then(|v| v.as_str());
    let book_id = args.get("book_id").and_then(|v| v.as_str());
    let title = args.get("title").and_then(|v| v.as_str());
    let multiplier = args.get("multiplier").and_then(|v| v.as_f64()).unwrap_or(1.0);
    let meal_type = args.get("meal_type").and_then(|v| v.as_str()).unwrap_or("dinner");
    let recipes = state.load_recipes();
    let book = state.load_book();
    let plan = crate::mealplan::add_meal_entry_typed(
        &state.db, &recipes, &book, date, recipe_key, book_id, title, multiplier, meal_type,
    )?;
    Ok(plan_json(&state, &plan))
}

fn tool_remove_meal(state: Arc<AppState>, args: Value) -> Result<Value, String> {
    let date = args
        .get("date")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'date'".to_string())?;
    let meal_id = args
        .get("meal_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'meal_id'".to_string())?;
    match crate::mealplan::remove_meal(&state.db, date, meal_id)? {
        Some(plan) => Ok(plan_json(&state, &plan)),
        None => Err(format!("No meal '{}' in the week of {}", meal_id, date)),
    }
}

fn tool_list_pantry(state: Arc<AppState>) -> Result<Value, String> {
    let items = pantry::list(&state.db);
    Ok(json!({ "count": items.len(), "pantry": items }))
}

fn tool_set_pantry(state: Arc<AppState>, args: Value) -> Result<Value, String> {
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'name'".to_string())?;
    let in_pantry = args
        .get("in_pantry")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| "missing 'in_pantry'".to_string())?;
    if in_pantry {
        pantry::add(&state.db, name)?;
    } else {
        pantry::remove(&state.db, name)?;
    }
    Ok(json!({ "name": pantry::normalize(name), "in_pantry": in_pantry }))
}

/// Parse ingredient JSON array into the model type.
fn parse_ingredients(v: &Value) -> Result<Vec<crate::models::Ingredient>, String> {
    let arr = v
        .as_array()
        .ok_or_else(|| "'ingredients' must be an array".to_string())?;
    let mut out = Vec::new();
    for item in arr {
        let name = item
            .get("name")
            .and_then(|x| x.as_str())
            .ok_or_else(|| "each ingredient needs a 'name'".to_string())?
            .to_string();
        let qty = item.get("qty").and_then(|x| x.as_f64()).unwrap_or(0.0);
        let unit = item
            .get("unit")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        out.push(crate::models::Ingredient { name, qty, unit });
    }
    Ok(out)
}

// ============================================================================
// Hidden recipe book tools
// ============================================================================

fn tool_browse_book(state: Arc<AppState>, args: Value) -> Result<Value, String> {
    let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
    let book = state.load_book();
    let ranked = crate::book::rank(query, &book, &std::collections::HashSet::new(), limit);
    let results: Vec<Value> = ranked
        .iter()
        .map(|b| {
            json!({
                "id": b.id,
                "title": b.title,
                "tags": b.tags,
                "servings": b.servings,
                "protein": b.protein,
                "method": b.method,
                "cuisine": b.cuisine,
                "ingredient_count": b.ingredients.len(),
            })
        })
        .collect();
    Ok(json!({
        "book_size": book.len(),
        "query": query,
        "results": results,
    }))
}

fn tool_read_book_recipe(state: Arc<AppState>, args: Value) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'id'".to_string())?;
    let book = state.load_book();
    let b = book
        .iter()
        .find(|b| b.id == id)
        .ok_or_else(|| format!("book recipe not found: {}", id))?;
    Ok(json!({
        "id": b.id,
        "title": b.title,
        "tags": b.tags,
        "servings": b.servings,
        "protein": b.protein,
        "method": b.method,
        "cuisine": b.cuisine,
        "ingredients": b.ingredients.iter().map(|i| json!({
            "name": i.name, "qty": i.qty, "unit": i.unit,
        })).collect::<Vec<_>>(),
        "body": b.body_markdown,
        "promoted_key": crate::book::promoted_key(&state.db, id),
    }))
}

fn tool_promote_book_recipe(state: Arc<AppState>, args: Value) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'id'".to_string())?;
    let filename = args.get("filename").and_then(|v| v.as_str());
    let recipes = state.load_recipes();
    let book = state.load_book();
    let res = crate::book::promote(&state.content_dir, &state.db, &recipes, &book, id, filename)?;
    serde_json::to_value(&res).map_err(|e| format!("serialize error: {}", e))
}

fn tool_create_recipe(state: Arc<AppState>, args: Value) -> Result<Value, String> {
    let filename = args
        .get("filename")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'filename'".to_string())?
        .trim()
        .to_string();
    validate_filename(&filename)?;

    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'title'".to_string())?;
    let servings = args.get("servings").and_then(|v| v.as_u64()).map(|s| s as u32);
    let tags: Vec<String> = args
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|t| t.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let ingredients = parse_ingredients(args.get("ingredients").unwrap_or(&json!([])))?;
    let body = args.get("body").and_then(|v| v.as_str()).unwrap_or("");

    let path = state.content_dir.join(&filename);
    validate_path_within(&state.content_dir, &path)?;
    if path.exists() {
        return Err(format!("recipe already exists: {}", filename));
    }
    let content = serialize_recipe(title, servings, &tags, &ingredients, body);
    fs::write(&path, &content).map_err(|e| format!("write failed: {}", e))?;

    let rel_path = PathBuf::from(&filename);
    let key = generate_key(&rel_path);
    git_commit(
        &state.content_dir,
        &state.content_dir.join(&filename),
        &format!("Add recipe via MCP: {}", title),
    );
    Ok(json!({ "key": key, "filename": filename }))
}

fn tool_update_recipe(state: Arc<AppState>, args: Value) -> Result<Value, String> {
    let key = args
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'key'".to_string())?;
    let map = state.recipes_map();
    let existing = map
        .get(key)
        .ok_or_else(|| format!("recipe not found: {}", key))?;
    let path = existing.path.clone();

    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'title'".to_string())?;
    let servings = args.get("servings").and_then(|v| v.as_u64()).map(|s| s as u32);
    let tags: Vec<String> = args
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|t| t.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let ingredients = parse_ingredients(args.get("ingredients").unwrap_or(&json!([])))?;
    let body = args.get("body").and_then(|v| v.as_str()).unwrap_or("");

    let content = serialize_recipe(title, servings, &tags, &ingredients, body);
    fs::write(&path, &content).map_err(|e| format!("write failed: {}", e))?;
    git_commit(
        &state.content_dir,
        &path,
        &format!("Update recipe via MCP: {}", title),
    );
    Ok(json!({ "key": key, "bytes": content.len() }))
}

fn tool_delete_recipe(state: Arc<AppState>, args: Value) -> Result<Value, String> {
    let key = args
        .get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'key'".to_string())?;
    let confirm = args.get("confirm").and_then(|v| v.as_bool()).unwrap_or(false);
    if !confirm {
        return Err("delete requires confirm=true".into());
    }
    let map = state.recipes_map();
    let existing = map
        .get(key)
        .ok_or_else(|| format!("recipe not found: {}", key))?;
    let path = existing.path.clone();
    let title = existing.title.clone();
    git_rm_commit(
        &state.content_dir,
        &path,
        &format!("Delete recipe via MCP: {}", title),
    );
    // git rm already removed the file; ensure it's gone even if git failed.
    let _ = fs::remove_file(&path);
    Ok(json!({ "ok": true, "key": key, "deleted_path": path.to_string_lossy() }))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipes::load_recipe;

    /// Build an AppState backed by a temp content dir + temp sled db, seeded
    /// with two simple recipes. Returns (state, key_a, key_b).
    fn seeded_state() -> (Arc<AppState>, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let content_dir = dir.path().join("content");
        fs::create_dir_all(&content_dir).unwrap();

        let recipe_a = "---\ntitle: Soup\ntags: mexican, freezer\ningredients:\n  - name: onion\n    qty: 1\n    unit: whole\n  - name: salt\n    qty: 2\n    unit: tsp\n---\n\nSimmer.\n";
        let recipe_b = "---\ntitle: Rice\ntags: mexican, freezer\ningredients:\n  - name: rice\n    qty: 2\n    unit: cups\n  - name: salt\n    qty: 1\n    unit: tsp\n---\n\nCook.\n";
        fs::write(content_dir.join("soup.md"), recipe_a).unwrap();
        fs::write(content_dir.join("rice.md"), recipe_b).unwrap();

        // Keys are derived from the path relative to content_dir.
        let key_a = load_recipe(&content_dir.join("soup.md"), &content_dir)
            .unwrap()
            .key;
        let key_b = load_recipe(&content_dir.join("rice.md"), &content_dir)
            .unwrap()
            .key;

        let db = sled::open(dir.path().join("db")).unwrap();
        // Leak the tempdir so files survive for the test's lifetime.
        std::mem::forget(dir);

        let state = Arc::new(AppState::for_test(content_dir, db));
        (state, key_a, key_b)
    }

    #[test]
    fn list_recipes_filters_by_tag() {
        let (state, _, _) = seeded_state();
        let out = tool_list_recipes(state, json!({ "tag": "freezer" })).unwrap();
        assert_eq!(out["total"], 2);
        let titles: Vec<&str> = out["recipes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["title"].as_str().unwrap())
            .collect();
        assert!(titles.contains(&"Soup"));
        assert!(titles.contains(&"Rice"));
    }

    #[test]
    fn read_recipe_returns_ingredients() {
        let (state, key_a, _) = seeded_state();
        let out = tool_read_recipe(state, json!({ "key": key_a })).unwrap();
        assert_eq!(out["title"], "Soup");
        assert_eq!(out["ingredients"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn build_shopping_list_aggregates_and_annotates_pantry() {
        let (state, key_a, key_b) = seeded_state();
        // Put onion in the pantry so it lands in `have`, not `to_buy`.
        pantry::add(&state.db, "onion").unwrap();

        let out = tool_build_shopping_list(
            state,
            json!({ "selections": [
                { "key": key_a, "multiplier": 1 },
                { "key": key_b, "multiplier": 2 }
            ]}),
        )
        .unwrap();

        assert!(out["unknown_keys"].as_array().unwrap().is_empty());

        // salt: 2 tsp (Soup x1) + 1 tsp (Rice x2 = 2 tsp) = 4 tsp, both sources.
        let to_buy = out["to_buy"].as_array().unwrap();
        let salt = to_buy
            .iter()
            .find(|i| i["name"] == "salt")
            .expect("salt should be on the buy list");
        assert_eq!(salt["qty"], 4.0);
        assert_eq!(salt["sources"].as_array().unwrap().len(), 2);

        // rice scaled by 2 = 4 cups.
        let rice = to_buy.iter().find(|i| i["name"] == "rice").unwrap();
        assert_eq!(rice["qty"], 4.0);

        // onion is pantry-annotated.
        let have = out["have"].as_array().unwrap();
        assert!(have.iter().any(|i| i["name"] == "onion"));
        assert!(!to_buy.iter().any(|i| i["name"] == "onion"));
    }

    #[test]
    fn build_shopping_list_reports_unknown_keys() {
        let (state, key_a, _) = seeded_state();
        let out = tool_build_shopping_list(
            state,
            json!({ "selections": [
                { "key": key_a, "multiplier": 1 },
                { "key": "zzzzzz", "multiplier": 1 }
            ]}),
        )
        .unwrap();
        assert_eq!(out["unknown_keys"], json!(["zzzzzz"]));
    }

    #[test]
    fn publish_trip_returns_short_link_and_snapshots_recipes() {
        let (state, key_a, key_b) = seeded_state();
        let out = tool_publish_trip(
            state.clone(),
            json!({
                "selections": [
                    { "key": key_a, "multiplier": 1 },
                    { "key": key_b, "multiplier": 2 }
                ],
                "title": "Weeknight Prep",
                "notes": "Cook **everything** Sunday."
            }),
        )
        .unwrap();

        let slug = out["slug"].as_str().unwrap();
        assert_eq!(slug.len(), 6);
        assert_eq!(
            out["url"].as_str().unwrap(),
            format!("https://recipes.kmicinski.com/t/{}", slug)
        );
        assert_eq!(out["recipe_count"], 2);
        assert_eq!(out["title"], "Weeknight Prep");

        // The published page is durable: deleting the underlying recipe file
        // must not break the snapshot.
        let loaded = shopping::load_published(&state.db, slug).unwrap();
        assert_eq!(loaded.cards.len(), 2);
        assert!(loaded.notes_html.contains("<strong>everything</strong>"));
        let rice_card = loaded.cards.iter().find(|c| c.title == "Rice").unwrap();
        assert_eq!(rice_card.multiplier, 2.0);
        assert!(!rice_card.ingredients.is_empty());
    }

    #[test]
    fn publish_trip_rejects_unknown_keys() {
        let (state, key_a, _) = seeded_state();
        let res = tool_publish_trip(
            state,
            json!({ "selections": [ { "key": key_a }, { "key": "nope00" } ] }),
        );
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("nope00"));
    }

    #[test]
    fn list_and_delete_trips() {
        let (state, key_a, _) = seeded_state();
        let pub_out = tool_publish_trip(
            state.clone(),
            json!({ "selections": [ { "key": key_a } ], "title": "T1" }),
        )
        .unwrap();
        let slug = pub_out["slug"].as_str().unwrap().to_string();

        let listed = tool_list_trips(state.clone()).unwrap();
        assert_eq!(listed["count"], 1);
        assert_eq!(listed["trips"][0]["slug"], slug);

        // delete requires confirm
        assert!(tool_delete_trip(state.clone(), json!({ "slug": slug })).is_err());

        let del = tool_delete_trip(
            state.clone(),
            json!({ "slug": slug, "confirm": true }),
        )
        .unwrap();
        assert_eq!(del["existed"], true);
        assert_eq!(tool_list_trips(state).unwrap()["count"], 0);
    }

    #[test]
    fn set_pantry_round_trips() {
        let (state, _, _) = seeded_state();
        tool_set_pantry(state.clone(), json!({ "name": "  Garlic ", "in_pantry": true })).unwrap();
        let listed = tool_list_pantry(state.clone()).unwrap();
        assert_eq!(listed["pantry"], json!(["garlic"]));
        tool_set_pantry(state.clone(), json!({ "name": "garlic", "in_pantry": false })).unwrap();
        let listed = tool_list_pantry(state).unwrap();
        assert_eq!(listed["count"], 0);
    }

    #[test]
    fn meal_plan_tools_round_trip() {
        let (state, key_a, _) = seeded_state();

        // Plan a recipe meal and a free-text meal in the week of 2026-07-13.
        let plan = tool_plan_meal(
            state.clone(),
            json!({ "date": "2026-07-15", "recipe_key": key_a, "multiplier": 2 }),
        )
        .unwrap();
        assert_eq!(plan["week_start"], "2026-07-13");
        let plan = tool_plan_meal(
            state.clone(),
            json!({ "date": "2026-07-17", "title": "Leftovers" }),
        )
        .unwrap();
        assert_eq!(plan["days"].as_array().unwrap().len(), 7);

        // get_meal_plan for any day of that week sees both meals.
        let got = tool_get_meal_plan(state.clone(), json!({ "week_of": "2026-07-19" })).unwrap();
        assert_eq!(got["week_start"], "2026-07-13");
        let wed = &got["days"][2]; // Mon +2 = Wednesday the 15th
        assert_eq!(wed["date"], "2026-07-15");
        assert_eq!(wed["meals"][0]["title"], "Soup");
        assert_eq!(wed["meals"][0]["multiplier"], 2.0);
        assert_eq!(got["trip"], json!(null));

        // Remove the free-text meal.
        let meal_id = got["days"][4]["meals"][0]["id"].as_str().unwrap().to_string();
        let after = tool_remove_meal(
            state.clone(),
            json!({ "date": "2026-07-17", "meal_id": meal_id }),
        )
        .unwrap();
        assert!(after["days"][4]["meals"].as_array().unwrap().is_empty());

        // Removing it again errors.
        assert!(tool_remove_meal(
            state.clone(),
            json!({ "date": "2026-07-17", "meal_id": "meal_gone" })
        )
        .is_err());

        // Validation: unknown recipe key, missing title/key.
        assert!(tool_plan_meal(state.clone(), json!({ "date": "2026-07-15", "recipe_key": "zzz" })).is_err());
        assert!(tool_plan_meal(state, json!({ "date": "2026-07-15" })).is_err());
    }

    #[test]
    fn plan_notes_and_lock_tools() {
        let (state, _, _) = seeded_state();

        let plan = tool_set_plan_notes(
            state.clone(),
            json!({ "week_of": "2026-07-15", "notes": "## Ideas\n- fish twice" }),
        )
        .unwrap();
        assert_eq!(plan["week_start"], "2026-07-13");
        assert_eq!(plan["notes"], "## Ideas\n- fish twice");
        assert_eq!(plan["locked"], false);
        assert_eq!(plan["week_start_day"], "monday");

        let plan = tool_lock_plan(state.clone(), json!({ "week_of": "2026-07-19", "locked": true })).unwrap();
        assert_eq!(plan["locked"], true);
        assert_eq!(plan["notes"], "## Ideas\n- fish twice"); // notes survive locking

        assert!(tool_lock_plan(state.clone(), json!({ "week_of": "2026-07-19" })).is_err());
        assert!(tool_set_plan_notes(state, json!({ "week_of": "2026-07-15" })).is_err());
    }

    #[test]
    fn week_start_day_tool_rebuckets() {
        let (state, key_a, _) = seeded_state();
        tool_plan_meal(state.clone(), json!({ "date": "2026-07-19", "recipe_key": key_a })).unwrap(); // Sun

        let out = tool_set_week_start_day(state.clone(), json!({ "day": "saturday" })).unwrap();
        assert_eq!(out["week_start_day"], "saturday");

        // The Sunday meal now lives in the Saturday-start week of 07-18.
        let got = tool_get_meal_plan(state.clone(), json!({ "week_of": "2026-07-19" })).unwrap();
        assert_eq!(got["week_start"], "2026-07-18");
        assert_eq!(got["week_start_day"], "saturday");
        assert_eq!(got["days"][1]["meals"][0]["title"], "Soup");

        assert!(tool_set_week_start_day(state, json!({ "day": "someday" })).is_err());
    }

    /// A seeded state whose book_path points at a real two-recipe corpus.
    fn state_with_book() -> (Arc<AppState>, String) {
        let (state, key_a, _) = seeded_state();
        let dir = tempfile::tempdir().unwrap();
        let book_path = dir.path().join("book.jsonl");
        let line = |id: &str, title: &str, protein: &str| {
            format!(
                r###"{{"id":"{id}","title":"{title}","servings":4,"tags":["dinner","grill"],"protein":"{protein}","method":"grill","cuisine":"american","ingredients":[{{"name":"onion","qty":1,"unit":"whole"}}],"body_markdown":"## Prep ahead\nChop.\n\n## Day of (~20 min)\nCook."}}"###
            )
        };
        fs::write(
            &book_path,
            format!(
                "{}\n{}\n",
                line("bk-0001", "Grilled Chicken Bowls", "chicken"),
                line("bk-0002", "Smash Burgers", "beef"),
            ),
        )
        .unwrap();
        std::mem::forget(dir);
        let mut inner = AppState::for_test(state.content_dir.clone(), state.db.clone());
        inner.book_path = book_path;
        (Arc::new(inner), key_a)
    }

    #[test]
    fn book_tools_in_catalog() {
        let names: Vec<String> = tool_catalog()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        for expected in ["browse_book", "read_book_recipe", "promote_book_recipe"] {
            assert!(names.contains(&expected.to_string()), "missing {}", expected);
        }
    }

    #[test]
    fn browse_and_read_book() {
        let (state, _) = state_with_book();
        let out = tool_browse_book(state.clone(), json!({ "query": "chicken" })).unwrap();
        assert_eq!(out["book_size"], 2);
        let results = out["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        // "chicken" is a facet hit for bk-0001 only, so it ranks first.
        assert_eq!(results[0]["id"], "bk-0001");

        let read = tool_read_book_recipe(state.clone(), json!({ "id": "bk-0002" })).unwrap();
        assert_eq!(read["title"], "Smash Burgers");
        assert!(read["body"].as_str().unwrap().contains("Day of"));
        assert_eq!(read["promoted_key"], json!(null));

        assert!(tool_read_book_recipe(state, json!({ "id": "bk-9999" })).is_err());
    }

    #[test]
    fn book_stays_hidden_from_listing_and_search() {
        let (state, _) = state_with_book();
        let listed = tool_list_recipes(state.clone(), json!({})).unwrap();
        assert_eq!(listed["total"], 2); // soup + rice only, no book recipes
        let found = tool_search_recipes(state, json!({ "query": "burgers" })).unwrap();
        assert!(found["results"].as_array().unwrap().is_empty());
    }

    #[test]
    fn plan_and_promote_book_recipe_via_mcp() {
        let (state, _) = state_with_book();

        // Plan a book meal; it shows up with its book_id and snapshotted title.
        let plan = tool_plan_meal(
            state.clone(),
            json!({ "date": "2026-07-15", "book_id": "bk-0001" }),
        )
        .unwrap();
        let wed = &plan["days"][2]["meals"][0];
        assert_eq!(wed["title"], "Grilled Chicken Bowls");
        assert_eq!(wed["book_id"], "bk-0001");
        assert_eq!(wed["recipe_key"], json!(null));

        // Building the week's trip resolves the book recipe's ingredients.
        let trip = tool_build_plan_trip(state.clone(), json!({ "week_of": "2026-07-15" })).unwrap();
        let to_buy = trip["to_buy"].as_array().unwrap();
        assert!(to_buy.iter().any(|i| i["name"] == "onion"));

        // Promote: file created, git-committed key returned, plan rewritten.
        let promoted =
            tool_promote_book_recipe(state.clone(), json!({ "id": "bk-0001" })).unwrap();
        assert_eq!(promoted["already_promoted"], false);
        assert_eq!(promoted["rewritten_meals"], 1);
        let key = promoted["key"].as_str().unwrap().to_string();

        let plan = tool_get_meal_plan(state.clone(), json!({ "week_of": "2026-07-15" })).unwrap();
        let wed = &plan["days"][2]["meals"][0];
        assert_eq!(wed["recipe_key"], key.as_str());
        assert_eq!(wed["book_id"], json!(null));

        // Now a real recipe, visible in the collection.
        let listed = tool_list_recipes(state, json!({ "query": "Grilled" })).unwrap();
        assert_eq!(listed["total"], 1);
    }

    #[test]
    fn build_plan_trip_tool() {
        let (state, key_a, _) = seeded_state();

        // No recipe meals yet → refuses.
        assert!(tool_build_plan_trip(state.clone(), json!({ "week_of": "2026-07-15" })).is_err());

        tool_plan_meal(state.clone(), json!({ "date": "2026-07-15", "recipe_key": key_a, "multiplier": 2 })).unwrap();
        let out = tool_build_plan_trip(state.clone(), json!({ "week_of": "2026-07-15" })).unwrap();
        let trip_id = out["trip_id"].as_str().unwrap();
        assert!(out["trip_url"].as_str().unwrap().contains(trip_id));
        assert_eq!(out["recipes"][0]["multiplier"], 2.0);
        assert!(!out["to_buy"].as_array().unwrap().is_empty());

        // The trip is linked to the plan and is the active trip.
        let got = tool_get_meal_plan(state.clone(), json!({ "week_of": "2026-07-15" })).unwrap();
        assert_eq!(got["trip"]["id"], trip_id);
        assert_eq!(shopping::active_trip(&state.db).unwrap().id, trip_id);
    }
}
