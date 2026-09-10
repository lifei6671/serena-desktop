import type { ReactElement } from "react";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

export function TooltipHint({ content, children }: { content?: string | null; children: ReactElement }) {
  if (!content) return children;
  return <Tooltip><TooltipTrigger asChild>{children}</TooltipTrigger>
    <TooltipContent sideOffset={6}>
      <div className="min-w-0 max-h-80 overflow-y-auto whitespace-pre-wrap break-all">{content}</div>
    </TooltipContent>
  </Tooltip>;
}
