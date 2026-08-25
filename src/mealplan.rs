//! Weekly meal planning.
//!
//! A meal plan is one record per week (keyed by the week's first day,
//! `YYYY-MM-DD`) holding the meals planned for each day. Which weekday a week
//! starts on is a household setting (`week_start_day`, default Monday) stored
//! in the `plan_settings` tree; changing it re-buckets every stored plan. A
//! meal is either a reference to a recipe (key + snapshotted title +
//! multiplier) or free text ("leftovers", "pizza out"). A plan also carries a
//! free-form `notes` scratchpad (the week's brainstorm — sketched ideas
//! before they become concrete meals) and a `locked` flag: once the week is
//! locked in, the UI stops emphasizing the draft/brainstorm and shows the
//! meals themselves (including on the app's home page). A plan can be
//! associated with one shopping trip — either built directly from the plan's
//! recipes or linked to an existing saved trip — so "what we planned" and
//! "what we bought for it" stay connected.
//!
//! Storage follows the `shopping.rs` pattern: serde-JSON blobs in a dedicated
//! Sled tree (`meal_plans`).

use crate::book::BookRecipe;
use crate::models::{Recipe, RecipeSelection};
use crate::shopping;
use chrono::{Datelike, Duration, NaiveDate, Weekday};
use serde::{Deserialize, Serialize};
use sled::Db;

const PLANS_TREE: &str = "meal_plans";
const SETTINGS_TREE: &str = "plan_settings";
const WEEK_START_KEY: &str = "week_start_day";
const STORE_KEY: &str = "instacart_store";

fn default_multiplier() -> f64 {
    1.0
}

fn default_meal_type() -> String {
    "dinner".to_string()
}

pub fn normalize_meal_type(value: &str) -> Result<&'static str, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "breakfast" => Ok("breakfast"),
        "lunch" => Ok("lunch"),
        "dinner" | "meal" | "" => Ok("dinner"),
        _ => Err("Meal type must be breakfast, lunch, or dinner".to_string()),
    }
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
    /// Present when the meal is a hidden-book recipe (id like `bk-0001`) that
    /// hasn't been promoted into the collection. At most one of `recipe_key`
    /// and `book_id` is set; promotion rewrites `book_id` → `recipe_key`.
    #[serde(default)]
    pub book_id: Option<String>,
    /// Servings multiplier, used when building a shopping trip from the plan.
    #[serde(default = "default_multiplier")]
    pub multiplier: f64,
    /// Calendar lane used by the week builder. Defaults to dinner for plans
    /// saved before breakfast/lunch planning was introduced.
    #[serde(default = "default_meal_type")]
    pub meal_type: String,
}

/// A week's meal plan. The Sled key is `week_start`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MealPlan {
    /// First day of the week (per the `week_start_day` setting), `YYYY-MM-DD`.
    pub week_start: String,
    #[serde(default)]
    pub meals: Vec<PlannedMeal>,
    /// Free-form brainstorm scratchpad for the week (markdown) — sketched
    /// ideas, constraints, "wife wants fish twice", etc.
    #[serde(default)]
    pub notes: String,
    /// A locked plan is final: the UI de-emphasizes the brainstorm and
    /// surfaces the meals (plan page + home-page strip).
    #[serde(default)]
    pub locked: bool,
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
            notes: String::new(),
            locked: false,
            trip_id: None,
            created_at: now.clone(),
            updated_at: now,
        }
    }

    /// Meals for one day, in planning order.
    pub fn meals_on(&self, date: &str) -> Vec<&PlannedMeal> {
        self.meals.iter().filter(|m| m.date == date).collect()
    }

    /// The seven dates of this plan's week, `week_start` first.
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

/// Parse a weekday name ("mon", "Monday", …) into a [`Weekday`].
pub fn parse_weekday(s: &str) -> Result<Weekday, String> {
    let t = s.trim().to_lowercase();
    match t.get(..3) {
        Some("mon") => Ok(Weekday::Mon),
        Some("tue") => Ok(Weekday::Tue),
        Some("wed") => Ok(Weekday::Wed),
        Some("thu") => Ok(Weekday::Thu),
        Some("fri") => Ok(Weekday::Fri),
        Some("sat") => Ok(Weekday::Sat),
        Some("sun") => Ok(Weekday::Sun),
        _ => Err(format!(
            "Invalid weekday '{}': expected monday…sunday",
            s.trim()
        )),
    }
}

