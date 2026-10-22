"""Reuse the existing saved-site bridge; keep this project's evidence here."""
import importlib.util
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('bridge', ROOT.parent / 'luby/03-实现与脚本/codex_board_bridge.py')
bridge = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bridge)
bridge.ROOT = ROOT
bridge.EVIDENCE = ROOT / '05-测试与验证/local/board'

if __name__ == '__main__':
    action, name = sys.argv[1:3]
    if action == 'submit':
        request = ROOT / (sys.argv[3] if len(sys.argv)>3 else '02-方案与设计/demo-v0.1-board-request.md')
        result = bridge.submit(name, request, 600)
    elif action == 'collect':
        result = bridge.collect(name)
    else:
        result = bridge.board_status(name)
    print(json.dumps(result, ensure_ascii=False, indent=2))
