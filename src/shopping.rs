//! Shopping list aggregation logic.
//!
//! Takes recipe selections with multipliers and produces an aggregated
//! shopping list, annotated with pantry status.

use crate::models::{Ingredient, Recipe, RecipeSelection, ShoppingItem};
use crate::pantry;
use serde::{Deserialize, Serialize};
use sled::Db;
use std::collections::BTreeMap;

const TRIPS_TREE: &str = "shopping_trips";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedTrip {
    pub id: String,
    pub items: Vec<ShoppingItem>,
    pub created_at: String,
}

/// Save a shopping trip to the database. Returns the trip ID.
pub fn save_trip(db: &Db, items: &[ShoppingItem]) -> Result<String, String> {
    let tree = db
        .open_tree(TRIPS_TREE)
        .map_err(|e| format!("DB error: {}", e))?;
    let now = chrono::Utc::now();
    let id = format!("trip_{}", now.timestamp_millis());
    let trip = SavedTrip {
        id: id.clone(),
        items: items.to_vec(),
        created_at: now.to_rfc3339(),
    };
    let value = serde_json::to_vec(&trip).map_err(|e| format!("Serialize error: {}", e))?;
    tree.insert(id.as_bytes(), value)
        .map_err(|e| format!("DB error: {}", e))?;
    Ok(id)
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
        let multiplier = if sel.multiplier <= 0.0 {
            1.0
        } else {
            sel.multiplier
        };

        if let Some(recipe) = recipe_map.get(sel.key.as_str()) {
            for ing in &recipe.ingredients {
                let key = agg_key(ing);
                let entry = agg.entry(key).or_insert_with(|| {
                    (0.0, ing.name.clone(), Vec::new())
                });
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
                Ingredient { name: "pasta".into(), qty: 500.0, unit: "g".into() },
                Ingredient { name: "tomato sauce".into(), qty: 1.0, unit: "jar".into() },
            ],
        )];
        let selections = vec![RecipeSelection { key: "abc".into(), multiplier: 1.0 }];

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
            vec![Ingredient { name: "rice".into(), qty: 200.0, unit: "g".into() }],
        )];
        let selections = vec![RecipeSelection { key: "abc".into(), multiplier: 3.0 }];

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
                    Ingredient { name: "olive oil".into(), qty: 2.0, unit: "tbsp".into() },
                    Ingredient { name: "garlic".into(), qty: 3.0, unit: "cloves".into() },
                ],
            ),
            make_recipe(
                "b",
                "Recipe B",
                vec![
                    Ingredient { name: "Olive Oil".into(), qty: 1.0, unit: "tbsp".into() },
                    Ingredient { name: "onion".into(), qty: 1.0, unit: "whole".into() },
                ],
            ),
        ];
        let selections = vec![
            RecipeSelection { key: "a".into(), multiplier: 1.0 },
            RecipeSelection { key: "b".into(), multiplier: 1.0 },
        ];

        let items = build_shopping_list(&selections, &recipes, &db);

        // olive oil should be aggregated (3 tbsp total)
        let oil = items.iter().find(|i| i.name.to_lowercase().contains("olive oil")).unwrap();
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
                Ingredient { name: "Salt".into(), qty: 1.0, unit: "tsp".into() },
                Ingredient { name: "pepper".into(), qty: 0.5, unit: "tsp".into() },
            ],
        )];
        let selections = vec![RecipeSelection { key: "a".into(), multiplier: 1.0 }];

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
            make_recipe("a", "Dish A", vec![
                Ingredient { name: "butter".into(), qty: 50.0, unit: "g".into() },
            ]),
            make_recipe("b", "Dish B", vec![
                Ingredient { name: "Butter".into(), qty: 30.0, unit: "g".into() },
            ]),
        ];
        let selections = vec![
            RecipeSelection { key: "a".into(), multiplier: 1.0 },
            RecipeSelection { key: "b".into(), multiplier: 1.0 },
        ];

        let items = build_shopping_list(&selections, &recipes, &db);
        let butter = items.iter().find(|i| i.name.to_lowercase() == "butter").unwrap();
        assert_eq!(butter.sources, vec!["Dish A", "Dish B"]);
        assert_eq!(butter.qty, 80.0);
    }

    #[test]
    fn test_nonexistent_recipe_key() {
        let db = temp_db();
        let recipes = vec![make_recipe("a", "Soup", vec![
            Ingredient { name: "water".into(), qty: 1.0, unit: "l".into() },
        ])];
        // Reference a key that doesn't exist
        let selections = vec![
            RecipeSelection { key: "a".into(), multiplier: 1.0 },
            RecipeSelection { key: "zzz".into(), multiplier: 1.0 },
        ];

        let items = build_shopping_list(&selections, &recipes, &db);
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn test_zero_multiplier_defaults_to_one() {
        let db = temp_db();
        let recipes = vec![make_recipe("a", "Test", vec![
            Ingredient { name: "flour".into(), qty: 100.0, unit: "g".into() },
        ])];
        let selections = vec![RecipeSelection { key: "a".into(), multiplier: 0.0 }];

        let items = build_shopping_list(&selections, &recipes, &db);
        assert_eq!(items[0].qty, 100.0);
    }
}
