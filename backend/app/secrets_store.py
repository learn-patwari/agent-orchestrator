"""Encrypt secrets (API keys) at rest and redact them from text (A-11, FR-4)."""
import os
import re

from cryptography.fernet import Fernet

from .config import data_dir


def _key() -> bytes:
    env = os.environ.get("AO_SECRET_KEY")
    if env:
        return env.encode()
    f = data_dir() / "secret.key"
    if not f.exists():
        f.write_bytes(Fernet.generate_key())
        try:
            os.chmod(f, 0o600)
        except OSError:
            pass
    return f.read_bytes()


def encrypt(plain: str) -> str:
    return Fernet(_key()).encrypt(plain.encode()).decode()


def decrypt(token: str) -> str:
    return Fernet(_key()).decrypt(token.encode()).decode()


def mask(plain: str) -> str:
    return "****" + plain[-4:] if len(plain) > 8 else "****"


_PATTERNS = [
    re.compile(r"sk-[A-Za-z0-9_\-]{16,}"),
    re.compile(r"gh[pousr]_[A-Za-z0-9]{20,}"),
    re.compile(r"(?i)bearer\s+[A-Za-z0-9._\-]{16,}"),
    re.compile(r"(?i)(api[_-]?key|password|secret|token)\s*[:=]\s*\S{6,}"),
]


def looks_like_secret(text: str) -> bool:
    return any(p.search(text) for p in _PATTERNS)


def redact(text: str) -> str:
    for p in _PATTERNS:
        text = p.sub("[REDACTED]", text)
    return text
