import { Copy, Download, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { useEffect, useRef, useState } from "react";
import { api } from "./api";

export function McpLogs() {
  const [lines, setLines] = useState<string[] | null>(null);
  const [clearing, setClearing] = useState(false);
  const [exporting, setExporting] = useState<"copy" | "download" | null>(null);
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

  const exportLogs = async (action: "copy" | "download") => {
    setExporting(action);
    try {
      if (action === "copy") {
        await navigator.clipboard.writeText((lines ?? []).join("\n"));
        toast.success("日志已复制");
      } else if (await api.downloadMcpLogs()) {
        toast.success("日志已保存");
      }
    } catch (reason) {
      toast.error(
        `${action === "copy" ? "复制" : "下载"}日志失败：${String(reason)}`,
      );
    } finally {
      setExporting(null);
    }
  };

  return (
    <section className="logs-page">
      <div className="page-heading">
        <div>
          <h1>MCP 日志</h1>
          <p>MCP 请求、入参及执行状态 · 保留最近 500 条</p>
        </div>
        <div className="flex shrink-0 items-center gap-2">
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

      <div className="log-viewer dark">
        <div className="log-toolbar" role="group" aria-label="日志工具">
          <Button
            variant="outline"
            size="icon"
            title="复制日志"
            aria-label="复制日志"
            disabled={!lines?.length || clearing || exporting !== null}
            aria-busy={exporting === "copy"}
            onClick={() => void exportLogs("copy")}
          >
            {exporting === "copy" ? <Spinner /> : <Copy />}
          </Button>
          <Button
            variant="outline"
            size="icon"
            title="下载日志"
            aria-label="下载日志"
            disabled={!lines?.length || clearing || exporting !== null}
            aria-busy={exporting === "download"}
            onClick={() => void exportLogs("download")}
          >
            {exporting === "download" ? <Spinner /> : <Download />}
          </Button>
          <Button
            variant="outline"
            size="icon"
            title="清空日志"
            aria-label="清空日志"
            disabled={!lines?.length || clearing || exporting !== null}
            aria-busy={clearing}
            onClick={() => void clear()}
          >
            {clearing ? <Spinner /> : <Trash2 />}
          </Button>
        </div>
        <div
          className="log-stream"
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
            <pre>
              {lines.map((line, index) => (
                <span
                  key={index}
                  className={`log-entry log-${line.startsWith("ERROR ") ? "error" : line.startsWith("WARN ") ? "warn" : "info"}`}
                >
                  {line}
                </span>
              ))}
            </pre>
          ) : (
            <p className="log-empty">
              {lines === null
                ? "正在读取日志…"
                : "暂无 MCP 日志，启动连接入口或发起请求后会在这里显示。"}
            </p>
          )}
        </div>
      </div>
    </section>
  );
}
