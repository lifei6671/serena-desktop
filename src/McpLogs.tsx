import { useEffect, useRef, useState } from "react";
import { api } from "./api";

export function McpLogs() {
  const [lines, setLines] = useState<string[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const viewport = useRef<HTMLDivElement>(null);
  const following = useRef(true);

  useEffect(() => {
    let active = true;
    let timer: number | undefined;
    const refresh = async () => {
      try {
        const next = await api.mcpLogs();
        if (active) {
          setLines(next);
          setError(null);
        }
      } catch (reason) {
        if (active) setError(String(reason));
      } finally {
        if (active) timer = window.setTimeout(refresh, 1000);
      }
    };
    void refresh();
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, []);

  useEffect(() => {
    if (following.current && viewport.current) {
      viewport.current.scrollTop = viewport.current.scrollHeight;
    }
  }, [lines]);

  return (
    <section className="logs-page">
      <div className="page-heading">
        <div>
          <h1>MCP 日志</h1>
          <p>本次应用运行的 MCP 连接入口日志 · 保留最近 500 条</p>
        </div>
        <button className="ghost-button" onClick={() => {
          following.current = true;
          if (viewport.current) viewport.current.scrollTop = viewport.current.scrollHeight;
        }}>回到最新</button>
      </div>
      {error && <p className="inline-error" role="alert">日志读取失败：{error}</p>}
      <div className="log-stream" ref={viewport} role="region" aria-label="MCP 运行日志" tabIndex={0}
        onScroll={(event) => {
          const node = event.currentTarget;
          following.current = node.scrollHeight - node.scrollTop - node.clientHeight < 32;
        }}>
        {lines?.length ? <pre>{lines.join("\n")}</pre> :
          <p className="log-empty">{lines === null ? "正在读取日志…" : "暂无 MCP 日志，启动连接入口或发起请求后会在这里显示。"}</p>}
      </div>
    </section>
  );
}
