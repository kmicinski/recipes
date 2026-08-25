//! The hidden recipe book: a large pre-generated corpus backing "meal builder
//! mode" on the weekly plan.
//!
//! The book is a single JSONL file (`book/book.jsonl` by default, overridable
//! via `BOOK_PATH`) of 500 freshly randomized meal-kit-style recipes — minimal prep/cleanup,
//! ~20 minutes day-of assuming weekend batch prep. Book recipes are **not**
//! part of the collection: they never touch `content/`, so the home page,
//! search, the meal picker, and the MCP `list_recipes`/`search_recipes` tools
//! cannot see them. They surface in exactly four places: the hot-or-not deck
//! on the plan page, `/book/{id}` pages, plan chips for picked meals, and
//! shopping-list aggregation for those meals. A book recipe becomes a real
//! git-committed recipe only through [`promote`].
//!
//! Book ids look like `bk-0001`. Real recipe keys are 6 lowercase hex chars,
//! so the two keyspaces are disjoint — a book recipe can masquerade as a
//! [`Recipe`] (see [`to_recipe`]) and flow through the existing shopping
//! pipeline without touching `shopping.rs`.

use crate::mealplan::MealPlan;
use crate::models::{Ingredient, Recipe, RecipeSelection};
use crate::{mealplan, recipes};
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use sled::Db;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

const SKIPS_TREE: &str = "book_skips";
const PROMOTIONS_TREE: &str = "book_promotions";

/// Prefix that keeps book ids disjoint from 6-hex-char recipe keys.
pub const BOOK_ID_PREFIX: &str = "bk-";

// ============================================================================
// Corpus
// ============================================================================

/// One recipe in the hidden book.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BookRecipe {
    /// `bk-NNNN`; enforced at parse time.
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub servings: Option<u32>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub ingredients: Vec<Ingredient>,
    pub body_markdown: String,
    /// Facets used for scoring and deck diversity. Also expected to appear in
    /// `tags` so nothing is lost on promotion (frontmatter has no facet keys).
    #[serde(default)]
    pub protein: String,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub cuisine: String,
}

/// Parse a JSONL corpus. Malformed lines and ids without the `bk-` prefix are
/// dropped (never fatal — a bad line loses one recipe, not the book).
pub fn parse_jsonl(text: &str) -> Vec<BookRecipe> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<BookRecipe>(l).ok())
        .filter(|r| r.id.starts_with(BOOK_ID_PREFIX) && !r.title.trim().is_empty())
        .collect()
}

/// Mtime-checked in-memory cache: the corpus is parsed once and only re-read
/// when the file changes (consistent with the app's read-from-disk philosophy
/// without re-parsing 2,000 lines per request).
#[derive(Default)]
pub struct BookCache(Mutex<Option<(SystemTime, Arc<Vec<BookRecipe>>)>>);

/// Load the book through the cache. A missing or unreadable file yields an
/// empty book — the builder UI then simply has nothing to deal.
pub fn load_book(path: &Path, cache: &BookCache) -> Arc<Vec<BookRecipe>> {
    let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let mut guard = cache.0.lock().unwrap_or_else(|p| p.into_inner());
    if let (Some((cached_at, book)), Some(mtime)) = (guard.as_ref(), mtime) {
        if *cached_at == mtime {
            return book.clone();
        }
    }
    let book = Arc::new(
        std::fs::read_to_string(path)
            .map(|text| parse_jsonl(&text))
            .unwrap_or_default(),
    );
    if let Some(mtime) = mtime {
        *guard = Some((mtime, book.clone()));
    } else {
        *guard = None;
    }
    book
}

// ============================================================================
// Book → Recipe adapter (lets book meals ride the shopping pipeline)
// ============================================================================

/// Present a book recipe as a [`Recipe`] keyed by its book id.
pub fn to_recipe(b: &BookRecipe) -> Recipe {
    Recipe {
        key: b.id.clone(),
        title: b.title.clone(),
        servings: b.servings,
        tags: b.tags.clone(),
        ingredients: b.ingredients.clone(),
        body_html: recipes::render_markdown(&b.body_markdown),
        body_markdown: b.body_markdown.clone(),
        path: PathBuf::new(),
        modified: chrono::Utc::now(),
    }
}