/// Canonical lowercase name for a weekday, matching what [`parse_weekday`] accepts.
pub fn weekday_name(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "monday",
        Weekday::Tue => "tuesday",
        Weekday::Wed => "wednesday",
        Weekday::Thu => "thursday",
        Weekday::Fri => "friday",
        Weekday::Sat => "saturday",
        Weekday::Sun => "sunday",
    }
}

/// The household's configured first day of the week (default Monday).
pub fn week_start_day(db: &Db) -> Weekday {
    db.open_tree(SETTINGS_TREE)
        .ok()
        .and_then(|t| t.get(WEEK_START_KEY).ok().flatten())
        .and_then(|v| String::from_utf8(v.to_vec()).ok())
        .and_then(|s| parse_weekday(&s).ok())
        .unwrap_or(Weekday::Mon)
}

/// The first day of the week containing `date`, for a week starting on `start`.
pub fn week_start_of(date: &str, start: Weekday) -> Result<String, String> {
    let d = parse_date(date)?;
    let back = (7 + d.weekday().num_days_from_monday() as i64
        - start.num_days_from_monday() as i64)
        % 7;
    Ok((d - Duration::days(back)).format("%Y-%m-%d").to_string())
}

/// [`week_start_of`] using the configured [`week_start_day`].
pub fn week_of(db: &Db, date: &str) -> Result<String, String> {
    week_start_of(date, week_start_day(db))
}

/// Change the first-day-of-week setting and re-bucket every stored plan so
/// each meal lands in the week (per the new start day) containing its date.
/// A plan's notes, lock state, and trip link follow the new week that
/// overlaps the old week the most (notes are concatenated if two old weeks
/// merge), so a round-trip through settings restores them alongside their
/// meals. Returns the number of plans after re-bucketing.
pub fn set_week_start_day(db: &Db, day: Weekday) -> Result<usize, String> {
    let old = week_start_day(db);
    let settings = db
        .open_tree(SETTINGS_TREE)
        .map_err(|e| format!("DB error: {}", e))?;
    settings
        .insert(WEEK_START_KEY, weekday_name(day).as_bytes())
        .map_err(|e| format!("DB error: {}", e))?;
    if old == day {
        let tree = db
            .open_tree(PLANS_TREE)
            .map_err(|e| format!("DB error: {}", e))?;
        return Ok(tree.len());
    }

    let tree = db
        .open_tree(PLANS_TREE)
        .map_err(|e| format!("DB error: {}", e))?;
    let plans: Vec<MealPlan> = tree
        .iter()
        .filter_map(|kv| kv.ok())
        .filter_map(|(_, v)| serde_json::from_slice(&v).ok())
        .collect();

    let mut rebucketed: std::collections::BTreeMap<String, MealPlan> = Default::default();
    for plan in &plans {
        // The new week containing the old start covers `7 - offset` of the old
        // week's days; the next new week covers the remaining `offset`.
        let anchor = week_start_of(&plan.week_start, day)?;
        let offset = (parse_date(&plan.week_start)? - parse_date(&anchor)?).num_days();
        let home = if offset <= 3 {
            anchor
        } else {
            (parse_date(&anchor)? + Duration::days(7))
                .format("%Y-%m-%d")
                .to_string()
        };
        let has_meta = !plan.notes.trim().is_empty() || plan.locked || plan.trip_id.is_some();
        if has_meta {
            let entry = rebucketed
                .entry(home.clone())
                .or_insert_with(|| MealPlan::empty(&home));
            if !plan.notes.trim().is_empty() {
                if entry.notes.trim().is_empty() {
                    entry.notes = plan.notes.clone();
                } else {
                    entry.notes = format!("{}\n\n{}", entry.notes, plan.notes);
                }
            }
            if entry.trip_id.is_none() {
                entry.trip_id = plan.trip_id.clone();
            }
            entry.locked = entry.locked || plan.locked;
            if plan.created_at < entry.created_at {
                entry.created_at = plan.created_at.clone();
            }
        }
        for meal in &plan.meals {
            let week = week_start_of(&meal.date, day)?;
            rebucketed
                .entry(week.clone())
                .or_insert_with(|| MealPlan::empty(&week))
                .meals
                .push(meal.clone());
        }
    }

    tree.clear().map_err(|e| format!("DB error: {}", e))?;
    let count = rebucketed.len();
    for (_, mut plan) in rebucketed {
        save_plan(db, &mut plan)?;
    }
    Ok(count)
}

/// Today's date in the server's local timezone.
pub fn today() -> String {
    chrono::Local::now().date_naive().format("%Y-%m-%d").to_string()
}

