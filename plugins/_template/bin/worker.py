#!/usr/bin/env python3
"""Minimal pc-worker-v1 local-worker implementation for the PortCanvas plugin template."""
import json
import sys


def emit(payload: dict) -> None:
    sys.stdout.write(json.dumps(payload, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def emit_patch(patch: dict) -> None:
    emit({"type": "state_patch", "patch": patch})


def main() -> None:
    message = "Hello PortCanvas"
    count = 0

    for raw in sys.stdin:
        line = raw.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue

        kind = msg.get("type", "")
        if kind == "init":
            emit_patch({
                "status": "就绪",
                "statusPhase": 2,
                "message": message,
                "count": count,
                "lastEcho": "—",
            })
        elif kind == "set_state":
            if msg.get("key") == "message":
                message = str(msg.get("value", ""))
        elif kind == "action":
            if msg.get("action") == "ping":
                count += 1
                echo = message.strip() or "(empty)"
                emit_patch({
                    "status": f"已响应 #{count}",
                    "statusPhase": 2,
                    "count": count,
                    "lastEcho": echo,
                })
                emit({"type": "notify", "message": f"Worker 收到：{echo}"})
        elif kind == "shutdown":
            break


if __name__ == "__main__":
    main()
