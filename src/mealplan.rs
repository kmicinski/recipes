//! Weekly meal planning.
//!
//! A meal plan is one record per week (keyed by the week's Monday,
//! `YYYY-MM-DD`) holding the meals planned for each day. A meal is either a
//! reference to a recipe (key + snapshotted title + multiplier) or free text
//! ("leftovers", "pizza out"). A plan can be associated with one shopping
//! trip — either built directly from the plan's recipes or linked to an
//! existing saved trip — so "what we planned" and "what we bought for it"
//! stay connected.
//!
//! Storage follows the `shopping.rs` pattern: serde-JSON blobs in a dedicated
//! Sled tree (`meal_plans`).

use crate::models::{Recipe, RecipeSelection};
use crate::shopping;
use chrono::{Datelike, Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use sled::Db;

const PLANS_TREE: &str = "meal_plans";

fn default_multiplier() -> f64 {
    1.0
}

/// One planned meal on a specific day.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlannedMeal {
    /// Unique within the plan; used to remove a specific meal.
    pub id: String,
    /// The day this meal is planned for (`YYYY-MM-DD`, inside the plan's week).
    pub date: String,
    /// Display title. For recipe meals this snapshots the recipe title at
    /// planning time (so the plan still reads sensibly if the recipe is
    /// renamed or deleted); for free-text meals it's the text itself.
    pub title: String,
    /// Present when the meal is a recipe from the collection.
    #[serde(default)]
    pub recipe_key: Option<String>,
    /// Servings multiplier, used when building a shopping trip from the plan.
    #[serde(default = "default_multiplier")]
    pub multiplier: f64,
}

/// A week's meal plan. The Sled key is `week_start`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MealPlan {
    /// Monday of the week, `YYYY-MM-DD`.
    pub week_start: String,
    #[serde(default)]
    pub meals: Vec<PlannedMeal>,
    /// The shopping trip pursued for this week's plan, if one has been
    /// built from it or linked to it.
    #[serde(default)]
    pub trip_id: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

impl MealPlan {
    fn empty(week_start: &str) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            week_start: week_start.to_string(),
            meals: Vec::new(),
            trip_id: None,
            created_at: now.clone(),
            updated_at: now,
        }
    }

    /// Meals for one day, in planning order.
    pub fn meals_on(&self, date: &str) -> Vec<&PlannedMeal> {
        self.meals.iter().filter(|m| m.date == date).collect()
    }

    /// The seven dates of this plan's week, Monday first.
    pub fn week_dates(&self) -> Vec<String> {
        let start = NaiveDate::parse_from_str(&self.week_start, "%Y-%m-%d")
            .expect("week_start is validated on write");
        (0..7)
            .map(|i| (start + Duration::days(i)).format("%Y-%m-%d").to_string())
            .collect()
    }
}

/// Parse a `YYYY-MM-DD` date string.
pub fn parse_date(date: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|_| format!("Invalid date '{}': expected YYYY-MM-DD", date))
}

/// The Monday of the week containing `date`.
pub fn week_start_of(date: &str) -> Result<String, String> {
    let d = parse_date(date)?;
    let monday = d - Duration::days(d.weekday().num_days_from_monday() as i64);
    Ok(monday.format("%Y-%m-%d").to_string())
}

/// Today's date in the server's local timezone.
pub fn today() -> String {
    chrono::Local::now().date_naive().format("%Y-%m-%d").to_string()
}

/// Load the plan for the week containing `week_start` (which must already be
/// a Monday — use [`week_start_of`] to normalize). Returns an empty plan if
/// none has been saved yet.
pub fn load_plan(db: &Db, week_start: &str) -> MealPlan {
    let Ok(tree) = db.open_tree(PLANS_TREE) else {
        return MealPlan::empty(week_start);
    };
    tree.get(week_start.as_bytes())
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_slice(&v).ok())
        .unwrap_or_else(|| MealPlan::empty(week_start))
}

