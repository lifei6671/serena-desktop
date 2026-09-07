import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { useEffect, useRef, useState } from "react";
import { api } from "./api";

export function McpLogs() {
  const [lines, setLines] = useState<string[] | null>(null);
  const [clearing, setClearing] = useState(false);
  const clearingRef = useRef(false);
  const readEpoch = useRef(0);

  const viewport = useRef<HTMLDivElement>(null);
  const following = useRef(true);

  useEffect(() => {
    let active = true;
    let timer: number | undefined;
    let lastError = "";
    const refresh = async () => {
      const epoch = readEpoch.current;
      try {
        if (clearingRef.current) return;
        const next = await api.mcpLogs();
        if (active && epoch === readEpoch.current) {
          setLines(next);
          lastError = "";
        }
      } catch (reason) {
        if (
          active &&
          epoch === readEpoch.current &&
          lastError !== String(reason)
        ) {
          lastError = String(reason);
          toast.error(`日志读取失败：${lastError}`, { id: "mcp-log-error" });
        }
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

  const clear = async () => {
    if (clearingRef.current) return;
    clearingRef.current = true;
    setClearing(true);
    readEpoch.current++;
    try {
      await api.clearMcpLogs();
      setLines([]);
      following.current = true;
      toast.success("日志已清空");
    } catch (reason) {
      toast.error(`清空日志失败：${String(reason)}`);
    } finally {
      clearingRef.current = false;
      setClearing(false);
    }
  };

  return (
    <section className="logs-page">
      <div className="page-heading">
        <div>
          <h1>MCP 日志</h1>
          <p>本次应用运行的 MCP 连接入口日志 · 保留最近 500 条</p>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <Button
            variant="outline"
            disabled={clearing}
            aria-busy={clearing}
            onClick={() => void clear()}
          >
            {clearing && (
              <Spinner data-icon="inline-start" aria-hidden="true" />
            )}
            {clearing ? "清空中…" : "清空日志"}
          </Button>
          <Button
            variant="outline"
            onClick={() => {
              following.current = true;
              if (viewport.current)
                viewport.current.scrollTop = viewport.current.scrollHeight;
            }}
          >
            回到最新
          </Button>
        </div>
      </div>

      <div
        className="log-stream dark"
        ref={viewport}
        role="region"
        aria-label="MCP 运行日志"
        tabIndex={0}
        onScroll={(event) => {
          const node = event.currentTarget;
          following.current =
            node.scrollHeight - node.scrollTop - node.clientHeight < 32;
        }}
      >
        {lines?.length ? (
          <pre>{lines.join("\n")}</pre>
        ) : (
          <p className="log-empty">
            {lines === null
              ? "正在读取日志…"
              : "暂无 MCP 日志，启动连接入口或发起请求后会在这里显示。"}
          </p>
        )}
      </div>
    </section>
  );
}
