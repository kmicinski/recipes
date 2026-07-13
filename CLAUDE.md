Dearest Claude,

This directory should be a Rust + Axum + Sled webapp, in the style of
the webapp that lives at ../notes (see the beautiful theme?). This app
will be a "recipes app," which will look a lot like the central page
of the notes app, in the sense that there will be lots of individual
recipes which are .md files on disc and tracked via Git add /
remove. Now, the format will be a bit special, there will be a uniform
layout for each reipe named ingredients. There should be a pane /
screen which lets you add from all of the recipes and then assembles a
shopping list for you. There should also be a notion of a pantry. The
pantry should be binary: we either have an ingredient or we don't,
it's too hard to track quantities in the pantry. Now, the "shopping"
part of the app should allow you to select among the various recipes
you have and to add as many as you want, possibly also in a
quantity. The app should then tell you everyting which you need to
buy--things already in your pantry should also be shown below in an
unobstrusive visualization showing you already have them. Also, there
should be a way to easily take the assembled shopping pane and say "in
pantry" (in which case it gets added to your pantry) or to take
something from your pantry and to mark it as actually not in your
pantry (in which case, you rerender something back into the
cart). Please make the app happen, thank you Claude. Be sure to keep
the design simple, secure, and write plenty of high-quality tests. You
are an expert Rust + Axum + Sled user.

---

## Active shopping trip (in-store checklist)

Saving a trip from the Shopping page (`POST /api/shopping/save-trip`) marks it the
**active trip**. A single active-trip pointer lives in the `trip_meta` Sled tree
(key `active_trip`). A small script in `base_html` polls `GET /api/trip/active` on
every page and renders a persistent green "🛒 Go to active shopping trip" banner
(with live progress) until the trip is closed. The banner hides itself on the
trip's own page.

The trip page (`/shopping/trip/{id}`, durable + bookmarkable — the "Copy link"
button copies it) is an in-store checklist:

- **Grouped by store section.** `src/aisle.rs` classifies each buyable item into a
  section (`SECTIONS`, in walking order) via a keyword heuristic, with a
  per-ingredient manual override persisted in the `aisle_overrides` tree. Each row
  has a dropdown to move an item to a different section; the correction sticks
  across trips (`POST /api/shopping/section {name, section}`).
- **Check off as you go.** Each checkbox `POST`s to
  `/api/shopping/trip/{id}/check {key, checked}` immediately, so a page refresh
  restores progress. Check-off state is stored on `SavedTrip.checked` (a list of
  item keys; see `shopping::item_key`). Items already in the pantry are not on the
  checklist — they appear unobtrusively under "Already Have".
- **Close / reopen.** "Done shopping" → `POST /api/shopping/trip/{id}/close` sets
  `SavedTrip.closed` and clears the active pointer (banner disappears). A closed
  trip can be reopened (`/reopen`), which makes it active again.

`shopping::active_trip` self-heals a pointer to a missing or closed trip.

## Weekly meal plan (`src/mealplan.rs` + `/plan`)

One `MealPlan` record per week (Sled tree `meal_plans`, keyed by the week's
**Monday** `YYYY-MM-DD`; `mealplan::week_start_of` normalizes any date).
A `PlannedMeal` is either a recipe reference (`recipe_key` + snapshotted
`title` + `multiplier`) or free text ("leftovers"). Pages: `/plan` (this
week) and `/plan/{monday}` (canonical — other days redirect). APIs:
`POST /api/plan/meal` (add), `POST /api/plan/meal/remove`, and
`POST /api/plan/trip` with `action: "build" | "link" | "unlink"`.

**Trip association:** `build` runs the plan's recipe meals through
`build_shopping_list` → `save_trip`, makes it the **active trip** (banner and
in-store checklist work as usual), and stores `trip_id` on the plan; `link`
attaches an existing saved trip (the page lists recent ones to click);
`unlink` detaches. The plan page shows the linked trip's live progress.

**The week board is the shared kcal calendar component**, vendored at
`src/vendor/kcal.{js,css}` and inlined via `include_str!` (this app serves no
static files). Canonical source: mycloud repo `/srv/apps/shared/kcal/` — edit
there and run its `sync.sh`; never edit the vendored copies. The template
(`templates/mealplan.rs`) mounts it with `view: "week"`, `weekStart: 1`,
`header: false` (prev/today/next are server-side links), a `renderChip` for
meal chips, and `dayFooter` for the per-day ＋ add button.

Deferred: drag-to-move meals between days (remove + re-add covers it).

## MCP server (`src/mcp.rs`)

Hand-rolled JSON-RPC 2.0 over a single `POST /mcp`, modeled on `../notes/src/mcp.rs`.
Bearer-token auth (constant-time compare) against `RECIPES_MCP_TOKEN`; returns 503 when
the env var is unset. Caddy bypasses Authelia for the `/mcp` path (see `/srv/CLAUDE.md`).
No SSE — every tool call is a synchronous request/response.

**Methods:** `initialize`, `ping`, `tools/list`, `tools/call`, `resources/list` (empty),
`prompts/list` (empty). Notifications (id-less) are accepted and dropped.

**Tools:**
- `list_recipes(query?, tag?, limit?, offset?)` — summaries (key, title, tags, servings, ingredient_count)
- `read_recipe(key)` — title, tags, servings, structured ingredients, markdown body
- `search_recipes(query, limit?)` — substring match across title/tags/ingredients/body
- `build_shopping_list(selections:[{key, multiplier}])` — aggregates ingredients by (name, unit)
  across recipes, annotated with sources and pantry status; returns `to_buy` / `have` groups and `unknown_keys`
- `publish_trip(selections, title?, notes?)` — snapshot a trip to a durable short `/t/{slug}` page
- `list_trips()` / `delete_trip(slug, confirm:true)` — published-trip management
- `get_meal_plan(week_of?)` — the weekly plan (Mon-Sun days with meals + associated trip summary)
- `plan_meal(date, recipe_key?|title?, multiplier?)` / `remove_meal(date, meal_id)` — edit the plan
- `list_pantry()` / `set_pantry(name, in_pantry)` — binary pantry have/don't-have state
- `create_recipe(filename, title, servings?, tags?, ingredients, body)` — git-committed
- `update_recipe(key, title, servings?, tags?, ingredients, body)` — git-committed
- `delete_recipe(key, confirm:true)` — git rm + commit

Tool results return `{ content: [{type:"text", text: pretty-json}], structuredContent: <same>, isError: false }`.
Errors return `isError: true` with the message in `content[0].text`.

**Desktop link-up:**
```bash
claude mcp add --transport http recipes-server https://recipes.kmicinski.com/mcp \
  --header "Authorization: Bearer $RECIPES_MCP_TOKEN"
```
