#!/bin/sh
# 模拟 claude 的 stream-json 控制协议,供集成测试使用。
# 约定:收到 initialize 回模型列表;收到用户消息后依次输出
#   流式文字 → 完整消息 → Bash 工具调用 → can_use_tool 请求,
# 然后阻塞读取一行应答,根据 allow/deny 输出工具结果与 result。
while IFS= read -r line; do
  case "$line" in
    *'"subtype":"initialize"'*)
      # MOCK_NO_INIT:模拟卡在启动阶段,永不应答握手
      [ -n "$MOCK_NO_INIT" ] && continue
      echo '{"type":"control_response","response":{"subtype":"success","request_id":"req_init","response":{"models":[{"value":"default","displayName":"默认","description":"d"},{"value":"opus","displayName":"Opus","description":"o"}],"current_permission_mode":"default"}}}'
      ;;
    *'"subtype":"interrupt"'*)
      id=$(printf '%s' "$line" | sed 's/.*"request_id":"\([^"]*\)".*/\1/')
      echo "{\"type\":\"control_response\",\"response\":{\"subtype\":\"success\",\"request_id\":\"$id\",\"response\":{}}}"
      echo '{"type":"result","subtype":"error_during_execution","is_error":true,"result":"","total_cost_usd":0}'
      ;;
    *'"subtype":"set_permission_mode"'*)
      id=$(printf '%s' "$line" | sed 's/.*"request_id":"\([^"]*\)".*/\1/')
      echo "{\"type\":\"control_response\",\"response\":{\"subtype\":\"success\",\"request_id\":\"$id\",\"response\":{}}}"
      ;;
    *'"type":"user"'*)
      case "$line" in
        *SLEEP*) continue ;;
        *EXIT*) echo "boom on stderr" >&2; exit 3 ;;
        # NORESULT:处理完只报告空闲、不发 result(验证忙闲兜底)
        *NORESULT*)
          uuid=$(printf '%s' "$line" | sed 's/.*"uuid":"\([^"]*\)".*/\1/')
          echo '{"type":"system","subtype":"session_state_changed","state":"running"}'
          echo '{"type":"assistant","message":{"id":"m9","content":[{"type":"text","text":"做完了"}]}}'
          echo "{\"type\":\"command_lifecycle\",\"command_uuid\":\"$uuid\",\"state\":\"completed\"}"
          echo '{"type":"system","subtype":"session_state_changed","state":"idle"}'
          continue ;;
      esac
      echo '{"type":"system","subtype":"init","session_id":"sess-1","cwd":"/tmp"}'
      echo '{"type":"stream_event","api_message_id":"m1","event":{"type":"message_start","message":{"id":"m1"}}}'
      echo '{"type":"stream_event","api_message_id":"m1","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}}'
      echo '{"type":"stream_event","api_message_id":"m1","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"你好"}}}'
      echo '{"type":"assistant","message":{"id":"m1","content":[{"type":"text","text":"你好"}]}}'
      echo '{"type":"stream_event","api_message_id":"m1","event":{"type":"content_block_stop","index":0}}'
      echo '{"type":"assistant","message":{"id":"m2","content":[{"type":"tool_use","id":"tu1","name":"Bash","input":{"command":"echo hi"}}]}}'
      echo '{"type":"control_request","request_id":"cr1","request":{"subtype":"can_use_tool","tool_name":"Bash","tool_use_id":"tu1","input":{"command":"echo hi"}}}'
      IFS= read -r resp
      case "$resp" in
        *'"behavior":"allow"'*)
          echo '{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tu1","content":"hi"}]},"tool_use_result":{"stdout":"hi","stderr":""}}'
          echo '{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn","total_cost_usd":0.01,"usage":{"input_tokens":10,"output_tokens":5}}'
          ;;
        *)
          echo '{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tu1","is_error":true,"content":"denied"}]}}'
          echo '{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn","total_cost_usd":0.01,"usage":{"input_tokens":10,"output_tokens":5}}'
          ;;
      esac
      ;;
  esac
done
