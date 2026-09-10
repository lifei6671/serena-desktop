import { memo } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";

export const MarkdownContent = memo(function MarkdownContent({ children, className = "" }: { children: string; className?: string }) {
  return <div className={`agent-prose agent-markdown ${className}`}>
    <Markdown remarkPlugins={[remarkGfm]} components={{
      a: ({ href, children }) => href ? <a href={href} target="_blank" rel="noopener noreferrer">{children}</a> : <span>{children}</span>,
      pre: ({ children }) => <pre tabIndex={0}>{children}</pre>,
      table: ({ children }) => <div className="agent-markdown-table" role="region" aria-label="Markdown 表格" tabIndex={0}><table>{children}</table></div>,
    }}>{children}</Markdown>
  </div>;
});
