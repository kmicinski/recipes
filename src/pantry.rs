//! Sled-backed pantry management.
//!
//! The pantry tracks which ingredients you have on hand as a binary state
//! (have / don't have). Ingredient names are normalized to lowercase/trimmed.

use sled::Db;

const PANTRY_TREE: &str = "pantry";

/// Normalize an ingredient name for pantry lookups.
pub fn normalize(name: &str) -> String {
    name.trim().to_lowercase()
}

/// Check if an ingredient is in the pantry.
pub fn has(db: &Db, name: &str) -> bool {
    let tree = match db.open_tree(PANTRY_TREE) {
        Ok(t) => t,
        Err(_) => return false,
    };
    tree.contains_key(normalize(name).as_bytes()).unwrap_or(false)
}

/// Add an ingredient to the pantry.
pub fn add(db: &Db, name: &str) -> Result<(), String> {
    let tree = db
        .open_tree(PANTRY_TREE)
        .map_err(|e| format!("DB error: {}", e))?;
    tree.insert(normalize(name).as_bytes(), b"1")
        .map_err(|e| format!("DB error: {}", e))?;
    Ok(())
}

/// Remove an ingredient from the pantry.
pub fn remove(db: &Db, name: &str) -> Result<(), String> {
    let tree = db
        .open_tree(PANTRY_TREE)
        .map_err(|e| format!("DB error: {}", e))?;
    tree.remove(normalize(name).as_bytes())
        .map_err(|e| format!("DB error: {}", e))?;
    Ok(())
}

/// Toggle an ingredient in/out of the pantry. Returns the new state (true = in pantry).
pub fn toggle(db: &Db, name: &str) -> Result<bool, String> {
    if has(db, name) {
        remove(db, name)?;
        Ok(false)
    } else {
        add(db, name)?;
        Ok(true)
    }
}

/// Bulk add ingredients to the pantry.
pub fn bulk_add(db: &Db, names: &[String]) -> Result<(), String> {
    for name in names {
        add(db, name)?;
    }
    Ok(())
}

/// Bulk remove ingredients from the pantry.
pub fn bulk_remove(db: &Db, names: &[String]) -> Result<(), String> {
    for name in names {
        remove(db, name)?;
    }
    Ok(())
}

/// List all ingredients currently in the pantry, sorted alphabetically.
pub fn list(db: &Db) -> Vec<String> {
    let tree = match db.open_tree(PANTRY_TREE) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };

    let mut items: Vec<String> = tree
        .iter()
        .filter_map(|r| r.ok())
        .filter_map(|(k, _)| String::from_utf8(k.to_vec()).ok())
        .collect();

    items.sort();
    items
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

    #[test]
    fn test_normalize() {
        assert_eq!(normalize("  Chicken Breast  "), "chicken breast");
        assert_eq!(normalize("SOY SAUCE"), "soy sauce");
    }

    #[test]
    fn test_add_and_has() {
        let db = temp_db();
        assert!(!has(&db, "flour"));
        add(&db, "Flour").unwrap();
        assert!(has(&db, "flour"));
        assert!(has(&db, "Flour"));
        assert!(has(&db, "  FLOUR  "));
    }

    #[test]
    fn test_remove() {
        let db = temp_db();
        add(&db, "salt").unwrap();
        assert!(has(&db, "salt"));
        remove(&db, "salt").unwrap();
        assert!(!has(&db, "salt"));
    }

    #[test]
    fn test_toggle() {
        let db = temp_db();
        assert!(!has(&db, "pepper"));
        let state = toggle(&db, "pepper").unwrap();
        assert!(state);
        assert!(has(&db, "pepper"));
        let state = toggle(&db, "pepper").unwrap();
        assert!(!state);
        assert!(!has(&db, "pepper"));
    }

    #[test]
    fn test_bulk_add() {
        let db = temp_db();
        let items: Vec<String> = vec!["eggs".into(), "milk".into(), "butter".into()];
        bulk_add(&db, &items).unwrap();
        assert!(has(&db, "eggs"));
        assert!(has(&db, "milk"));
        assert!(has(&db, "butter"));
    }

    #[test]
    fn test_bulk_remove() {
        let db = temp_db();
        bulk_add(&db, &vec!["a".into(), "b".into(), "c".into()]).unwrap();
        bulk_remove(&db, &vec!["a".into(), "c".into()]).unwrap();
        assert!(!has(&db, "a"));
        assert!(has(&db, "b"));
        assert!(!has(&db, "c"));
    }

    #[test]
    fn test_list() {
        let db = temp_db();
        bulk_add(&db, &vec!["zucchini".into(), "apple".into(), "banana".into()]).unwrap();
        let items = list(&db);
        assert_eq!(items, vec!["apple", "banana", "zucchini"]);
    }

    #[test]
    fn test_empty_pantry() {
        let db = temp_db();
        assert!(list(&db).is_empty());
        assert!(!has(&db, "anything"));
    }

    #[test]
    fn test_remove_nonexistent() {
        let db = temp_db();
        // Should not error
        remove(&db, "nonexistent").unwrap();
    }
}