fn save_plan(db: &Db, plan: &mut MealPlan) -> Result<(), String> {
    plan.updated_at = chrono::Utc::now().to_rfc3339();
    let tree = db
        .open_tree(PLANS_TREE)
        .map_err(|e| format!("DB error: {}", e))?;
    let value = serde_json::to_vec(plan).map_err(|e| format!("Serialize error: {}", e))?;
    tree.insert(plan.week_start.as_bytes(), value)
        .map_err(|e| format!("DB error: {}", e))?;
    Ok(())
}

/// Add a meal on `date`. Either `recipe_key` (validated against the loaded
/// recipes; the title is snapshotted from the recipe) or a non-empty free-text
/// `title` is required. Returns the updated week plan.
pub fn add_meal(
    db: &Db,
    recipes: &[Recipe],
    date: &str,
    recipe_key: Option<&str>,
    title: Option<&str>,
    multiplier: f64,
) -> Result<MealPlan, String> {
    let week = week_start_of(date)?;
    let (resolved_key, resolved_title) = match recipe_key.filter(|k| !k.trim().is_empty()) {
        Some(key) => {
            let recipe = recipes
                .iter()
                .find(|r| r.key == key)
                .ok_or_else(|| format!("Unknown recipe key: {}", key))?;
            (Some(recipe.key.clone()), recipe.title.clone())
        }
        None => {
            let t = title.map(str::trim).unwrap_or_default();
            if t.is_empty() {
                return Err("A meal needs a recipe_key or a title".to_string());
            }
            (None, t.to_string())
        }
    };

    let mut plan = load_plan(db, &week);
    let id = format!(
        "meal_{}_{}",
        chrono::Utc::now().timestamp_millis(),
        plan.meals.len()
    );
    plan.meals.push(PlannedMeal {
        id,
        date: date.to_string(),
        title: resolved_title,
        recipe_key: resolved_key,
        multiplier: if multiplier > 0.0 { multiplier } else { 1.0 },
    });
    save_plan(db, &mut plan)?;
    Ok(plan)
}

/// Remove a meal by id from the week containing `date`. Returns the updated
/// plan, or `Ok(None)` if no such meal exists.
pub fn remove_meal(db: &Db, date: &str, meal_id: &str) -> Result<Option<MealPlan>, String> {
    let week = week_start_of(date)?;
    let mut plan = load_plan(db, &week);
    let before = plan.meals.len();
    plan.meals.retain(|m| m.id != meal_id);
    if plan.meals.len() == before {
        return Ok(None);
    }
    save_plan(db, &mut plan)?;
    Ok(Some(plan))
}

/// Associate (or, with `None`, dissociate) a shopping trip with a week's plan.
/// The trip must exist when linking.
pub fn link_trip(db: &Db, week_start: &str, trip_id: Option<&str>) -> Result<MealPlan, String> {
    let week = week_start_of(week_start)?;
    if let Some(id) = trip_id {
        if shopping::load_trip(db, id).is_none() {
            return Err(format!("No such trip: {}", id));
        }
    }
    let mut plan = load_plan(db, &week);
    plan.trip_id = trip_id.map(str::to_string);
    save_plan(db, &mut plan)?;
    Ok(plan)
}

/// The plan's recipe-backed meals as shopping-list selections, with
/// multipliers summed per recipe.
pub fn plan_selections(plan: &MealPlan) -> Vec<RecipeSelection> {
    let mut order: Vec<String> = Vec::new();
    let mut by_key: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for meal in &plan.meals {
        if let Some(key) = &meal.recipe_key {
            if !by_key.contains_key(key) {
                order.push(key.clone());
            }
            *by_key.entry(key.clone()).or_insert(0.0) += meal.multiplier;
        }
    }
    order
        .into_iter()
        .map(|key| {
            let multiplier = by_key[&key];
            RecipeSelection { key, multiplier }
        })
        .collect()
}

