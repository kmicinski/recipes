#!/usr/bin/env python3
"""Read-only integrity validator for the hand-authored recipe book."""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

BOOK = Path(__file__).resolve().parents[1] / "book" / "book.jsonl"
BANNED = re.compile(
    r"\b(?:fish|seafood|salmon|tuna|cod|trout|tilapia|shrimp|prawn|crab|lobster|clam|mussel|anchov(?:y|ies)|sardine|oyster sauce|fish sauce|worcestershire)\b",
    re.IGNORECASE,
)
MEALS = {"breakfast", "lunch", "dinner"}
REQUIRED = {
    "id", "title", "servings", "tags", "protein", "method", "cuisine",
    "ingredients", "body_markdown",
}


def fail(message: str) -> None:
    raise ValueError(message)


def validate(path: Path = BOOK) -> list[dict]:
    lines = path.read_text(encoding="utf-8").splitlines()
    if len(lines) != 128:
        fail(f"expected 128 JSONL records, found {len(lines)}")

    recipes: list[dict] = []
    titles: set[str] = set()
    bodies: set[str] = set()
    meal_counts = {meal: 0 for meal in MEALS}

    for number, line in enumerate(lines, 1):
        try:
            recipe = json.loads(line)
        except json.JSONDecodeError as exc:
            fail(f"line {number}: invalid JSON: {exc}")
        if set(recipe) != REQUIRED:
            fail(f"line {number}: fields must be exactly {sorted(REQUIRED)}")
        expected_id = f"bk-{number:04d}"
        if recipe["id"] != expected_id:
            fail(f"line {number}: expected id {expected_id!r}")
        title = recipe["title"]
        if not isinstance(title, str) or len(title.strip()) < 8 or title in titles:
            fail(f"line {number}: title is missing, too short, or duplicated")
        titles.add(title)
        if not isinstance(recipe["servings"], int) or recipe["servings"] < 1:
            fail(f"line {number}: servings must be a positive integer")

        tags = recipe["tags"]
        if not isinstance(tags, list) or len(tags) < 4 or len(tags) != len(set(tags)):
            fail(f"line {number}: tags must contain at least four unique values")
        meals = MEALS.intersection(tags)
        if len(meals) != 1:
            fail(f"line {number}: tags must contain exactly one meal tag")
        meal_counts[next(iter(meals))] += 1
        for field in ("protein", "method", "cuisine"):
            value = recipe[field]
            if not isinstance(value, str) or not value.strip() or value not in tags:
                fail(f"line {number}: {field} must be a nonempty tag")

        ingredients = recipe["ingredients"]
        if not isinstance(ingredients, list) or len(ingredients) < 6:
            fail(f"line {number}: at least six ingredients are required")
        ingredient_names: set[str] = set()
        for ingredient in ingredients:
            if set(ingredient) != {"name", "qty", "unit"}:
                fail(f"line {number}: malformed ingredient")
            name, qty, unit = ingredient["name"], ingredient["qty"], ingredient["unit"]
            if not isinstance(name, str) or len(name.strip()) < 2 or name.casefold() in ingredient_names:
                fail(f"line {number}: ingredient names must be meaningful and unique")
            ingredient_names.add(name.casefold())
            if isinstance(qty, bool) or not isinstance(qty, (int, float)) or qty <= 0:
                fail(f"line {number}: ingredient quantity must be positive")
            if not isinstance(unit, str) or not unit.strip():
                fail(f"line {number}: ingredient unit is required")

        body = recipe["body_markdown"]
        if not isinstance(body, str) or not body.startswith("## Prep ahead\n"):
            fail(f"line {number}: body must begin with the Prep ahead section")
        if body.count("## Prep ahead") != 1 or body.count("## Day of (~20 min)") != 1:
            fail(f"line {number}: body must contain both required sections exactly once")
        if body.index("## Prep ahead") > body.index("## Day of (~20 min)"):
            fail(f"line {number}: recipe sections are out of order")
        if len(body) < 180 or body in bodies:
            fail(f"line {number}: directions are too short or duplicated")
        bodies.add(body)
        if BANNED.search(json.dumps(recipe, ensure_ascii=False)):
            fail(f"line {number}: prohibited seafood-related term")
        recipes.append(recipe)

    if any(meal_counts[meal] < 20 for meal in MEALS):
        fail(f"meal distribution is insufficiently diverse: {meal_counts}")
    if len({recipe["cuisine"] for recipe in recipes}) < 15:
        fail("at least 15 cuisines are required")
    if len({recipe["protein"] for recipe in recipes}) < 8:
        fail("at least 8 protein categories are required")
    if len({recipe["method"] for recipe in recipes}) < 7:
        fail("at least 7 cooking methods are required")
    return recipes


if __name__ == "__main__":
    try:
        validated = validate()
    except (OSError, ValueError) as exc:
        print(f"INVALID: {exc}", file=sys.stderr)
        raise SystemExit(1)
    print(f"OK: {len(validated)} authored recipes validated from {BOOK}")