/// The recipe slice `selections` should resolve against: all real recipes
/// plus the book recipes the selections actually reference.
pub fn augment(
    recipes: &[Recipe],
    book: &[BookRecipe],
    selections: &[RecipeSelection],
) -> Vec<Recipe> {
    let mut all: Vec<Recipe> = recipes.to_vec();
    for sel in selections {
        if sel.key.starts_with(BOOK_ID_PREFIX) {
            if let Some(b) = book.iter().find(|b| b.id == sel.key) {
                all.push(to_recipe(b));
            }
        }
    }
    all
}

// ============================================================================
// Prompt scoring
// ============================================================================

/// Tokens to match (`include`) and tokens that disqualify a recipe entirely
/// (`exclude`, written as `-mushrooms` in the prompt).
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedPrompt {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

/// Small curated synonym table; the corpus tags do most of the matching work.
const SYNONYMS: &[(&str, &[&str])] = &[
    ("quick", &["weeknight", "fast", "easy"]),
    ("easy", &["weeknight", "quick"]),
    ("cozy", &["comfort", "hearty", "stew", "soup"]),
    ("comfort", &["cozy", "hearty"]),
    ("healthy", &["light", "fresh", "high-protein"]),
    ("light", &["fresh", "salad"]),
    ("pasta", &["noodles", "spaghetti", "penne", "orzo"]),
    ("noodles", &["pasta", "ramen", "lo-mein"]),
    ("beef", &["steak", "ground-beef"]),
    ("veggie", &["vegetarian", "meatless"]),
    ("vegetarian", &["meatless", "veggie", "bean", "lentil"]),
    ("grill", &["grilled", "bbq", "barbecue"]),
    ("bbq", &["grill", "barbecue", "grilled"]),
    ("mexican", &["tex-mex", "taco", "fajita"]),
    ("asian", &["stir-fry", "thai", "chinese", "japanese", "korean"]),
    ("spicy", &["hot", "chili"]),
    ("soup", &["stew", "chili"]),
    ("wrap", &["taco", "pita", "sandwich"]),
    ("bowl", &["grain-bowl", "rice-bowl"]),
];

fn synonyms_of(token: &str) -> &'static [&'static str] {
    SYNONYMS
        .iter()
        .find(|(t, _)| *t == token)
        .map(|(_, syns)| *syns)
        .unwrap_or(&[])
}

