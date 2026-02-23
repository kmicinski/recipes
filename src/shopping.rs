//! Shopping list aggregation logic.
//!
//! Takes recipe selections with multipliers and produces an aggregated
//! shopping list, annotated with pantry status.

use crate::models::{Ingredient, Recipe, RecipeSelection, ShoppingItem};
use crate::pantry;
use serde::{Deserialize, Serialize};
use sled::Db;
use std::collections::{BTreeMap, HashMap};

const TRIPS_TREE: &str = "shopping_trips";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TripRecipe {
    pub key: String,
    pub title: String,
    pub multiplier: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedTrip {
    pub id: String,
    pub items: Vec<ShoppingItem>,
    #[serde(default)]
    pub recipes: Vec<TripRecipe>,
    #[serde(default)]
    pub instacart_products_link_url: Option<String>,
    #[serde(default)]
    pub instacart_products_link_fingerprint: Option<String>,
    pub created_at: String,
}

fn normalized_multiplier(multiplier: f64) -> f64 {
    if multiplier <= 0.0 {
        1.0
    } else {
        multiplier
    }
}

/// Resolve selected recipes into a persisted trip recipe summary.
pub fn resolve_trip_recipes(selections: &[RecipeSelection], recipes: &[Recipe]) -> Vec<TripRecipe> {
    let recipe_map: HashMap<&str, &Recipe> = recipes.iter().map(|r| (r.key.as_str(), r)).collect();
    let mut resolved: Vec<TripRecipe> = Vec::new();
    let mut index_by_key: HashMap<String, usize> = HashMap::new();

    for sel in selections {
        let Some(recipe) = recipe_map.get(sel.key.as_str()) else {
            continue;
        };
        let multiplier = normalized_multiplier(sel.multiplier);

        if let Some(idx) = index_by_key.get(recipe.key.as_str()) {
            resolved[*idx].multiplier += multiplier;
            continue;
        }

        let idx = resolved.len();
        index_by_key.insert(recipe.key.clone(), idx);
        resolved.push(TripRecipe {
            key: recipe.key.clone(),
            title: recipe.title.clone(),
            multiplier,
        });
    }

    resolved
}

/// Save a shopping trip to the database. Returns the trip ID.
pub fn save_trip(
    db: &Db,
    items: &[ShoppingItem],
    recipes: &[TripRecipe],
) -> Result<String, String> {
    let now = chrono::Utc::now();
    let id = format!("trip_{}", now.timestamp_millis());
    let trip = SavedTrip {
        id: id.clone(),
        items: items.to_vec(),
        recipes: recipes.to_vec(),
        instacart_products_link_url: None,
        instacart_products_link_fingerprint: None,
        created_at: now.to_rfc3339(),
    };
    save_trip_record(db, &trip)?;
    Ok(id)
}

/// Insert or update a saved trip record.
pub fn save_trip_record(db: &Db, trip: &SavedTrip) -> Result<(), String> {
    let tree = db
        .open_tree(TRIPS_TREE)
        .map_err(|e| format!("DB error: {}", e))?;
    let value = serde_json::to_vec(trip).map_err(|e| format!("Serialize error: {}", e))?;
    tree.insert(trip.id.as_bytes(), value)
        .map_err(|e| format!("DB error: {}", e))?;
    Ok(())
}

/// Load a single saved trip by ID.
pub fn load_trip(db: &Db, id: &str) -> Option<SavedTrip> {
    let tree = db.open_tree(TRIPS_TREE).ok()?;
    let bytes = tree.get(id.as_bytes()).ok()??;
    serde_json::from_slice(&bytes).ok()
}

/// List recent saved trips, most recent first, limited to 10.
pub fn list_trips(db: &Db) -> Vec<SavedTrip> {
    let tree = match db.open_tree(TRIPS_TREE) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let mut trips: Vec<SavedTrip> = tree
        .iter()
        .filter_map(|r| r.ok())
        .filter_map(|(_, v)| serde_json::from_slice(&v).ok())
        .collect();
    // Sort by created_at descending (newest first)
    trips.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    trips.truncate(10);
    trips
}

const INSTACART_STOP_WORDS: &[&str] = &[
    "fresh", "large", "small", "medium", "optional", "chopped", "diced", "minced", "sliced", "to",
    "taste", "boneless", "skinless",
];

/// Build a best-effort query string for Instacart search.
pub fn instacart_search_query(name: &str) -> String {
    let mut normalized = name
        .to_lowercase()
        .replace('&', " ")
        .replace('/', " ")
        .replace('-', " ");
    normalized = normalized.replace("scallions", "green onions");
    normalized = normalized.replace("scallion", "green onion");
    normalized = normalized.replace("confectioners sugar", "powdered sugar");
    let cleaned: String = normalized
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c.is_ascii_whitespace() {
                c
            } else {
                ' '
            }
        })
        .collect();

    let mut tokens = Vec::new();
    for token in cleaned.split_whitespace() {
        if INSTACART_STOP_WORDS.contains(&token) {
            continue;
        }
        let rewritten = match token {
            "courgette" => "zucchini",
            "aubergine" => "eggplant",
            "capsicum" => "pepper",
            _ => token,
        };
        tokens.push(rewritten);
    }

    if tokens.is_empty() {
        return name.trim().to_lowercase();
    }

    tokens.join(" ")
}

