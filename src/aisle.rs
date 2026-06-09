//! Store-section ("aisle") classification for shopping-list items.
//!
//! When you walk a store you don't want your list alphabetized — you want it
//! grouped by where things physically live, in the order you pass them. This
//! module assigns each ingredient to a section using a keyword heuristic, with
//! a per-ingredient manual override persisted in Sled so a wrong guess can be
//! corrected once and stay corrected.

use crate::models::ShoppingItem;
use sled::Db;

const AISLE_TREE: &str = "aisle_overrides";

/// Canonical store sections, listed in a sensible front-to-back walking order.
/// `group_items` emits sections in exactly this order. The last entry is the
/// fallback for anything the classifier can't place.
pub const SECTIONS: &[&str] = &[
    "Produce",
    "Bakery",
    "Meat & Seafood",
    "Dairy & Eggs",
    "Frozen",
    "Dry Goods & Pasta",
    "Canned & Jarred",
    "Condiments & Sauces",
    "Baking & Spices",
    "Beverages",
    "Snacks",
    "Other",
];

/// The fallback section for items that match no keyword.
pub const OTHER: &str = "Other";

/// Keyword rules, evaluated in order — the first section with a matching
/// keyword wins, so more specific sections come before broad ones. Each
/// keyword is matched as a lowercase substring of the normalized item name.
const RULES: &[(&str, &[&str])] = &[
    // Frozen first: "frozen X" should override whatever X is.
    (
        "Frozen",
        &["frozen", "ice cream", "popsicle", "frozen pizza"],
    ),
    (
        "Meat & Seafood",
        &[
            "chicken", "beef", "pork", "bacon", "sausage", "turkey", "lamb", "steak", "ground beef",
            "ground turkey", "ham", "salmon", "shrimp", "fish", "cod", "tilapia", "prosciutto",
        ],
    ),
    (
        "Dairy & Eggs",
        &[
            "milk", "cheese", "butter", "yogurt", "cream", "egg", "eggs", "sour cream",
            "half and half", "mozzarella", "parmesan", "cheddar", "feta", "ricotta",
            "cottage cheese",
        ],
    ),
    (
        "Bakery",
        &[
            "bread", "bun", "buns", "bagel", "tortilla", "baguette", "roll", "pita", "croissant",
            "naan", "english muffin",
        ],
    ),
    (
        "Condiments & Sauces",
        &[
            "ketchup", "mustard", "mayo", "mayonnaise", "soy sauce", "hot sauce", "sriracha", "bbq",
            "barbecue", "salsa", "dressing", "vinegar", "olive oil", "sesame oil", "honey", "syrup",
            "jam", "jelly", "peanut butter", "relish", "worcestershire", "fish sauce", "oyster sauce",
        ],
    ),
    (
        "Canned & Jarred",
        &[
            "canned", "broth", "stock", "tomato paste", "tomato sauce", "crushed tomato",
            "diced tomato", "coconut milk", "tuna", "olives", "pickles", "chickpeas",
            "black beans", "kidney beans",
        ],
    ),
    (
        "Baking & Spices",
        &[
            "flour", "sugar", "baking soda", "baking powder", "yeast", "vanilla", "cocoa",
            "cinnamon", "nutmeg", "cumin", "paprika", "oregano", "thyme", "salt", "black pepper",
            "peppercorn", "chili powder", "curry powder", "cornstarch", "brown sugar",
            "powdered sugar", "extract",
        ],
    ),
    (
        "Dry Goods & Pasta",
        &[
            "pasta", "spaghetti", "rice", "noodle", "noodles", "cereal", "oats", "oatmeal",
            "quinoa", "lentils", "couscous", "macaroni",
        ],
    ),
    (
        "Beverages",
        &["water", "juice", "soda", "coffee", "tea", "wine", "beer", "cola", "seltzer"],
    ),
    (
        "Snacks",
        &[
            "chips", "crackers", "cookie", "cookies", "candy", "chocolate", "nuts", "popcorn",
            "pretzel", "granola bar",
        ],
    ),
    (
        "Produce",
        &[
            "onion", "garlic", "tomato", "potato", "lettuce", "carrot", "pepper", "apple",
            "banana", "lemon", "lime", "spinach", "broccoli", "cilantro", "parsley", "ginger",
            "mushroom", "celery", "cucumber", "avocado", "kale", "zucchini", "scallion",
            "green onion", "herb", "basil", "lettuce", "berries", "strawberr", "grape", "orange",
            "cabbage", "cauliflower", "corn", "bean sprout", "leek", "shallot", "fruit",
            "vegetable",
        ],
    ),
];

/// Normalize an ingredient name for classification and override lookups.
fn normalize(name: &str) -> String {
    name.trim().to_lowercase()
}

/// Classify an ingredient name into a store section using the keyword heuristic.
/// Returns [`OTHER`] when nothing matches.
pub fn classify(name: &str) -> &'static str {
    let n = normalize(name);
    for (section, keywords) in RULES {
        if keywords.iter().any(|kw| n.contains(kw)) {
            return section;
        }
    }
    OTHER
}