/// Normalize one word: lowercase, keep alphanumerics and inner hyphens.
fn norm_token(raw: &str) -> String {
    raw.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Parse a week prompt into include/exclude token lists.
pub fn parse_prompt(prompt: &str) -> ParsedPrompt {
    let mut include = Vec::new();
    let mut exclude = Vec::new();
    for raw in prompt.split(|c: char| c.is_whitespace() || c == ',' || c == ';') {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let (is_exclude, raw) = match raw.strip_prefix('-') {
            Some(rest) if !rest.is_empty() => (true, rest),
            _ => (false, raw),
        };
        let tok = norm_token(raw);
        // Drop empty and ultra-short noise words ("a", "of").
        if tok.len() < 3 {
            continue;
        }
        let list = if is_exclude { &mut exclude } else { &mut include };
        if !list.contains(&tok) {
            list.push(tok);
        }
    }
    ParsedPrompt { include, exclude }
}

/// Loose equality tolerating a trailing plural `s`.
fn eq_loose(a: &str, b: &str) -> bool {
    a == b
        || (a.len() > 3 && a.strip_suffix('s') == Some(b))
        || (b.len() > 3 && b.strip_suffix('s') == Some(a))
}

/// Whether `token` matches `field` (a tag, facet, or word), tolerating
/// hyphenation differences: "onepot" matches "one-pot", "pot" matches a part.
fn field_matches(token: &str, field: &str) -> bool {
    if eq_loose(token, field) {
        return true;
    }
    let squashed: String = field.chars().filter(|c| *c != '-').collect();
    let token_squashed: String = token.chars().filter(|c| *c != '-').collect();
    if eq_loose(&token_squashed, &squashed) {
        return true;
    }
    field.split('-').any(|part| eq_loose(token, part))
}

const W_TAG: f64 = 3.0;
const W_FACET: f64 = 2.5;
const W_TITLE: f64 = 2.0;
const W_INGREDIENT: f64 = 1.0;
const SYNONYM_FACTOR: f64 = 0.8;

/// Best single-field weight for `token` against `r`, or 0.0 for no match.
fn token_hit(token: &str, r: &BookRecipe) -> f64 {
    if r.tags.iter().any(|t| field_matches(token, &t.to_lowercase())) {
        return W_TAG;
    }
    if [&r.protein, &r.method, &r.cuisine]
        .iter()
        .any(|f| field_matches(token, &f.to_lowercase()))
    {
        return W_FACET;
    }
    if r.title
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .filter(|w| w.len() >= 3)
        .any(|w| field_matches(token, &w.to_lowercase()))
    {
        return W_TITLE;
    }
    if r.ingredients.iter().any(|i| {
        i.name
            .split(|c: char| !c.is_alphanumeric() && c != '-')
            .filter(|w| w.len() >= 3)
            .any(|w| field_matches(token, &w.to_lowercase()))
    }) {
        return W_INGREDIENT;
    }
    0.0
}

/// Score a recipe against a parsed prompt. `-1.0` means excluded.
pub fn score(p: &ParsedPrompt, r: &BookRecipe) -> f64 {
    for ex in &p.exclude {
        if token_hit(ex, r) > 0.0 {
            return -1.0;
        }
    }
    let mut total = 0.0;
    for tok in &p.include {
        let direct = token_hit(tok, r);
        let syn_best = synonyms_of(tok)
            .iter()
            .map(|s| token_hit(s, r) * SYNONYM_FACTOR)
            .fold(0.0, f64::max);
        total += direct.max(syn_best);
    }
    total
}

/// Rank the book against `prompt`, excluding skips/planned recipes and
/// anything hit by a `-token`. Equal-scoring matches are freshly shuffled on
/// every deal, then chosen with immediate anti-repetition for flavor, cuisine,
/// protein, and method.
pub fn rank<'a>(
    prompt: &str,
    book: &'a [BookRecipe],
    excluded_ids: &HashSet<String>,
    limit: usize,
) -> Vec<&'a BookRecipe> {
    let parsed = parse_prompt(prompt);
    let mut candidates: Vec<(f64, &BookRecipe)> = Vec::new();
    for r in book {
        if excluded_ids.contains(&r.id) {
            continue;
        }
        let s = score(&parsed, r);
        if s < 0.0 {
            continue;
        }
        candidates.push((s, r));
    }
    candidates.shuffle(&mut rand::thread_rng());
    candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut out = Vec::new();
    while out.len() < limit && !candidates.is_empty() {
        let best = candidates[0].0;
        let tier_end = candidates.iter().position(|x| x.0 < best).unwrap_or(candidates.len());
        let pick = (0..tier_end).find(|&i| {
            let r = candidates[i].1;
            let flavor = r.title.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
            let flavor_is_fresh = out.iter().rev().take(5).all(|prev: &&BookRecipe| {
                prev.title.split_whitespace().take(2).collect::<Vec<_>>().join(" ") != flavor
            });
            let cuisine_is_fresh = out.iter().rev().take(2).all(|prev: &&BookRecipe| prev.cuisine != r.cuisine);
            let adjacent_is_fresh = out.last().map(|prev: &&BookRecipe| prev.protein != r.protein && prev.method != r.method).unwrap_or(true);
            flavor_is_fresh && cuisine_is_fresh && adjacent_is_fresh
        }).unwrap_or(0);
        out.push(candidates.remove(pick).1);
    }
    out
}

// ============================================================================
// Per-week skip list ("Not" swipes)
// ============================================================================

