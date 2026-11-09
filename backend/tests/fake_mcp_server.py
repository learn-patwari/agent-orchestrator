"""A tiny MCP server over stdio for tests: newline-delimited JSON-RPC 2.0 with initialize, tools/list, tools/call."""
import json
import os
import sys

TOOLS = [
    {"name": "echo", "description": "Echo the text back.",
     "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}},
    {"name": "env_value", "description": "Return the value of an environment variable.",
     "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}}, "required": ["name"]}},
]


def reply(id_, result=None, error=None):
    msg = {"jsonrpc": "2.0", "id": id_}
    msg.update({"error": error} if error else {"result": result})
    sys.stdout.write(json.dumps(msg) + "\n")
    sys.stdout.flush()


for line in sys.stdin:
    req = json.loads(line)
    method, id_ = req.get("method"), req.get("id")
    if id_ is None:  # a notification such as notifications/initialized
        continue
    if method == "initialize":
        reply(id_, {"protocolVersion": "2024-11-05", "capabilities": {"tools": {}},
                    "serverInfo": {"name": "fake", "version": "1"}})
    elif method == "tools/list":
        reply(id_, {"tools": TOOLS})
    elif method == "tools/call":
        p = req["params"]
        a = p.get("arguments", {})
        if p["name"] == "echo":
            reply(id_, {"content": [{"type": "text", "text": "echo: " + a["text"]}]})
        elif p["name"] == "env_value":
            reply(id_, {"content": [{"type": "text", "text": os.environ.get(a["name"], "(unset)")}]})
        else:
            reply(id_, error={"code": -32601, "message": "unknown tool " + p["name"]})
    else:
        reply(id_, error={"code": -32601, "message": "unknown method " + str(method)})
