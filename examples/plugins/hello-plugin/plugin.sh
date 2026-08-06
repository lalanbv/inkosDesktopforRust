#!/usr/bin/env bash
# 示例进程隔离插件 —— 实现 JSON-RPC over stdio 协议。
#
# inkos 插件进程隔离契约：
#   - 宿主通过 stdin 发一行 JSON-RPC 请求（RpcRequest）
#   - 插件从 stdout 回一行 JSON-RPC 响应（RpcResponse）
#   - 每行一个完整 JSON 对象，换行分隔
#
# 本插件支持的方法：
#   - plugin.echo   : 原样回显 params
#   - plugin.upper  : 把 params.text 转大写
#   - plugin.health : 返回固定健康信息
#
# 依赖：jq（JSON 命令行处理器）。macOS 自带，Linux 多数发行版预装。
set -euo pipefail

# 逐行读取请求（每行一个 JSON-RPC 请求）
while IFS= read -r line; do
    # 空行跳过
    [ -z "$line" ] && continue

    id=$(echo "$line" | jq -r '.id // null')
    method=$(echo "$line" | jq -r '.method // ""')

    case "$method" in
        plugin.echo)
            params=$(echo "$line" | jq -c '.params // null')
            echo "{\"jsonrpc\":\"2.0\",\"id\":${id},\"result\":{\"echo\":${params}}}"
            ;;
        plugin.upper)
            text=$(echo "$line" | jq -r '.params.text // ""')
            upper=$(printf '%s' "$text" | tr '[:lower:]' '[:upper:]')
            # printf 不加 trailing newline，避免 jq -Rs 把换行也 slurp 进字符串
            echo "{\"jsonrpc\":\"2.0\",\"id\":${id},\"result\":{\"text\":$(printf '%s' "$upper" | jq -Rs .)}}"
            ;;
        plugin.health)
            echo "{\"jsonrpc\":\"2.0\",\"id\":${id},\"result\":{\"status\":\"healthy\",\"name\":\"hello-plugin\",\"version\":\"1.0.0\"}}"
            ;;
        *)
            msg=$(echo "$method" | jq -Rs .)
            echo "{\"jsonrpc\":\"2.0\",\"id\":${id},\"error\":{\"code\":-32601,\"message\":${msg}}}"
            ;;
    esac
done
