"""Build adapters from stored provider records; external providers stay hidden (FR-9)."""
import time

import httpx

from .. import db, secrets_store
from ..config import DEFAULT_SETTINGS
from .anthropic_compat import AnthropicCompatProvider
from .base import LLMProvider
from .openai_compat import OpenAICompatProvider
from .pricing import cost

ADAPTERS = {"openai": OpenAICompatProvider, "anthropic": AnthropicCompatProvider}
EXTERNAL_TYPE = "external"  # reserved: delegating to another product/CLI/hosted agent


def get_settings() -> dict:
    stored = db.kv_get("settings", {})
    flags = {**DEFAULT_SETTINGS["feature_flags"], **stored.get("feature_flags", {})}
    return {**DEFAULT_SETTINGS, **stored, "feature_flags": flags}


def external_enabled() -> bool:
    return bool(get_settings()["feature_flags"].get("external_providers"))


def allowed_types() -> list[str]:
    return list(ADAPTERS) + ([EXTERNAL_TYPE] if external_enabled() else [])


def build(record: dict, client: httpx.Client | None = None) -> LLMProvider:
    t = record["type"]
    if t == EXTERNAL_TYPE:
        raise ValueError("external providers are not available yet")
    if t not in ADAPTERS:
        raise ValueError(f"unknown provider type {t}")
    return ADAPTERS[t](record["base_url"], secrets_store.decrypt(record["api_key_enc"]), client=client)


def record_call(role: str, provider_id: str, model: str, usage, run_id: str | None = None) -> dict:
    return db.DocRepo("llm_calls").create({
        "role": role, "provider_id": provider_id, "model": model, "run_id": run_id,
        "input_tokens": usage.input_tokens, "output_tokens": usage.output_tokens,
        "cost_usd": cost(model, usage.input_tokens, usage.output_tokens), "at": time.time(),
    })