fn skips_tree(db: &Db) -> Result<sled::Tree, String> {
    db.open_tree(SKIPS_TREE).map_err(|e| format!("DB error: {}", e))
}

/// Book ids skipped ("Not") for a week's deck.
pub fn skips(db: &Db, week_start: &str) -> Vec<String> {
    skips_tree(db)
        .ok()
        .and_then(|t| t.get(week_start.as_bytes()).ok().flatten())
        .and_then(|v| serde_json::from_slice(&v).ok())
        .unwrap_or_default()
}

/// Record a skip (idempotent).
pub fn add_skip(db: &Db, week_start: &str, id: &str) -> Result<(), String> {
    let mut list = skips(db, week_start);
    if !list.iter().any(|s| s == id) {
        list.push(id.to_string());
    }
    let value = serde_json::to_vec(&list).map_err(|e| format!("Serialize error: {}", e))?;
    skips_tree(db)?
        .insert(week_start.as_bytes(), value)
        .map_err(|e| format!("DB error: {}", e))?;
    Ok(())
}

/// Forget every skip for the week ("reshuffle the deck").
pub fn clear_skips(db: &Db, week_start: &str) -> Result<(), String> {
    skips_tree(db)?
        .remove(week_start.as_bytes())
        .map_err(|e| format!("DB error: {}", e))?;
    Ok(())
}

// ============================================================================
// Day assignment for deck picks
// ============================================================================

/// The date a fresh pick should land on: the first day of the week (in week
/// order) with the fewest meals, so consecutive picks spread across the week.
pub fn assign_date(plan: &MealPlan) -> String {
    assign_date_for_type(plan, "dinner")
}

/// Spread a meal type across the week independently, so breakfast and lunch
/// picks do not push dinners onto later days (and vice versa).
pub fn assign_date_for_type(plan: &MealPlan, meal_type: &str) -> String {
    plan.week_dates()
        .into_iter()
        .min_by_key(|d| {
            plan.meals_on(d)
                .into_iter()
                .filter(|m| m.meal_type == meal_type)
                .count()
        })
        .expect("a week always has seven dates")
}

// ============================================================================
// Promotion: book recipe → real, git-committed recipe
// ============================================================================

/// Outcome of promoting a book recipe into the collection.
#[derive(Debug, Clone, Serialize)]
pub struct PromotionResult {
    pub key: String,
    pub filename: String,
    pub title: String,
    /// True when the book recipe had already been promoted; the existing
    /// recipe is returned instead of creating a duplicate.
    pub already_promoted: bool,
    /// Planned meals (across all weeks) rewritten from `book_id` to the key.
    pub rewritten_meals: usize,
}

fn promotions_tree(db: &Db) -> Result<sled::Tree, String> {
    db.open_tree(PROMOTIONS_TREE)
        .map_err(|e| format!("DB error: {}", e))
}

/// The collection key a book recipe was promoted to, if any.
pub fn promoted_key(db: &Db, book_id: &str) -> Option<String> {
    promotions_tree(db)
        .ok()
        .and_then(|t| t.get(book_id.as_bytes()).ok().flatten())
        .and_then(|v| String::from_utf8(v.to_vec()).ok())
}

