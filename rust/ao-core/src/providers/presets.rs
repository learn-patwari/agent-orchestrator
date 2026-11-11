//! Named provider presets, each mapped to a protocol adapter plus a default base URL. Same list as presets.py.
//! Not covered (they need request signing or OAuth, not a bearer key): AWS Bedrock, Google Vertex AI.
use serde_json::{json, Value};

pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub kind: &'static str, // "type" in the API: openai | anthropic | claude_cli
    pub base_url: &'static str,
    pub needs_key: bool,
    pub note: &'static str,
}

const fn p(id: &'static str, name: &'static str, kind: &'static str, base_url: &'static str, needs_key: bool, note: &'static str) -> Preset {
    Preset { id, name, kind, base_url, needs_key, note }
}

pub const PRESETS: &[Preset] = &[
    p("openai", "OpenAI", "openai", "https://api.openai.com/v1", true, ""),
    p("anthropic", "Anthropic (API key)", "anthropic", "https://api.anthropic.com", true, ""),
    p("claude_cli", "Claude (your own login, no API key)", "claude_cli", "", false, "Needs a one-time /login in the claude program. Models: sonnet, opus, haiku."),
    p("gemini", "Google Gemini", "openai", "https://generativelanguage.googleapis.com/v1beta/openai", true, ""),
    p("azure_foundry", "Microsoft Foundry / Azure OpenAI", "openai", "", true, "Base URL: https://<your-resource>.openai.azure.com/openai/v1 . Model = your deployment name."),
    p("mistral", "Mistral", "openai", "https://api.mistral.ai/v1", true, ""),
    p("groq", "Groq", "openai", "https://api.groq.com/openai/v1", true, ""),
    p("xai", "xAI (Grok)", "openai", "https://api.x.ai/v1", true, ""),
    p("deepseek", "DeepSeek", "openai", "https://api.deepseek.com", true, ""),
    p("openrouter", "OpenRouter (many models, one key)", "openai", "https://openrouter.ai/api/v1", true, ""),
    p("together", "Together AI", "openai", "https://api.together.xyz/v1", true, ""),
    p("fireworks", "Fireworks AI", "openai", "https://api.fireworks.ai/inference/v1", true, ""),
    p("perplexity", "Perplexity", "openai", "https://api.perplexity.ai", true, ""),
    p("cohere", "Cohere", "openai", "https://api.cohere.ai/compatibility/v1", true, ""),
    p("ollama", "Ollama (local)", "openai", "http://localhost:11434/v1", false, "Runs on this computer."),
    p("lmstudio", "LM Studio (local)", "openai", "http://localhost:1234/v1", false, "Runs on this computer."),
    p("custom", "Custom (any compatible endpoint)", "openai", "", true, "Pick the protocol (openai or anthropic style), then enter your own URL and key."),
];

/// The array `GET /api/providers/presets` returns under "presets".
pub fn as_json() -> Value {
    Value::Array(
        PRESETS
            .iter()
            .map(|x| json!({"id": x.id, "name": x.name, "type": x.kind, "base_url": x.base_url, "needs_key": x.needs_key, "note": x.note}))
            .collect(),
    )
}
