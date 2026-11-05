"""Cost accounting from a user-editable per-model price table (A-18, FR-8).
Prices are USD per 1M tokens. Unknown model gives cost None (never guessed)."""
from .. import db

KEY = "price_table"


def get_table() -> dict:
    return db.kv_get(KEY, {})


def set_table(table: dict) -> None:
    for model, p in table.items():
        if not isinstance(p, dict) or not {"input", "output"} <= p.keys():
            raise ValueError(f"{model}: need input and output prices")
    db.kv_set(KEY, table)


def cost(model: str, input_tokens: int, output_tokens: int, table: dict | None = None) -> float | None:
    p = (table if table is not None else get_table()).get(model)
    if not p:
        return None
    return round((input_tokens * p["input"] + output_tokens * p["output"]) / 1_000_000, 8)
