"""Single local user (FR-2, Q-8): password set on first run, signed session token."""
import hashlib
import hmac
import os
import secrets
import time

from fastapi import Cookie, Header, HTTPException

from . import db
from .secrets_store import _key

SESSION_TTL = 7 * 24 * 3600
users = db.DocRepo("users")


def _hash(password: str, salt: bytes) -> str:
    return hashlib.scrypt(password.encode(), salt=salt, n=2**14, r=8, p=1).hex()


def has_user() -> bool:
    return bool(users.list())


def set_password(password: str) -> None:
    if len(password) < 8:
        raise HTTPException(400, "password must be at least 8 characters")
    if has_user():
        raise HTTPException(409, "already set up")
    salt = os.urandom(16)
    users.create({"salt": salt.hex(), "hash": _hash(password, salt)})


def verify_password(password: str) -> bool:
    u = users.list()
    if not u:
        return False
    return hmac.compare_digest(_hash(password, bytes.fromhex(u[0]["salt"])), u[0]["hash"])


def _sign(payload: str) -> str:
    return hmac.new(_key(), payload.encode(), hashlib.sha256).hexdigest()


def make_token() -> str:
    exp = str(int(time.time()) + SESSION_TTL)
    nonce = secrets.token_hex(8)
    p = f"{exp}.{nonce}"
    return f"{p}.{_sign(p)}"


def valid_token(token: str | None) -> bool:
    if not token:
        return False
    try:
        exp, nonce, sig = token.split(".")
    except ValueError:
        return False
    return hmac.compare_digest(sig, _sign(f"{exp}.{nonce}")) and int(exp) > time.time()


def require_auth(
    authorization: str | None = Header(default=None),
    ao_session: str | None = Cookie(default=None),
) -> None:
    token = ao_session
    if authorization and authorization.lower().startswith("bearer "):
        token = authorization[7:]
    if not valid_token(token):
        raise HTTPException(401, "not authenticated")
