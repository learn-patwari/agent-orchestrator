"""Named provider presets. Each maps to one of the protocol adapters (openai = /chat/completions,
anthropic = /v1/messages) plus a default base URL, so adding a provider is pick, paste key, test.
URLs are the vendors' documented OpenAI-compatible endpoints; check them if a vendor changes theirs.
Not covered (they need request signing or OAuth, not a bearer key): AWS Bedrock, Google Vertex AI.
"""
from __future__ import annotations


def _p(id, name, type, base_url, needs_key=True, note=""):
    return {"id": id, "name": name, "type": type, "base_url": base_url, "needs_key": needs_key, "note": note}


PRESETS = [
    _p("openai", "OpenAI", "openai", "https://api.openai.com/v1"),
    _p("anthropic", "Anthropic (API key)", "anthropic", "https://api.anthropic.com"),
    _p("claude_cli", "Claude (your own login, no API key)", "claude_cli", "", False,
       "Needs a one-time /login in the claude program. Models: sonnet, opus, haiku."),
    _p("gemini", "Google Gemini", "openai", "https://generativelanguage.googleapis.com/v1beta/openai"),
    _p("azure_foundry", "Microsoft Foundry / Azure OpenAI", "openai", "", True,
       "Base URL: https://<your-resource>.openai.azure.com/openai/v1 . Model = your deployment name."),
    _p("mistral", "Mistral", "openai", "https://api.mistral.ai/v1"),
    _p("groq", "Groq", "openai", "https://api.groq.com/openai/v1"),
    _p("xai", "xAI (Grok)", "openai", "https://api.x.ai/v1"),
    _p("deepseek", "DeepSeek", "openai", "https://api.deepseek.com"),
    _p("openrouter", "OpenRouter (many models, one key)", "openai", "https://openrouter.ai/api/v1"),
    _p("together", "Together AI", "openai", "https://api.together.xyz/v1"),
    _p("fireworks", "Fireworks AI", "openai", "https://api.fireworks.ai/inference/v1"),
    _p("perplexity", "Perplexity", "openai", "https://api.perplexity.ai"),
    _p("cohere", "Cohere", "openai", "https://api.cohere.ai/compatibility/v1"),
    _p("ollama", "Ollama (local)", "openai", "http://localhost:11434/v1", False, "Runs on this computer."),
    _p("lmstudio", "LM Studio (local)", "openai", "http://localhost:1234/v1", False, "Runs on this computer."),
    _p("custom", "Custom (any compatible endpoint)", "openai", "", True,
       "Pick the protocol (openai or anthropic style), then enter your own URL and key."),
]
