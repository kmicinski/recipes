//! Instacart Connect integration for generating shoppable carts.

use crate::shopping::SavedTrip;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::env;

const DEFAULT_INSTACART_BASE_URL: &str = "https://connect.instacart.com";

#[derive(Debug)]
pub enum InstacartError {
    NotConfigured(String),
    InvalidTrip(String),
    Http(String),
    Upstream(String),
    Decode(String),
}

impl InstacartError {
    pub fn as_message(&self) -> String {
        match self {
            InstacartError::NotConfigured(m)
            | InstacartError::InvalidTrip(m)
            | InstacartError::Http(m)
            | InstacartError::Upstream(m)
            | InstacartError::Decode(m) => m.clone(),
        }
    }

    pub fn is_not_configured(&self) -> bool {
        matches!(self, InstacartError::NotConfigured(_))
    }
}

#[derive(Debug, Clone)]
struct InstacartConfig {
    api_key: String,
    base_url: String,
    partner_link_base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct InstacartLineItem {
    name: String,
    quantity: f64,
    unit: String,
}

#[derive(Debug, Clone, Serialize)]
struct LandingPageConfiguration {
    partner_linkback_url: String,
}

#[derive(Debug, Clone, Serialize)]
struct CreateProductsLinkRequest {
    title: String,
    link_type: &'static str,
    line_items: Vec<InstacartLineItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    landing_page_configuration: Option<LandingPageConfiguration>,
}

#[derive(Debug, Deserialize)]
struct CreateProductsLinkResponse {
    products_link_url: String,
}

fn load_config_from_env() -> Result<InstacartConfig, InstacartError> {
    let api_key = env::var("INSTACART_API_KEY")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .ok_or_else(|| {
            InstacartError::NotConfigured(
                "Instacart is not configured: set INSTACART_API_KEY".to_string(),
            )
        })?;
    let base_url = env::var("INSTACART_BASE_URL")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_INSTACART_BASE_URL.to_string());
    let partner_link_base_url = env::var("INSTACART_PARTNER_LINK_BASE_URL")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());

    Ok(InstacartConfig {
        api_key,
        base_url,
        partner_link_base_url,
    })
}

fn normalize_instacart_unit(unit: &str) -> String {
    match unit.trim().to_lowercase().as_str() {
        "tsp" | "teaspoon" | "teaspoons" => "teaspoon",
        "tbsp" | "tablespoon" | "tablespoons" => "tablespoon",
        "oz" | "ounce" | "ounces" => "ounce",
        "lb" | "lbs" | "pound" | "pounds" => "pound",
        "g" | "gram" | "grams" => "gram",
        "kg" | "kilogram" | "kilograms" => "kilogram",
        "ml" | "milliliter" | "milliliters" => "milliliter",
        "l" | "liter" | "liters" => "liter",
        "cup" | "cups" => "cup",
        "package" | "packages" | "pack" | "packs" | "jar" | "jars" => "package",
        "can" | "cans" => "can",
        "clove" | "cloves" => "clove",
        "pinch" | "pinches" => "pinch",
        "bunch" | "bunches" => "bunch",
        "whole" | "piece" | "pieces" | "item" | "items" | "each" => "each",
        _ => "each",
    }
    .to_string()
}

fn normalize_instacart_quantity(qty: f64) -> f64 {
    if qty.is_finite() && qty > 0.0 {
        qty
    } else {
        1.0
    }
}

fn normalize_instacart_name(name: &str) -> String {
    name.split_whitespace().collect::<Vec<&str>>().join(" ")
}

fn build_line_items(trip: &SavedTrip) -> Vec<InstacartLineItem> {
    trip.items
        .iter()
        .filter(|item| !item.in_pantry)
        .filter_map(|item| {
            let name = normalize_instacart_name(&item.name);
            if name.is_empty() {
                return None;
            }
            Some(InstacartLineItem {
                name,
                quantity: normalize_instacart_quantity(item.qty),
                unit: normalize_instacart_unit(&item.unit),
            })
        })
        .collect()
}

