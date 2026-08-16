#!/usr/bin/env python3
"""Call one req-engine MCP tool over stdio and print JSON.

Usage:
  python mcp_call.py --exe path --home path --pair disc_xxx tools/list
  python mcp_call.py --exe path --home path --pair disc_xxx create_requirement --args '{"title":"..."}'
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import threading
import time


def send_line(proc, obj):
    proc.stdin.write((json.dumps(obj, ensure_ascii=False) + "\n").encode("utf-8"))
    proc.stdin.flush()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--exe", required=True)
    ap.add_argument("--home", required=True)
    ap.add_argument("--pair", default="")
    ap.add_argument("--role", default="", choices=["", "planner", "foreman"])
    ap.add_argument("--token", default="")
    ap.add_argument("tool")
    ap.add_argument("--args", default="{}")
    args = ap.parse_args()
    tool_args = json.loads(args.args)

    cmd = [args.exe, "mcp", "--home", args.home]
    if args.pair:
        cmd.extend(["--pair", args.pair])
    elif args.role and args.token:
        cmd.extend(["--role", args.role, "--token", args.token])
    else:
        print("need --pair CODE or --role + --token", file=sys.stderr)
        return 2

    proc = subprocess.Popen(
        cmd,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        bufsize=0,
    )
    chunks: list[bytes] = []

    def pump():
        while True:
            b = proc.stdout.read(1)
            if not b:
                break
            chunks.append(b)

    threading.Thread(target=pump, daemon=True).start()

    send_line(
        proc,
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "mcp_call", "version": "0.1"},
            },
        },
    )
    time.sleep(0.25)
    send_line(proc, {"jsonrpc": "2.0", "method": "notifications/initialized"})
    time.sleep(0.1)
    if args.tool in ("tools/list", "list_tools"):
        send_line(proc, {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
    else:
        send_line(
            proc,
            {
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {"name": args.tool, "arguments": tool_args},
            },
        )
    time.sleep(0.6)
    try:
        proc.stdin.close()
    except Exception:
        pass
    time.sleep(0.2)
    proc.kill()
    out = b"".join(chunks).decode("utf-8", "replace")
    err = proc.stderr.read().decode("utf-8", "replace")
    results = []
    for line in out.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            results.append(json.loads(line))
        except json.JSONDecodeError:
            results.append({"raw": line})
    payload = next((r for r in results if r.get("id") == 2), None)
    if payload is None:
        print(json.dumps({"ok": False, "error": "no tool result", "stderr": err[-800:]}, ensure_ascii=False))
        sys.exit(2)
    if "error" in payload:
        print(json.dumps({"ok": False, "error": payload["error"], "stderr": err[-400:]}, ensure_ascii=False))
        sys.exit(1)
    print(json.dumps({"ok": True, "result": payload.get("result")}, ensure_ascii=False))


if __name__ == "__main__":
    main()