/// Load the plan for the week starting at `week_start` (which must already be
/// a week's first day — use [`week_of`] to normalize). Returns an empty plan
/// if none has been saved yet.
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
    add_meal_entry(db, recipes, &[], date, recipe_key, None, title, multiplier)
}

/// [`add_meal`] with hidden-book support: exactly one of `recipe_key`
/// (validated against `recipes`), `book_id` (validated against `book`), or a
/// non-empty free-text `title` identifies the meal; titles are snapshotted.
#[allow(clippy::too_many_arguments)]
pub fn add_meal_entry(
    db: &Db,
    recipes: &[Recipe],
    book: &[BookRecipe],
    date: &str,
    recipe_key: Option<&str>,
    book_id: Option<&str>,
    title: Option<&str>,
    multiplier: f64,
) -> Result<MealPlan, String> {
    add_meal_entry_typed(
        db, recipes, book, date, recipe_key, book_id, title, multiplier, "dinner",
    )
}

#[allow(clippy::too_many_arguments)]
pub fn add_meal_entry_typed(
    db: &Db,
    recipes: &[Recipe],
    book: &[BookRecipe],
    date: &str,
    recipe_key: Option<&str>,
    book_id: Option<&str>,
    title: Option<&str>,
    multiplier: f64,
    meal_type: &str,
) -> Result<MealPlan, String> {
    let week = week_of(db, date)?;
    let meal_type = normalize_meal_type(meal_type)?;
    let recipe_key = recipe_key.filter(|k| !k.trim().is_empty());
    let book_id = book_id.filter(|b| !b.trim().is_empty());
    if recipe_key.is_some() && book_id.is_some() {
        return Err("A meal takes a recipe_key or a book_id, not both".to_string());
    }
    let (resolved_key, resolved_book, resolved_title) = match (recipe_key, book_id) {
        (Some(key), None) => {
            let recipe = recipes
                .iter()
                .find(|r| r.key == key)
                .ok_or_else(|| format!("Unknown recipe key: {}", key))?;
            (Some(recipe.key.clone()), None, recipe.title.clone())
        }
        (None, Some(id)) => {
            let b = book
                .iter()
                .find(|b| b.id == id)
                .ok_or_else(|| format!("Unknown book recipe: {}", id))?;
            (None, Some(b.id.clone()), b.title.clone())
        }
        (None, None) => {
            let t = title.map(str::trim).unwrap_or_default();
            if t.is_empty() {
                return Err("A meal needs a recipe_key, book_id, or title".to_string());
            }
            (None, None, t.to_string())
        }
        (Some(_), Some(_)) => unreachable!("checked above"),
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
        book_id: resolved_book,
        multiplier: if multiplier > 0.0 { multiplier } else { 1.0 },
        meal_type: meal_type.to_string(),
    });
    save_plan(db, &mut plan)?;
    Ok(plan)
}

/// Rewrite every planned meal (across all stored weeks) referencing `book_id`
/// to reference the promoted recipe `new_key` instead. Returns the number of
/// meals rewritten. Called by [`crate::book::promote`].
pub fn rewrite_book_refs(db: &Db, book_id: &str, new_key: &str) -> Result<usize, String> {
    let tree = db
        .open_tree(PLANS_TREE)
        .map_err(|e| format!("DB error: {}", e))?;
    let mut rewritten = 0;
    let plans: Vec<MealPlan> = tree
        .iter()
        .filter_map(|kv| kv.ok())
        .filter_map(|(_, v)| serde_json::from_slice(&v).ok())
        .collect();
    for mut plan in plans {
        let mut touched = false;
        for meal in &mut plan.meals {
            if meal.book_id.as_deref() == Some(book_id) {
                meal.book_id = None;
                meal.recipe_key = Some(new_key.to_string());
                touched = true;
                rewritten += 1;
            }
        }
        if touched {
            save_plan(db, &mut plan)?;
        }
    }
    Ok(rewritten)
}

/// Remove a meal by id from the week containing `date`. Returns the updated
/// plan, or `Ok(None)` if no such meal exists.
pub fn remove_meal(db: &Db, date: &str, meal_id: &str) -> Result<Option<MealPlan>, String> {
    let week = week_of(db, date)?;
    let mut plan = load_plan(db, &week);
    let before = plan.meals.len();
    plan.meals.retain(|m| m.id != meal_id);
    if plan.meals.len() == before {
        return Ok(None);
    }
    save_plan(db, &mut plan)?;
    Ok(Some(plan))
}

