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
use crate::recipes::{generate_key, git_commit, git_rm_commit, serialize_recipe};
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

fn validate_filename(filename: &str) -> Result<(), String> {
    if !filename.ends_with(".md") {
        return Err("filename must end with .md".into());
    }
    if filename.contains("..")
        || filename.contains('/')
        || filename.contains('\\')
        || filename.contains('\0')
        || filename.starts_with('.')
    {
        return Err("filename must be a single .md segment".into());
    }
    Ok(())
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

        let state = Arc::new(AppState {
            content_dir,
            db,
            mcp_token: Some("test-token".to_string()),
        });
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
}