fn trip_title(trip: &SavedTrip) -> String {
    let date = trip.created_at.get(..10).unwrap_or("unknown-date");
    format!("Shopping Trip {}", date)
}

fn create_products_link_request(
    trip: &SavedTrip,
    partner_linkback_url: Option<String>,
) -> Result<CreateProductsLinkRequest, InstacartError> {
    let line_items = build_line_items(trip);
    if line_items.is_empty() {
        return Err(InstacartError::InvalidTrip(
            "Trip has no purchasable items for Instacart".to_string(),
        ));
    }

    let landing_page_configuration = partner_linkback_url.map(|url| LandingPageConfiguration {
        partner_linkback_url: url,
    });

    Ok(CreateProductsLinkRequest {
        title: trip_title(trip),
        link_type: "shopping_list",
        line_items,
        landing_page_configuration,
    })
}

fn build_partner_linkback_url(base: &str, trip_id: &str) -> String {
    format!("{}/shopping/trip/{}", base.trim_end_matches('/'), trip_id)
}

/// Stable fingerprint used for caching cart links per trip payload.
pub fn trip_payload_fingerprint(trip: &SavedTrip) -> String {
    let payload = create_products_link_request(trip, None)
        .and_then(|req| serde_json::to_vec(&req).map_err(|e| InstacartError::Decode(e.to_string())))
        .unwrap_or_default();
    let digest = Sha256::digest(payload);
    digest.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Create a products link URL for a trip via Instacart Connect.
pub async fn create_products_link_for_trip(trip: &SavedTrip) -> Result<String, InstacartError> {
    let config = load_config_from_env()?;
    let linkback_url = config
        .partner_link_base_url
        .as_deref()
        .map(|base| build_partner_linkback_url(base, &trip.id));
    let request_body = create_products_link_request(trip, linkback_url)?;
    let endpoint = format!(
        "{}/idp/v1/products/products_link",
        config.base_url.trim_end_matches('/')
    );

    let resp = Client::new()
        .post(endpoint)
        .bearer_auth(config.api_key)
        .header("Accept", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| InstacartError::Http(format!("Instacart request failed: {}", e)))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| InstacartError::Http(format!("Instacart response read failed: {}", e)))?;
    if !status.is_success() {
        let snippet: String = body.chars().take(400).collect();
        return Err(InstacartError::Upstream(format!(
            "Instacart returned {}: {}",
            status.as_u16(),
            snippet
        )));
    }

    let parsed: CreateProductsLinkResponse = serde_json::from_str(&body)
        .map_err(|e| InstacartError::Decode(format!("Invalid Instacart response: {}", e)))?;
    if parsed.products_link_url.trim().is_empty() {
        return Err(InstacartError::Decode(
            "Instacart response missing products_link_url".to_string(),
        ));
    }
    Ok(parsed.products_link_url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ShoppingItem;
    use crate::shopping::TripRecipe;

    fn sample_trip(items: Vec<ShoppingItem>) -> SavedTrip {
        SavedTrip {
            id: "trip_123".to_string(),
            items,
            recipes: vec![TripRecipe {
                key: "abc123".to_string(),
                title: "Dish".to_string(),
                multiplier: 1.0,
            }],
            created_at: "2026-02-22T00:00:00Z".to_string(),
            instacart_products_link_url: None,
            instacart_products_link_fingerprint: None,
        }
    }

    #[test]
    fn test_normalize_instacart_unit_synonym() {
        assert_eq!(normalize_instacart_unit("tbsp"), "tablespoon");
        assert_eq!(normalize_instacart_unit("cups"), "cup");
    }

    #[test]
    fn test_normalize_instacart_unit_unknown_defaults_to_each() {
        assert_eq!(normalize_instacart_unit("foobar"), "each");
        assert_eq!(normalize_instacart_unit(""), "each");
    }

    #[test]
    fn test_normalize_instacart_quantity_invalid_defaults_to_one() {
        assert_eq!(normalize_instacart_quantity(0.0), 1.0);
        assert_eq!(normalize_instacart_quantity(-2.0), 1.0);
        assert_eq!(normalize_instacart_quantity(f64::NAN), 1.0);
    }

    #[test]
    fn test_normalize_instacart_name_collapses_whitespace() {
        assert_eq!(
            normalize_instacart_name("  green   onions \n bunch "),
            "green onions bunch"
        );
    }

    #[test]
    fn test_build_line_items_filters_pantry_and_blank_names() {
        let trip = sample_trip(vec![
            ShoppingItem {
                name: "milk".to_string(),
                qty: 1.0,
                unit: "carton".to_string(),
                in_pantry: false,
                sources: vec![],
            },
            ShoppingItem {
                name: "  ".to_string(),
                qty: 2.0,
                unit: "g".to_string(),
                in_pantry: false,
                sources: vec![],
            },
            ShoppingItem {
                name: "salt".to_string(),
                qty: 1.0,
                unit: "tsp".to_string(),
                in_pantry: true,
                sources: vec![],
            },
        ]);

        let line_items = build_line_items(&trip);
        assert_eq!(line_items.len(), 1);
        assert_eq!(line_items[0].name, "milk");
    }

    #[test]
    fn test_build_line_items_normalizes_qty_and_unit() {
        let trip = sample_trip(vec![ShoppingItem {
            name: "olive oil".to_string(),
            qty: 0.0,
            unit: "tbsp".to_string(),
            in_pantry: false,
            sources: vec![],
        }]);

        let line_items = build_line_items(&trip);
        assert_eq!(line_items[0].quantity, 1.0);
        assert_eq!(line_items[0].unit, "tablespoon");
    }

    #[test]
    fn test_create_products_link_request_has_linkback_when_set() {
        let trip = sample_trip(vec![ShoppingItem {
            name: "onion".to_string(),
            qty: 2.0,
            unit: "whole".to_string(),
            in_pantry: false,
            sources: vec![],
        }]);
        let req = create_products_link_request(
            &trip,
            Some("https://recipes.example.com/shopping/trip/trip_123".to_string()),
        )
        .unwrap();

        assert_eq!(req.link_type, "shopping_list");
        assert!(req.landing_page_configuration.is_some());
        assert_eq!(req.line_items.len(), 1);
    }

    #[test]
    fn test_create_products_link_request_errors_without_need_items() {
        let trip = sample_trip(vec![ShoppingItem {
            name: "salt".to_string(),
            qty: 1.0,
            unit: "tsp".to_string(),
            in_pantry: true,
            sources: vec![],
        }]);
        let err = create_products_link_request(&trip, None).unwrap_err();
        assert!(matches!(err, InstacartError::InvalidTrip(_)));
    }

    #[test]
    fn test_build_partner_linkback_url_trims_slash() {
        let url = build_partner_linkback_url("https://recipes.example.com/", "trip_123");
        assert_eq!(url, "https://recipes.example.com/shopping/trip/trip_123");
    }

    #[test]
    fn test_trip_payload_fingerprint_is_stable() {
        let trip = sample_trip(vec![ShoppingItem {
            name: "milk".to_string(),
            qty: 1.0,
            unit: "whole".to_string(),
            in_pantry: false,
            sources: vec![],
        }]);
        let a = trip_payload_fingerprint(&trip);
        let b = trip_payload_fingerprint(&trip);
        assert_eq!(a, b);
    }

    #[test]
    fn test_trip_payload_fingerprint_changes_when_items_change() {
        let trip_a = sample_trip(vec![ShoppingItem {
            name: "milk".to_string(),
            qty: 1.0,
            unit: "whole".to_string(),
            in_pantry: false,
            sources: vec![],
        }]);
        let trip_b = sample_trip(vec![ShoppingItem {
            name: "milk".to_string(),
            qty: 2.0,
            unit: "whole".to_string(),
            in_pantry: false,
            sources: vec![],
        }]);
        assert_ne!(
            trip_payload_fingerprint(&trip_a),
            trip_payload_fingerprint(&trip_b)
        );
    }
}