/// Promote a book recipe into the collection: write the markdown file, commit
/// it to git, remember the mapping, and rewrite any planned meals referencing
/// the book id to the new recipe key. Idempotent — a second promote returns
/// the existing recipe (a stale mapping to a since-deleted recipe self-heals
/// by promoting again).
pub fn promote(
    content_dir: &Path,
    db: &Db,
    recipes_list: &[Recipe],
    book: &[BookRecipe],
    book_id: &str,
    filename: Option<&str>,
) -> Result<PromotionResult, String> {
    let b = book
        .iter()
        .find(|b| b.id == book_id)
        .ok_or_else(|| format!("No such book recipe: {}", book_id))?;

    if let Some(key) = promoted_key(db, book_id) {
        if let Some(existing) = recipes_list.iter().find(|r| r.key == key) {
            // Still rewrite in case older plans reference the book id.
            let rewritten = mealplan::rewrite_book_refs(db, book_id, &key)?;
            return Ok(PromotionResult {
                key,
                filename: existing
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                title: existing.title.clone(),
                already_promoted: true,
                rewritten_meals: rewritten,
            });
        }
        // Mapping points at a deleted recipe — fall through and re-promote.
    }

    let path = match filename {
        Some(f) => {
            let f = f.trim();
            recipes::validate_filename(f)?;
            let p = content_dir.join(f);
            if p.exists() {
                return Err(format!("recipe already exists: {}", f));
            }
            p
        }
        None => recipes::unique_recipe_path(content_dir, &b.title),
    };
    crate::validate_path_within(&content_dir.to_path_buf(), &path)?;
    let final_filename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or("bad promotion path")?;

    // Facets fold into tags: frontmatter has no facet keys, so this is how
    // protein/method/cuisine survive the round-trip.
    let mut tags = b.tags.clone();
    for facet in [&b.protein, &b.method, &b.cuisine] {
        let f = facet.trim();
        if !f.is_empty() && !tags.iter().any(|t| t.eq_ignore_ascii_case(f)) {
            tags.push(f.to_string());
        }
    }

    let content =
        recipes::serialize_recipe(&b.title, b.servings, &tags, &b.ingredients, &b.body_markdown);
    std::fs::write(&path, &content).map_err(|e| format!("write failed: {}", e))?;
    recipes::git_commit(
        &content_dir.to_path_buf(),
        &path,
        &format!("Add recipe from book: {}", b.title),
    );

    let key = recipes::generate_key(&PathBuf::from(&final_filename));
    promotions_tree(db)?
        .insert(book_id.as_bytes(), key.as_bytes())
        .map_err(|e| format!("DB error: {}", e))?;
    let rewritten = mealplan::rewrite_book_refs(db, book_id, &key)?;

    Ok(PromotionResult {
        key,
        filename: final_filename,
        title: b.title.clone(),
        already_promoted: false,
        rewritten_meals: rewritten,
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = sled::open(dir.path().join("db")).unwrap();
        (dir, db)
    }

    fn book_recipe(id: &str, title: &str, protein: &str, method: &str) -> BookRecipe {
        BookRecipe {
            id: id.to_string(),
            title: title.to_string(),
            servings: Some(4),
            tags: vec!["dinner".into(), method.to_string()],
            ingredients: vec![
                Ingredient {
                    name: format!("{} pieces", protein),
                    qty: 1.0,
                    unit: "lb".into(),
                },
                Ingredient {
                    name: "onion".into(),
                    qty: 1.0,
                    unit: "whole".into(),
                },
            ],
            body_markdown: "## Prep ahead\nChop.\n\n## Day of (~20 min)\nCook.".into(),
            protein: protein.to_string(),
            method: method.to_string(),
            cuisine: "american".into(),
        }
    }

    fn jsonl_line(id: &str, title: &str) -> String {
        serde_json::to_string(&book_recipe(id, title, "chicken", "grill")).unwrap()
    }

    // ---- corpus ----

    #[test]
    fn parse_jsonl_happy_path_and_bad_lines() {
        let text = format!(
            "{}\nnot json at all\n{}\n\n{}\n",
            jsonl_line("bk-0001", "Grilled Chicken"),
            jsonl_line("bk-0002", "Chicken Skewers"),
            // wrong prefix → dropped
            jsonl_line("abc123", "Sneaky Recipe"),
        );
        let book = parse_jsonl(&text);
        assert_eq!(book.len(), 2);
        assert_eq!(book[0].id, "bk-0001");
        assert_eq!(book[1].id, "bk-0002");
    }

    #[test]
    fn load_book_missing_file_is_empty() {
        let cache = BookCache::default();
        let book = load_book(Path::new("/nonexistent/book.jsonl"), &cache);
        assert!(book.is_empty());
    }

    #[test]
    fn load_book_caches_and_reloads_on_mtime_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.jsonl");
        std::fs::write(&path, jsonl_line("bk-0001", "One")).unwrap();
        let cache = BookCache::default();

        let first = load_book(&path, &cache);
        assert_eq!(first.len(), 1);
        // Same mtime → same Arc back.
        let again = load_book(&path, &cache);
        assert!(Arc::ptr_eq(&first, &again));

        // Rewrite with a bumped mtime → reload.
        std::fs::write(
            &path,
            format!("{}\n{}", jsonl_line("bk-0001", "One"), jsonl_line("bk-0002", "Two")),
        )
        .unwrap();
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::fs::File::open(&path)
            .and_then(|f| f.set_modified(mtime + std::time::Duration::from_secs(2)))
            .unwrap();
        let reloaded = load_book(&path, &cache);
        assert_eq!(reloaded.len(), 2);
    }

    // ---- adapter ----

    #[test]
    fn to_recipe_maps_id_to_key_and_renders_html() {
        let b = book_recipe("bk-0042", "Test", "chicken", "grill");
        let r = to_recipe(&b);
        assert_eq!(r.key, "bk-0042");
        assert_eq!(r.title, "Test");
        assert_eq!(r.ingredients.len(), 2);
        assert!(r.body_html.contains("<h2>"));
    }

    #[test]
    fn augment_appends_only_referenced_book_recipes() {
        let real = vec![to_recipe(&book_recipe("bk-x", "pretend-real", "beef", "oven"))];
        let mut real = real;
        real[0].key = "abc123".into();
        let book = vec![
            book_recipe("bk-0001", "One", "chicken", "grill"),
            book_recipe("bk-0002", "Two", "beef", "oven"),
        ];
        let selections = vec![
            RecipeSelection { key: "abc123".into(), multiplier: 1.0 },
            RecipeSelection { key: "bk-0002".into(), multiplier: 1.0 },
            RecipeSelection { key: "bk-9999".into(), multiplier: 1.0 }, // unknown → ignored
        ];
        let all = augment(&real, &book, &selections);
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|r| r.key == "abc123"));
        assert!(all.iter().any(|r| r.key == "bk-0002" && r.title == "Two"));
    }

    // ---- prompt parsing & scoring ----

    #[test]
    fn parse_prompt_extracts_includes_and_excludes() {
        let p = parse_prompt("Grill-heavy week, use up CABBAGE -mushrooms, -fish a of");
        assert_eq!(p.include, vec!["grill-heavy", "week", "use", "cabbage"]);
        assert_eq!(p.exclude, vec!["mushrooms", "fish"]);
    }

    #[test]
    fn scoring_weight_ordering() {
        let r = book_recipe("bk-0001", "Lemon Skillet Rice", "chicken", "grill");
        // tag hit ("dinner") > facet hit ("chicken") > title hit ("lemon") > ingredient ("onion")
        let tag = score(&parse_prompt("dinner"), &r);
        let facet = score(&parse_prompt("chicken"), &r);
        let title = score(&parse_prompt("lemon"), &r);
        let ing = score(&parse_prompt("onion"), &r);
        assert!(tag > facet && facet > title && title > ing && ing > 0.0);
    }

    #[test]
    fn synonym_scores_lower_than_direct() {
        let mut r = book_recipe("bk-0001", "Big Salad", "chicken", "grill");
        r.tags = vec!["bbq".into()];
        // "grill" hits tag "bbq" only via synonym (facet "grill" is a direct hit,
        // so isolate: compare synonym-only tag hit vs direct tag hit).
        r.protein = "".into();
        r.method = "".into();
        let direct = score(&parse_prompt("bbq"), &r);
        let synonym = score(&parse_prompt("grill"), &r);
        assert!(direct > synonym && synonym > 0.0);
    }

    #[test]
    fn exclusion_token_removes_recipe() {
        let r = book_recipe("bk-0001", "Onion Soup", "chicken", "grill");
        assert_eq!(score(&parse_prompt("dinner -onion"), &r), -1.0);
        assert!(score(&parse_prompt("dinner -tofu"), &r) > 0.0);
    }

    #[test]
    fn field_matches_hyphen_variants() {
        assert!(field_matches("onepot", "one-pot"));
        assert!(field_matches("pot", "one-pot"));
        assert!(field_matches("taco", "tacos"));
        assert!(!field_matches("rice", "ricer-thing"));
    }

    #[test]
    fn rank_is_randomized_and_diverse() {
        let mut book = Vec::new();
        for i in 0..10 {
            book.push(book_recipe(&format!("bk-00{:02}", i), &format!("Chicken {}", i), "chicken", "grill"));
        }
        for i in 10..20 {
            book.push(book_recipe(&format!("bk-00{:02}", i), &format!("Beef {}", i), "beef", "oven"));
        }
        let a = rank("dinner", &book, &HashSet::new(), 8);
        assert_eq!(a.len(), 8);
        // The result keeps multiple protein families in play.
        let first_four: Vec<&str> = a.iter().take(4).map(|r| r.protein.as_str()).collect();
        assert!(first_four.contains(&"chicken") && first_four.contains(&"beef"));
    }

    #[test]
    fn rank_excludes_given_ids_and_respects_limit() {
        let book = vec![
            book_recipe("bk-0001", "One", "chicken", "grill"),
            book_recipe("bk-0002", "Two", "chicken", "grill"),
            book_recipe("bk-0003", "Three", "beef", "oven"),
        ];
        let excluded: HashSet<String> = ["bk-0002".to_string()].into();
        let out = rank("", &book, &excluded, 10);
        assert_eq!(out.len(), 2);
        assert!(!out.iter().any(|r| r.id == "bk-0002"));
        assert_eq!(rank("", &book, &HashSet::new(), 1).len(), 1);
    }

    #[test]
    fn empty_prompt_still_deals_a_deck() {
        let book = vec![
            book_recipe("bk-0001", "One", "chicken", "grill"),
            book_recipe("bk-0002", "Two", "beef", "oven"),
        ];
        let out = rank("", &book, &HashSet::new(), 10);
        assert_eq!(out.len(), 2);
    }

    // ---- skips ----

    #[test]
    fn skips_crud_idempotent() {
        let (_dir, db) = temp_db();
        assert!(skips(&db, "2026-08-24").is_empty());
        add_skip(&db, "2026-08-24", "bk-0001").unwrap();
        add_skip(&db, "2026-08-24", "bk-0001").unwrap();
        add_skip(&db, "2026-08-24", "bk-0002").unwrap();
        assert_eq!(skips(&db, "2026-08-24"), vec!["bk-0001", "bk-0002"]);
        // Other weeks unaffected.
        assert!(skips(&db, "2026-08-31").is_empty());
        clear_skips(&db, "2026-08-24").unwrap();
        assert!(skips(&db, "2026-08-24").is_empty());
    }

    // ---- day assignment ----

    #[test]
    fn assign_date_prefers_first_emptiest_day() {
        let (_dir, db) = temp_db();
        let book = vec![book_recipe("bk-0001", "One", "chicken", "grill")];
        // Empty plan: first day of the week.
        let plan = mealplan::load_plan(&db, "2026-08-24");
        assert_eq!(assign_date(&plan), "2026-08-24");
        // With a meal on Monday, the next empty day wins.
        let plan = mealplan::add_meal_entry(
            &db,
            &[],
            &book,
            "2026-08-24",
            None,
            Some("bk-0001"),
            None,
            1.0,
        )
        .unwrap();
        assert_eq!(assign_date(&plan), "2026-08-25");
    }

    #[test]
    fn assign_date_spreads_each_meal_type_independently() {
        let (_dir, db, _content_dir, book) = promo_fixture();
        mealplan::add_meal_entry_typed(
            &db, &[], &book, "2026-08-24", None, Some("bk-0001"), None, 1.0,
            "breakfast",
        )
        .unwrap();
        let plan = mealplan::load_plan(&db, "2026-08-24");
        assert_eq!(assign_date_for_type(&plan, "breakfast"), "2026-08-25");
        assert_eq!(assign_date_for_type(&plan, "dinner"), "2026-08-24");
    }

    // ---- promotion ----

    fn promo_fixture() -> (tempfile::TempDir, Db, PathBuf, Vec<BookRecipe>) {
        let dir = tempfile::tempdir().unwrap();
        let content_dir = dir.path().join("content");
        std::fs::create_dir_all(&content_dir).unwrap();
        let db = sled::open(dir.path().join("db")).unwrap();
        let book = vec![book_recipe("bk-0001", "Grilled Chicken Bowls", "chicken", "grill")];
        (dir, db, content_dir, book)
    }

    #[test]
    fn promote_writes_roundtrippable_file_with_facet_tags() {
        let (_dir, db, content_dir, book) = promo_fixture();
        let res = promote(&content_dir, &db, &[], &book, "bk-0001", None).unwrap();
        assert!(!res.already_promoted);
        assert_eq!(res.filename, "grilled-chicken-bowls.md");
        assert_eq!(res.key.len(), 6);

        let written = std::fs::read_to_string(content_dir.join(&res.filename)).unwrap();
        let (fm, body) = recipes::parse_frontmatter(&written);
        assert_eq!(fm.title.unwrap(), "Grilled Chicken Bowls");
        assert_eq!(fm.ingredients.len(), 2);
        // Facets folded into tags.
        assert!(fm.tags.iter().any(|t| t == "chicken"));
        assert!(fm.tags.iter().any(|t| t == "grill"));
        assert!(fm.tags.iter().any(|t| t == "american"));
        assert!(body.contains("## Day of (~20 min)"));
    }

    #[test]
    fn promote_is_idempotent_and_self_heals_stale_mapping() {
        let (_dir, db, content_dir, book) = promo_fixture();
        let first = promote(&content_dir, &db, &[], &book, "bk-0001", None).unwrap();

        // Second promote sees the existing recipe (via the loaded list).
        let loaded = recipes::load_all_recipes(&content_dir);
        let second = promote(&content_dir, &db, &loaded, &book, "bk-0001", None).unwrap();
        assert!(second.already_promoted);
        assert_eq!(second.key, first.key);

        // Delete the file → stale mapping → re-promotes cleanly.
        std::fs::remove_file(content_dir.join(&first.filename)).unwrap();
        let third = promote(&content_dir, &db, &[], &book, "bk-0001", None).unwrap();
        assert!(!third.already_promoted);
        assert_eq!(third.filename, "grilled-chicken-bowls.md");
    }

    #[test]
    fn promote_duplicate_title_gets_suffix() {
        let (_dir, db, content_dir, book) = promo_fixture();
        std::fs::write(content_dir.join("grilled-chicken-bowls.md"), "occupied").unwrap();
        let res = promote(&content_dir, &db, &[], &book, "bk-0001", None).unwrap();
        assert_eq!(res.filename, "grilled-chicken-bowls-1.md");
    }

    #[test]
    fn promote_rejects_unknown_id_and_bad_filename() {
        let (_dir, db, content_dir, book) = promo_fixture();
        assert!(promote(&content_dir, &db, &[], &book, "bk-9999", None).is_err());
        assert!(promote(&content_dir, &db, &[], &book, "bk-0001", Some("../evil.md")).is_err());
        assert!(promote(&content_dir, &db, &[], &book, "bk-0001", Some("no-extension")).is_err());
    }

    #[test]
    fn promote_rewrites_planned_meals_across_weeks() {
        let (_dir, db, content_dir, book) = promo_fixture();
        // Plan the book meal in two different weeks.
        mealplan::add_meal_entry(&db, &[], &book, "2026-08-24", None, Some("bk-0001"), None, 1.0)
            .unwrap();
        mealplan::add_meal_entry(&db, &[], &book, "2026-09-02", None, Some("bk-0001"), None, 2.0)
            .unwrap();

        let res = promote(&content_dir, &db, &[], &book, "bk-0001", None).unwrap();
        assert_eq!(res.rewritten_meals, 2);

        for week in ["2026-08-24", "2026-08-31"] {
            let plan = mealplan::load_plan(&db, week);
            for m in &plan.meals {
                assert_eq!(m.book_id, None);
                assert_eq!(m.recipe_key.as_deref(), Some(res.key.as_str()));
            }
        }
    }
}