/// Build a shopping trip from the week's recipe-backed meals, make it the
/// active trip, and associate it with the plan. Returns the new trip's id.
pub fn build_trip_for_week(
    db: &Db,
    recipes: &[Recipe],
    week_start: &str,
) -> Result<String, String> {
    let week = week_start_of(week_start)?;
    let plan = load_plan(db, &week);
    let selections = plan_selections(&plan);
    if selections.is_empty() {
        return Err("No recipe-based meals planned this week — add some first".to_string());
    }
    let items = shopping::build_shopping_list(&selections, recipes, db);
    let trip_recipes = shopping::resolve_trip_recipes(&selections, recipes);
    let trip_id = shopping::save_trip(db, &items, &trip_recipes)?;
    shopping::set_active_trip(db, &trip_id).ok();
    link_trip(db, &week, Some(&trip_id))?;
    Ok(trip_id)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Ingredient;

    fn temp_db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = sled::open(dir.path().join("db")).unwrap();
        (dir, db)
    }

    fn recipe(key: &str, title: &str, ingredients: Vec<(&str, f64, &str)>) -> Recipe {
        Recipe {
            key: key.to_string(),
            title: title.to_string(),
            servings: Some(4),
            tags: vec![],
            ingredients: ingredients
                .into_iter()
                .map(|(n, q, u)| Ingredient {
                    name: n.to_string(),
                    qty: q,
                    unit: u.to_string(),
                })
                .collect(),
            body_markdown: String::new(),
            body_html: String::new(),
            path: std::path::PathBuf::new(),
            modified: chrono::Utc::now(),
        }
    }

    #[test]
    fn week_start_normalizes_to_monday() {
        assert_eq!(week_start_of("2026-07-13").unwrap(), "2026-07-13"); // Mon
        assert_eq!(week_start_of("2026-07-15").unwrap(), "2026-07-13"); // Wed
        assert_eq!(week_start_of("2026-07-19").unwrap(), "2026-07-13"); // Sun
        assert_eq!(week_start_of("2026-07-20").unwrap(), "2026-07-20"); // next Mon
        assert_eq!(week_start_of("2026-01-01").unwrap(), "2025-12-29"); // year boundary
        assert!(week_start_of("garbage").is_err());
        // chrono is lenient about zero-padding; the output is still canonical.
        assert_eq!(week_start_of("2026-7-3").unwrap(), "2026-06-29");
    }

    #[test]
    fn week_dates_are_the_full_week() {
        let plan = MealPlan::empty("2026-07-13");
        let dates = plan.week_dates();
        assert_eq!(dates.len(), 7);
        assert_eq!(dates[0], "2026-07-13");
        assert_eq!(dates[6], "2026-07-19");
    }

    #[test]
    fn add_meal_snapshots_recipe_title_and_buckets_by_week() {
        let (_dir, db) = temp_db();
        let recipes = vec![recipe("abc123", "Lasagna", vec![("pasta", 1.0, "lb")])];

        let plan = add_meal(&db, &recipes, "2026-07-15", Some("abc123"), None, 2.0).unwrap();
        assert_eq!(plan.week_start, "2026-07-13");
        assert_eq!(plan.meals.len(), 1);
        assert_eq!(plan.meals[0].title, "Lasagna");
        assert_eq!(plan.meals[0].recipe_key.as_deref(), Some("abc123"));
        assert_eq!(plan.meals[0].multiplier, 2.0);

        // Reload round-trips.
        let loaded = load_plan(&db, "2026-07-13");
        assert_eq!(loaded.meals, plan.meals);
        // A different week is a different (empty) plan.
        assert!(load_plan(&db, "2026-07-20").meals.is_empty());
    }

    #[test]
    fn add_meal_free_text_and_validation() {
        let (_dir, db) = temp_db();
        let recipes = vec![];

        let plan = add_meal(&db, &recipes, "2026-07-14", None, Some("Pizza out"), 0.0).unwrap();
        assert_eq!(plan.meals[0].title, "Pizza out");
        assert_eq!(plan.meals[0].recipe_key, None);
        assert_eq!(plan.meals[0].multiplier, 1.0); // <=0 normalized

        assert!(add_meal(&db, &recipes, "2026-07-14", None, Some("   "), 1.0).is_err());
        assert!(add_meal(&db, &recipes, "2026-07-14", Some("nope"), None, 1.0).is_err());
        assert!(add_meal(&db, &recipes, "bad-date", None, Some("x"), 1.0).is_err());
    }

    #[test]
    fn remove_meal_by_id() {
        let (_dir, db) = temp_db();
        let plan = add_meal(&db, &[], "2026-07-14", None, Some("Tacos"), 1.0).unwrap();
        let id = plan.meals[0].id.clone();

        assert!(remove_meal(&db, "2026-07-14", "meal_nonexistent").unwrap().is_none());
        let after = remove_meal(&db, "2026-07-14", &id).unwrap().unwrap();
        assert!(after.meals.is_empty());
        assert!(load_plan(&db, "2026-07-13").meals.is_empty());
    }

    #[test]
    fn meals_on_filters_by_date() {
        let (_dir, db) = temp_db();
        add_meal(&db, &[], "2026-07-14", None, Some("Tacos"), 1.0).unwrap();
        let plan = add_meal(&db, &[], "2026-07-16", None, Some("Soup"), 1.0).unwrap();
        assert_eq!(plan.meals_on("2026-07-14").len(), 1);
        assert_eq!(plan.meals_on("2026-07-16").len(), 1);
        assert_eq!(plan.meals_on("2026-07-15").len(), 0);
    }

    #[test]
    fn plan_selections_aggregates_recipe_meals() {
        let (_dir, db) = temp_db();
        let recipes = vec![
            recipe("aaa", "Lasagna", vec![("pasta", 1.0, "lb")]),
            recipe("bbb", "Soup", vec![("stock", 4.0, "cup")]),
        ];
        add_meal(&db, &recipes, "2026-07-13", Some("aaa"), None, 1.0).unwrap();
        add_meal(&db, &recipes, "2026-07-15", Some("aaa"), None, 0.5).unwrap();
        add_meal(&db, &recipes, "2026-07-16", Some("bbb"), None, 1.0).unwrap();
        let plan = add_meal(&db, &recipes, "2026-07-17", None, Some("Leftovers"), 1.0).unwrap();

        let sels = plan_selections(&plan);
        assert_eq!(sels.len(), 2); // free-text meal excluded
        assert_eq!(sels[0].key, "aaa");
        assert_eq!(sels[0].multiplier, 1.5);
        assert_eq!(sels[1].key, "bbb");
    }

    #[test]
    fn build_trip_for_week_creates_and_links_active_trip() {
        let (_dir, db) = temp_db();
        let recipes = vec![recipe("aaa", "Lasagna", vec![("pasta", 1.0, "lb")])];

        // Empty plan → refuse.
        assert!(build_trip_for_week(&db, &recipes, "2026-07-13").is_err());

        add_meal(&db, &recipes, "2026-07-14", Some("aaa"), None, 2.0).unwrap();
        let trip_id = build_trip_for_week(&db, &recipes, "2026-07-15").unwrap();

        let trip = shopping::load_trip(&db, &trip_id).unwrap();
        assert_eq!(trip.items.len(), 1);
        assert_eq!(trip.items[0].qty, 2.0);
        assert_eq!(trip.recipes[0].key, "aaa");
        assert_eq!(shopping::active_trip(&db).unwrap().id, trip_id);
        assert_eq!(load_plan(&db, "2026-07-13").trip_id.as_deref(), Some(trip_id.as_str()));
    }

    #[test]
    fn link_and_unlink_trip() {
        let (_dir, db) = temp_db();
        assert!(link_trip(&db, "2026-07-13", Some("trip_missing")).is_err());

        let trip_id = shopping::save_trip(&db, &[], &[]).unwrap();
        let plan = link_trip(&db, "2026-07-16", Some(&trip_id)).unwrap();
        assert_eq!(plan.week_start, "2026-07-13"); // normalized
        assert_eq!(plan.trip_id.as_deref(), Some(trip_id.as_str()));

        let plan = link_trip(&db, "2026-07-13", None).unwrap();
        assert_eq!(plan.trip_id, None);
    }
}