/// Resolve the section for an ingredient: a manual override if one is stored,
/// otherwise the heuristic classification.
pub fn section_for(db: &Db, name: &str) -> String {
    if let Ok(tree) = db.open_tree(AISLE_TREE) {
        if let Ok(Some(bytes)) = tree.get(normalize(name).as_bytes()) {
            if let Ok(section) = String::from_utf8(bytes.to_vec()) {
                if !section.is_empty() {
                    return section;
                }
            }
        }
    }
    classify(name).to_string()
}

/// Store (or clear) a manual section override for an ingredient. Passing a
/// section that isn't one of [`SECTIONS`] is rejected. Passing an empty string
/// removes the override, reverting to the heuristic.
pub fn set_override(db: &Db, name: &str, section: &str) -> Result<(), String> {
    let tree = db
        .open_tree(AISLE_TREE)
        .map_err(|e| format!("DB error: {}", e))?;
    let key = normalize(name);
    if section.is_empty() {
        tree.remove(key.as_bytes())
            .map_err(|e| format!("DB error: {}", e))?;
        return Ok(());
    }
    if !SECTIONS.contains(&section) {
        return Err(format!("Unknown store section: {}", section));
    }
    tree.insert(key.as_bytes(), section.as_bytes())
        .map_err(|e| format!("DB error: {}", e))?;
    Ok(())
}

/// Group items by store section, returned in canonical [`SECTIONS`] order.
/// Empty sections are omitted. Items keep their incoming relative order within
/// a section.
pub fn group_items<'a>(
    db: &Db,
    items: &'a [ShoppingItem],
) -> Vec<(&'static str, Vec<&'a ShoppingItem>)> {
    let mut buckets: Vec<(&'static str, Vec<&'a ShoppingItem>)> =
        SECTIONS.iter().map(|s| (*s, Vec::new())).collect();

    for item in items {
        let section = section_for(db, &item.name);
        // Find the canonical bucket; fall back to OTHER if an override somehow
        // names a section no longer in SECTIONS.
        let idx = SECTIONS
            .iter()
            .position(|s| *s == section)
            .unwrap_or(SECTIONS.len() - 1);
        buckets[idx].1.push(item);
    }

    buckets.into_iter().filter(|(_, v)| !v.is_empty()).collect()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> Db {
        let dir = tempfile::tempdir().unwrap();
        sled::open(dir.path()).unwrap()
    }

    fn item(name: &str) -> ShoppingItem {
        ShoppingItem {
            name: name.to_string(),
            qty: 1.0,
            unit: "".into(),
            in_pantry: false,
            sources: vec![],
        }
    }

    #[test]
    fn test_classify_clear_cases() {
        assert_eq!(classify("frozen peas"), "Frozen");
        assert_eq!(classify("Boneless Chicken Breast"), "Meat & Seafood");
        assert_eq!(classify("whole milk"), "Dairy & Eggs");
        assert_eq!(classify("sourdough bread"), "Bakery");
        assert_eq!(classify("Granny Smith Apple"), "Produce");
        assert_eq!(classify("spaghetti"), "Dry Goods & Pasta");
        assert_eq!(classify("ketchup"), "Condiments & Sauces");
        assert_eq!(classify("all-purpose flour"), "Baking & Spices");
    }

    #[test]
    fn test_classify_fallback() {
        assert_eq!(classify("aluminum foil"), OTHER);
        assert_eq!(classify(""), OTHER);
    }

    #[test]
    fn test_override_roundtrip() {
        let db = temp_db();
        // Heuristic says Produce.
        assert_eq!(section_for(&db, "tomato"), "Produce");
        // Override wins.
        set_override(&db, "Tomato", "Canned & Jarred").unwrap();
        assert_eq!(section_for(&db, "tomato"), "Canned & Jarred");
        // Clearing reverts to the heuristic.
        set_override(&db, "tomato", "").unwrap();
        assert_eq!(section_for(&db, "tomato"), "Produce");
    }

    #[test]
    fn test_override_rejects_unknown_section() {
        let db = temp_db();
        assert!(set_override(&db, "salt", "Pharmacy").is_err());
    }

    #[test]
    fn test_group_items_order_and_emptiness() {
        let db = temp_db();
        let items = vec![
            item("ketchup"),       // Condiments & Sauces
            item("chicken thighs"), // Meat & Seafood
            item("banana"),        // Produce
            item("aluminum foil"), // Other
        ];
        let groups = group_items(&db, &items);
        let names: Vec<&str> = groups.iter().map(|(s, _)| *s).collect();
        // Canonical order: Produce, Meat & Seafood, Condiments & Sauces, Other.
        assert_eq!(
            names,
            vec!["Produce", "Meat & Seafood", "Condiments & Sauces", "Other"]
        );
        // No empty buckets leak through.
        assert!(groups.iter().all(|(_, v)| !v.is_empty()));
    }

    #[test]
    fn test_group_respects_override() {
        let db = temp_db();
        set_override(&db, "tomato", "Canned & Jarred").unwrap();
        let items = vec![item("tomato")];
        let groups = group_items(&db, &items);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, "Canned & Jarred");
    }
}