/// Build an Instacart search URL for an ingredient.
pub fn instacart_search_url(name: &str) -> String {
    let query = instacart_search_query(name);
    format!(
        "https://www.instacart.com/store/search?searchTerm={}",
        percent_encode_query_component(&query)
    )
}

fn percent_encode_query_component(raw: &str) -> String {
    let mut encoded = String::new();
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{:02X}", byte)),
        }
    }
    encoded
}

/// Key for aggregating ingredients: (normalized name, unit).
fn agg_key(ing: &Ingredient) -> (String, String) {
    (
        ing.name.trim().to_lowercase(),
        ing.unit.trim().to_lowercase(),
    )
}

/// Build an aggregated shopping list from recipe selections.
pub fn build_shopping_list(
    selections: &[RecipeSelection],
    recipes: &[Recipe],
    db: &Db,
) -> Vec<ShoppingItem> {
    let recipe_map: BTreeMap<&str, &Recipe> = recipes.iter().map(|r| (r.key.as_str(), r)).collect();

    // Aggregate: (normalized_name, unit) -> (qty, display_name, sources)
    let mut agg: BTreeMap<(String, String), (f64, String, Vec<String>)> = BTreeMap::new();

    for sel in selections {
        let multiplier = normalized_multiplier(sel.multiplier);

        if let Some(recipe) = recipe_map.get(sel.key.as_str()) {
            for ing in &recipe.ingredients {
                let key = agg_key(ing);
                let entry = agg
                    .entry(key)
                    .or_insert_with(|| (0.0, ing.name.clone(), Vec::new()));
                entry.0 += ing.qty * multiplier;
                if !entry.2.contains(&recipe.title) {
                    entry.2.push(recipe.title.clone());
                }
            }
        }
    }

    agg.into_iter()
        .map(|((norm_name, unit), (qty, display_name, sources))| {
            let in_pantry = pantry::has(db, &norm_name);
            ShoppingItem {
                name: display_name,
                qty,
                unit,
                in_pantry,
                sources,
            }
        })
        .collect()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Ingredient;
    use chrono::Utc;
    use std::path::PathBuf;

    fn temp_db() -> Db {
        let dir = tempfile::tempdir().unwrap();
        sled::open(dir.path()).unwrap()
    }

    fn make_recipe(key: &str, title: &str, ingredients: Vec<Ingredient>) -> Recipe {
        Recipe {
            key: key.to_string(),
            title: title.to_string(),
            servings: Some(4),
            tags: vec![],
            ingredients,
            body_markdown: String::new(),
            body_html: String::new(),
            path: PathBuf::from(format!("{}.md", key)),
            modified: Utc::now(),
        }
    }

    #[test]
    fn test_empty_selections() {
        let db = temp_db();
        let result = build_shopping_list(&[], &[], &db);
        assert!(result.is_empty());
    }

    #[test]
    fn test_single_recipe() {
        let db = temp_db();
        let recipes = vec![make_recipe(
            "abc",
            "Pasta",
            vec![
                Ingredient {
                    name: "pasta".into(),
                    qty: 500.0,
                    unit: "g".into(),
                },
                Ingredient {
                    name: "tomato sauce".into(),
                    qty: 1.0,
                    unit: "jar".into(),
                },
            ],
        )];
        let selections = vec![RecipeSelection {
            key: "abc".into(),
            multiplier: 1.0,
        }];

        let items = build_shopping_list(&selections, &recipes, &db);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].name, "pasta");
        assert_eq!(items[0].qty, 500.0);
        assert!(!items[0].in_pantry);
    }

    #[test]
    fn test_multiplier_scaling() {
        let db = temp_db();
        let recipes = vec![make_recipe(
            "abc",
            "Rice",
            vec![Ingredient {
                name: "rice".into(),
                qty: 200.0,
                unit: "g".into(),
            }],
        )];
        let selections = vec![RecipeSelection {
            key: "abc".into(),
            multiplier: 3.0,
        }];

        let items = build_shopping_list(&selections, &recipes, &db);
        assert_eq!(items[0].qty, 600.0);
    }

    #[test]
    fn test_aggregation_across_recipes() {
        let db = temp_db();
        let recipes = vec![
            make_recipe(
                "a",
                "Recipe A",
                vec![
                    Ingredient {
                        name: "olive oil".into(),
                        qty: 2.0,
                        unit: "tbsp".into(),
                    },
                    Ingredient {
                        name: "garlic".into(),
                        qty: 3.0,
                        unit: "cloves".into(),
                    },
                ],
            ),
            make_recipe(
                "b",
                "Recipe B",
                vec![
                    Ingredient {
                        name: "Olive Oil".into(),
                        qty: 1.0,
                        unit: "tbsp".into(),
                    },
                    Ingredient {
                        name: "onion".into(),
                        qty: 1.0,
                        unit: "whole".into(),
                    },
                ],
            ),
        ];
        let selections = vec![
            RecipeSelection {
                key: "a".into(),
                multiplier: 1.0,
            },
            RecipeSelection {
                key: "b".into(),
                multiplier: 1.0,
            },
        ];

        let items = build_shopping_list(&selections, &recipes, &db);

        // olive oil should be aggregated (3 tbsp total)
        let oil = items
            .iter()
            .find(|i| i.name.to_lowercase().contains("olive oil"))
            .unwrap();
        assert_eq!(oil.qty, 3.0);
        assert_eq!(oil.sources.len(), 2);

        // garlic and onion separate
        assert!(items.iter().any(|i| i.name.to_lowercase() == "garlic"));
        assert!(items.iter().any(|i| i.name.to_lowercase() == "onion"));
    }

    #[test]
    fn test_pantry_filtering() {
        let db = temp_db();
        pantry::add(&db, "salt").unwrap();

        let recipes = vec![make_recipe(
            "a",
            "Soup",
            vec![
                Ingredient {
                    name: "Salt".into(),
                    qty: 1.0,
                    unit: "tsp".into(),
                },
                Ingredient {
                    name: "pepper".into(),
                    qty: 0.5,
                    unit: "tsp".into(),
                },
            ],
        )];
        let selections = vec![RecipeSelection {
            key: "a".into(),
            multiplier: 1.0,
        }];

        let items = build_shopping_list(&selections, &recipes, &db);
        let salt = items.iter().find(|i| i.name == "Salt").unwrap();
        assert!(salt.in_pantry);
        let pepper = items.iter().find(|i| i.name == "pepper").unwrap();
        assert!(!pepper.in_pantry);
    }

    #[test]
    fn test_source_tracking() {
        let db = temp_db();
        let recipes = vec![
            make_recipe(
                "a",
                "Dish A",
                vec![Ingredient {
                    name: "butter".into(),
                    qty: 50.0,
                    unit: "g".into(),
                }],
            ),
            make_recipe(
                "b",
                "Dish B",
                vec![Ingredient {
                    name: "Butter".into(),
                    qty: 30.0,
                    unit: "g".into(),
                }],
            ),
        ];
        let selections = vec![
            RecipeSelection {
                key: "a".into(),
                multiplier: 1.0,
            },
            RecipeSelection {
                key: "b".into(),
                multiplier: 1.0,
            },
        ];

        let items = build_shopping_list(&selections, &recipes, &db);
        let butter = items
            .iter()
            .find(|i| i.name.to_lowercase() == "butter")
            .unwrap();
        assert_eq!(butter.sources, vec!["Dish A", "Dish B"]);
        assert_eq!(butter.qty, 80.0);
    }

    #[test]
    fn test_nonexistent_recipe_key() {
        let db = temp_db();
        let recipes = vec![make_recipe(
            "a",
            "Soup",
            vec![Ingredient {
                name: "water".into(),
                qty: 1.0,
                unit: "l".into(),
            }],
        )];
        // Reference a key that doesn't exist
        let selections = vec![
            RecipeSelection {
                key: "a".into(),
                multiplier: 1.0,
            },
            RecipeSelection {
                key: "zzz".into(),
                multiplier: 1.0,
            },
        ];

        let items = build_shopping_list(&selections, &recipes, &db);
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn test_zero_multiplier_defaults_to_one() {
        let db = temp_db();
        let recipes = vec![make_recipe(
            "a",
            "Test",
            vec![Ingredient {
                name: "flour".into(),
                qty: 100.0,
                unit: "g".into(),
            }],
        )];
        let selections = vec![RecipeSelection {
            key: "a".into(),
            multiplier: 0.0,
        }];

        let items = build_shopping_list(&selections, &recipes, &db);
        assert_eq!(items[0].qty, 100.0);
    }

    #[test]
    fn test_resolve_trip_recipes_tracks_keys_and_titles() {
        let recipes = vec![
            make_recipe("a", "Dish A", vec![]),
            make_recipe("b", "Dish B", vec![]),
        ];
        let selections = vec![
            RecipeSelection {
                key: "a".into(),
                multiplier: 2.0,
            },
            RecipeSelection {
                key: "b".into(),
                multiplier: 1.0,
            },
            RecipeSelection {
                key: "a".into(),
                multiplier: 0.0,
            },
            RecipeSelection {
                key: "zzz".into(),
                multiplier: 9.0,
            },
        ];

        let resolved = resolve_trip_recipes(&selections, &recipes);

        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].key, "a");
        assert_eq!(resolved[0].title, "Dish A");
        assert_eq!(resolved[0].multiplier, 3.0);
        assert_eq!(resolved[1].key, "b");
    }

    #[test]
    fn test_save_and_load_trip_with_recipes() {
        let db = temp_db();
        let items = vec![ShoppingItem {
            name: "milk".into(),
            qty: 1.0,
            unit: "carton".into(),
            in_pantry: false,
            sources: vec!["Dish A".into()],
        }];
        let recipes = vec![TripRecipe {
            key: "a".into(),
            title: "Dish A".into(),
            multiplier: 2.0,
        }];

        let id = save_trip(&db, &items, &recipes).unwrap();
        let trip = load_trip(&db, &id).unwrap();

        assert_eq!(trip.items.len(), 1);
        assert_eq!(trip.recipes, recipes);
        assert!(trip.instacart_products_link_url.is_none());
        assert!(trip.instacart_products_link_fingerprint.is_none());
    }

    #[test]
    fn test_load_legacy_trip_defaults_recipes_to_empty() {
        let db = temp_db();
        let tree = db.open_tree(TRIPS_TREE).unwrap();
        let legacy = serde_json::json!({
            "id": "trip_legacy",
            "items": [],
            "created_at": "2026-01-01T00:00:00Z"
        });
        tree.insert(b"trip_legacy", serde_json::to_vec(&legacy).unwrap())
            .unwrap();

        let trip = load_trip(&db, "trip_legacy").unwrap();
        assert!(trip.recipes.is_empty());
        assert!(trip.instacart_products_link_url.is_none());
        assert!(trip.instacart_products_link_fingerprint.is_none());
    }

    #[test]
    fn test_instacart_query_normalization() {
        assert_eq!(instacart_search_query("Fresh Scallions"), "green onions");
        assert_eq!(
            instacart_search_query("Boneless skinless chicken"),
            "chicken"
        );
    }

    #[test]
    fn test_instacart_search_url_encoding() {
        let url = instacart_search_url("Red bell pepper");
        assert_eq!(
            url,
            "https://www.instacart.com/store/search?searchTerm=red+bell+pepper"
        );
    }

    #[test]
    fn test_save_trip_record_updates_existing_trip() {
        let db = temp_db();
        let items = vec![ShoppingItem {
            name: "milk".into(),
            qty: 1.0,
            unit: "carton".into(),
            in_pantry: false,
            sources: vec![],
        }];
        let id = save_trip(&db, &items, &[]).unwrap();
        let mut trip = load_trip(&db, &id).unwrap();
        trip.instacart_products_link_url = Some("https://connect.instacart.com/foo".into());
        trip.instacart_products_link_fingerprint = Some("abc".into());

        save_trip_record(&db, &trip).unwrap();
        let loaded = load_trip(&db, &id).unwrap();
        assert_eq!(
            loaded.instacart_products_link_url.as_deref(),
            Some("https://connect.instacart.com/foo")
        );
        assert_eq!(
            loaded.instacart_products_link_fingerprint.as_deref(),
            Some("abc")
        );
    }
}
