"""正常fixture専用。GUIの手動許可後にMCP経由で検討操作を検証する。"""
import argparse
import json
from pathlib import Path
import queue
import subprocess
import threading
import time
import uuid


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", required=True)
    parser.add_argument("--pipe", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--scenario", choices=["review", "comparison"], required=True)
    parser.add_argument("--release-marker")
    args = parser.parse_args()
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    events = []
    child = subprocess.Popen([args.exe, "--gui-pipe", args.pipe], stdin=subprocess.PIPE,
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                             encoding="utf-8", creationflags=subprocess.CREATE_NO_WINDOW)
    replies = queue.Queue()

    def read():
        for line in child.stdout:
            replies.put(json.loads(line))
    threading.Thread(target=read, daemon=True).start()
    number = 0

    def save(event):
        events.append(event)
        output.write_text(json.dumps(events, ensure_ascii=False, indent=2), encoding="utf-8")

    def rpc(method, params, notify=False):
        nonlocal number
        number += 1
        body = dict(jsonrpc="2.0", method=method, params=params)
        if not notify:
            body["id"] = number
        child.stdin.write(json.dumps(body, ensure_ascii=False) + "\n")
        child.stdin.flush()
        if notify:
            return None
        response = replies.get(timeout=30)
        assert response.get("id") == number, (number, response)
        if "error" in response:
            raise RuntimeError(response)
        return response["result"]

    def envelope(body):
        result = rpc("tools/call", dict(name="gui_request", arguments=dict(envelope=body)))
        if result.get("isError"):
            raise RuntimeError(result)
        response = json.loads(result["content"][0]["text"])
        assert response["id"] == body["id"], (body["id"], response)
        return response

    try:
        initialized = rpc("initialize", dict(protocolVersion="2024-11-05", capabilities={},
                          clientInfo=dict(name="sepika-fixture-probe", version="1")))
        assert initialized["serverInfo"]["name"] == "sepika-gui-bridge"
        rpc("notifications/initialized", {}, notify=True)
        tools = rpc("tools/list", {})
        assert [tool["name"] for tool in tools["tools"]] == ["gui_request"]
        save(dict(stage="MCP initialized", tools=[tool["name"] for tool in tools["tools"]]))
        deadline = time.monotonic() + 180
        while True:
            connected = envelope(dict(version=1, id=str(uuid.uuid4()), operation="connect"))
            if connected.get("error"):
                raise RuntimeError(connected)
            connected = connected["result"]
            if connected["status"] == "approved":
                break
            if time.monotonic() > deadline:
                raise TimeoutError("GUIでの許可がありません")
            time.sleep(0.5)
        save(dict(stage="GUI approved", connection=connected))
        context = dict(version=1, session=connected["session"], project=connected["project"],
                       study=connected["study"], request=str(uuid.uuid4()))
        current = connected["baseline"]

        def call(operation, arguments=None, expect_error=None):
            body = dict(context, id=str(uuid.uuid4()), operation=operation,
                        proposal=current["proposal"], input=current, arguments=arguments)
            response = envelope(body)
            save(dict(operation=operation, response=response))
            if expect_error:
                assert response["error"]["code"] == expect_error, response
            elif response.get("error"):
                raise RuntimeError(response)
            return response.get("result")

        call("begin_request")
        study = call("study")
        fixed = call("fixed_targets", dict(offset=0, limit=100))["items"]
        call("conditions")
        candidates = call("sections", dict(offset=0, limit=100))["items"]
        assert study["fixed_count"] == len(fixed)
        if args.scenario == "review":
            assert connected["mode"] == "Review"
            call("create_comparison", dict(name="作成不可"), expect_error="permission_denied")
            call("calculate", expect_error="permission_denied")
            call("submit", dict(explanation="固定対象と条件を参照しました。計算していません。",
                                unchecked=["解析・検定", "人による採用"], references=[]))
        else:
            assert connected["mode"] == "Comparison"
            current = call("create_comparison", dict(name="MCP実通信比較"))["input"]
            candidate = next(section for section in candidates if section["id"] == 2)
            current = call("assign_section", dict(targets=fixed, section=candidate["id"]))["input"]
            call("changes", dict(offset=0, limit=100))
            job = call("calculate")
            deadline = time.monotonic() + 120
            while True:
                state = call("job", dict(id=job["id"]))
                if state["state"] == "calculated":
                    break
                assert state["state"] == "running", state
                if time.monotonic() > deadline:
                    raise TimeoutError("計算が完了しません")
                time.sleep(0.1)
            results = call("results", dict(run_index=state["run_index"], offset=0, limit=100))
            reference = next(row["reference"] for row in results["page"]["items"]
                             if row.get("kind") == "check" and row.get("reference"))
            call("evidence", dict(reference=reference, table="nodes", offset=0, limit=1))
            call("quantity", dict(offset=0, limit=1))
            call("submit", dict(run_index=state["run_index"], explanation="既存計算・検定の同じ根拠を参照しました。",
                                unchecked=["人による採用", "規準全体の確認"], references=[reference]))
        if args.release_marker:
            marker = Path(args.release_marker)
            assert not marker.exists(), "専用の未使用markerを指定してください"
            save(dict(stage="waiting_for_gui_release", marker=str(marker)))
            deadline = time.monotonic() + 600
            while not marker.exists():
                if time.monotonic() > deadline:
                    raise TimeoutError("GUI解除後のmarkerがありません")
                time.sleep(0.1)
            body = dict(context, id=str(uuid.uuid4()), operation="study",
                        proposal=current["proposal"], input=current, arguments=None)
            try:
                response = envelope(body)
            except RuntimeError as error:
                save(dict(stage="old_session_rejected_after_gui_release", error=str(error)))
            else:
                assert response.get("error"), response
                save(dict(stage="old_session_rejected_after_gui_release", response=response))
        save(dict(stage="completed", scenario=args.scenario))
    finally:
        child.stdin.close()
        try:
            child.wait(timeout=10)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()


if __name__ == "__main__":
    main()