/// Replace the brainstorm notes for the week containing `date`.
pub fn set_notes(db: &Db, date: &str, notes: &str) -> Result<MealPlan, String> {
    let week = week_of(db, date)?;
    let mut plan = load_plan(db, &week);
    plan.notes = notes.to_string();
    save_plan(db, &mut plan)?;
    Ok(plan)
}

/// Lock (or unlock) the plan for the week containing `date`. A locked plan
/// is final — the UI shows the meals rather than the brainstorm.
pub fn set_locked(db: &Db, date: &str, locked: bool) -> Result<MealPlan, String> {
    let week = week_of(db, date)?;
    let mut plan = load_plan(db, &week);
    plan.locked = locked;
    save_plan(db, &mut plan)?;
    Ok(plan)
}

/// Associate (or, with `None`, dissociate) a shopping trip with a week's plan.
/// The trip must exist when linking.
pub fn link_trip(db: &Db, week_start: &str, trip_id: Option<&str>) -> Result<MealPlan, String> {
    let week = week_of(db, week_start)?;
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

/// The plan's recipe- and book-backed meals as shopping-list selections, with
/// multipliers summed per key. Book meals use their `bk-…` id as the key;
/// resolve them via [`crate::book::augment`].
pub fn plan_selections(plan: &MealPlan) -> Vec<RecipeSelection> {
    let mut order: Vec<String> = Vec::new();
    let mut by_key: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for meal in &plan.meals {
        if let Some(key) = meal.recipe_key.as_ref().or(meal.book_id.as_ref()) {
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

/// Build a shopping trip from the week's recipe- and book-backed meals, make
/// it the active trip, and associate it with the plan. Returns the new trip's
/// id.
pub fn build_trip_for_week(
    db: &Db,
    recipes: &[Recipe],
    book: &[BookRecipe],
    week_start: &str,
) -> Result<String, String> {
    let week = week_of(db, week_start)?;
    let plan = load_plan(db, &week);
    let selections = plan_selections(&plan);
    if selections.is_empty() {
        return Err("No recipe-based meals planned this week — add some first".to_string());
    }
    let all = crate::book::augment(recipes, book, &selections);
    let items = shopping::build_shopping_list(&selections, &all, db);
    let trip_recipes = shopping::resolve_trip_recipes(&selections, &all);
    let trip_id = shopping::save_trip(db, &items, &trip_recipes)?;
    shopping::set_active_trip(db, &trip_id).ok();
    link_trip(db, &week, Some(&trip_id))?;
    Ok(trip_id)
}

/// The household's preferred Instacart store for the Shop-with-Claude block.
pub fn preferred_store(db: &Db) -> shopping::Store {
    db.open_tree(SETTINGS_TREE)
        .ok()
        .and_then(|t| t.get(STORE_KEY).ok().flatten())
        .and_then(|v| String::from_utf8(v.to_vec()).ok())
        .and_then(|s| shopping::parse_store(&s).ok())
        .unwrap_or(shopping::Store::Aldi)
}

/// Persist the preferred Instacart store (household-wide, like the week
/// start day).
pub fn set_preferred_store(db: &Db, store: shopping::Store) -> Result<(), String> {
    db.open_tree(SETTINGS_TREE)
        .map_err(|e| format!("DB error: {}", e))?
        .insert(STORE_KEY, shopping::store_name(store).as_bytes())
        .map_err(|e| format!("DB error: {}", e))?;
    Ok(())
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
    fn week_start_normalizes_to_monday_by_default() {
        let m = Weekday::Mon;
        assert_eq!(week_start_of("2026-07-13", m).unwrap(), "2026-07-13"); // Mon
        assert_eq!(week_start_of("2026-07-15", m).unwrap(), "2026-07-13"); // Wed
        assert_eq!(week_start_of("2026-07-19", m).unwrap(), "2026-07-13"); // Sun
        assert_eq!(week_start_of("2026-07-20", m).unwrap(), "2026-07-20"); // next Mon
        assert_eq!(week_start_of("2026-01-01", m).unwrap(), "2025-12-29"); // year boundary
        assert!(week_start_of("garbage", m).is_err());
        // chrono is lenient about zero-padding; the output is still canonical.
        assert_eq!(week_start_of("2026-7-3", m).unwrap(), "2026-06-29");
    }

    #[test]
    fn week_start_honors_arbitrary_start_days() {
        // 2026-07-19 is a Sunday.
        assert_eq!(week_start_of("2026-07-19", Weekday::Sun).unwrap(), "2026-07-19");
        assert_eq!(week_start_of("2026-07-19", Weekday::Sat).unwrap(), "2026-07-18");
        assert_eq!(week_start_of("2026-07-19", Weekday::Wed).unwrap(), "2026-07-15");
        // Wednesday, Wed-start: is its own week start.
        assert_eq!(week_start_of("2026-07-15", Weekday::Wed).unwrap(), "2026-07-15");
        // Tuesday, Wed-start: previous Wednesday.
        assert_eq!(week_start_of("2026-07-14", Weekday::Wed).unwrap(), "2026-07-08");
    }

    #[test]
    fn parse_weekday_and_names_round_trip() {
        for day in [
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
            Weekday::Sat,
            Weekday::Sun,
        ] {
            assert_eq!(parse_weekday(weekday_name(day)).unwrap(), day);
        }
        assert_eq!(parse_weekday("SAT").unwrap(), Weekday::Sat);
        assert_eq!(parse_weekday(" Sunday ").unwrap(), Weekday::Sun);
        assert!(parse_weekday("noday").is_err());
        assert!(parse_weekday("").is_err());
    }

    #[test]
    fn week_start_day_setting_persists_and_rebuckets() {
        let (_dir, db) = temp_db();
        assert_eq!(week_start_day(&db), Weekday::Mon);

        // Plan a Mon-start week: meals on Wed and Sun, notes, locked.
        add_meal(&db, &[], "2026-07-15", None, Some("Tacos"), 1.0).unwrap(); // Wed
        add_meal(&db, &[], "2026-07-19", None, Some("Soup"), 1.0).unwrap(); // Sun
        set_notes(&db, "2026-07-15", "fish twice this week").unwrap();
        set_locked(&db, "2026-07-15", true).unwrap();

        // Switch to Saturday-start weeks.
        set_week_start_day(&db, Weekday::Sat).unwrap();
        assert_eq!(week_start_day(&db), Weekday::Sat);

        // Wed 07-15 now belongs to the Sat 07-11 week; Sun 07-19 to Sat 07-18.
        let wk1 = load_plan(&db, "2026-07-11");
        assert_eq!(wk1.meals.len(), 1);
        assert_eq!(wk1.meals[0].title, "Tacos");
        // Notes + lock followed the week containing the old start date (Mon 07-13).
        assert_eq!(wk1.notes, "fish twice this week");
        assert!(wk1.locked);
        let wk2 = load_plan(&db, "2026-07-18");
        assert_eq!(wk2.meals.len(), 1);
        assert_eq!(wk2.meals[0].title, "Soup");
        assert!(!wk2.locked);

        // week_of now buckets by Saturday.
        assert_eq!(week_of(&db, "2026-07-19").unwrap(), "2026-07-18");

        // Switching back re-merges everything into the Monday week.
        set_week_start_day(&db, Weekday::Mon).unwrap();
        let back = load_plan(&db, "2026-07-13");
        assert_eq!(back.meals.len(), 2);
        assert_eq!(back.notes, "fish twice this week");
        assert!(back.locked);
    }

    #[test]
    fn notes_and_lock_round_trip() {
        let (_dir, db) = temp_db();
        let plan = set_notes(&db, "2026-07-15", "ideas: pasta, curry").unwrap();
        assert_eq!(plan.week_start, "2026-07-13");
        assert_eq!(plan.notes, "ideas: pasta, curry");
        assert!(!plan.locked);

        let plan = set_locked(&db, "2026-07-19", true).unwrap();
        assert_eq!(plan.week_start, "2026-07-13"); // same week
        assert!(plan.locked);
        assert_eq!(plan.notes, "ideas: pasta, curry"); // notes preserved

        let plan = set_locked(&db, "2026-07-13", false).unwrap();
        assert!(!plan.locked);
        assert!(set_notes(&db, "bad-date", "x").is_err());
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
    fn typed_meals_validate_and_persist() {
        let (_dir, db) = temp_db();
        let plan = add_meal_entry_typed(
            &db, &[], &[], "2026-07-13", None, None, Some("Oats"), 1.0,
            "breakfast",
        )
        .unwrap();
        assert_eq!(plan.meals[0].meal_type, "breakfast");
        assert!(add_meal_entry_typed(
            &db, &[], &[], "2026-07-14", None, None, Some("Snack"), 1.0,
            "snack",
        )
        .is_err());
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
        assert!(build_trip_for_week(&db, &recipes, &[], "2026-07-13").is_err());

        add_meal(&db, &recipes, "2026-07-14", Some("aaa"), None, 2.0).unwrap();
        let trip_id = build_trip_for_week(&db, &recipes, &[], "2026-07-15").unwrap();

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
