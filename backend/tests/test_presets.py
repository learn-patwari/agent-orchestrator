from app.providers import registry
from app.providers.presets import PRESETS


def test_presets_cover_the_major_providers_and_custom():
    ids = {p["id"] for p in PRESETS}
    for want in ["openai", "anthropic", "gemini", "azure_foundry", "mistral", "groq", "xai", "deepseek",
                 "openrouter", "together", "fireworks", "perplexity", "cohere", "ollama", "lmstudio",
                 "claude_cli", "custom"]:
        assert want in ids, want
    assert len(ids) == len(PRESETS)  # no duplicate ids


def test_every_preset_uses_an_offered_protocol_and_has_a_sane_url():
    offered = set(registry.ADAPTERS)
    for p in PRESETS:
        assert p["type"] in offered, p["id"]
        if p["id"] not in ("custom", "claude_cli", "azure_foundry"):
            assert p["base_url"].startswith(("https://", "http://localhost")), p["id"]
        assert isinstance(p["needs_key"], bool) and p["name"]


def test_local_and_login_presets_need_no_key_and_custom_has_no_url():
    by = {p["id"]: p for p in PRESETS}
    assert not by["ollama"]["needs_key"] and not by["lmstudio"]["needs_key"] and not by["claude_cli"]["needs_key"]
    assert by["custom"]["base_url"] == "" and by["openai"]["needs_key"]


def test_presets_endpoint_requires_login_and_lists_them(client):
    assert client.get("/api/providers/presets").status_code == 401
    client.post("/api/auth/setup", json={"password": "correct-horse-1"})
    r = client.get("/api/providers/presets")
    assert r.status_code == 200 and len(r.json()["presets"]) == len(PRESETS)


def test_a_preset_can_be_added_without_a_key_when_local(authed):
    r = authed.post("/api/providers", json={"name": "Local Ollama", "type": "openai",
                                            "base_url": "http://localhost:11434/v1"})
    assert r.status_code == 200
