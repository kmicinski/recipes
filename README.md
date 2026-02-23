# Recipes App

Rust + Axum + Sled app made with Claude Code. 

- Uses Micinski's preferred style.
- Relatively simple recipe interface.

## Instacart Connect (Phase 1)

Set these environment variables to enable API-backed cart links from saved shopping trips:

- `INSTACART_API_KEY` (required): Instacart API bearer token.
- `INSTACART_BASE_URL` (optional): defaults to `https://connect.instacart.com`.
- `INSTACART_PARTNER_LINK_BASE_URL` (optional): base URL used for `partner_linkback_url`, for example `http://localhost:7001`.
