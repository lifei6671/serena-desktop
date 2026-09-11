import { useRef } from "react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import type { RemoteController } from "./useRemoteAccess";

export function RemoteApprovalDialog({ controller }: { controller: RemoteController }) {
  const { state, approvalBusy, approvalError, approve } = controller;
  const pending = state?.pending[0];
  const deny = useRef<HTMLButtonElement>(null);
  const previousFocus = useRef<HTMLElement | null>(null);
  async function decide(allow: boolean) {
    if (pending && !approvalBusy) await approve(pending.id, allow);
  }
  return <Dialog open={!!pending} onOpenChange={open => { if (!open) void decide(false); }}>
    <DialogContent className="max-h-[calc(100dvh-2rem)] overflow-y-auto" key={pending?.id} showCloseButton={false} onOpenAutoFocus={event => { event.preventDefault(); previousFocus.current = document.activeElement as HTMLElement | null; deny.current?.focus(); }} onCloseAutoFocus={event => { event.preventDefault(); if (previousFocus.current?.isConnected) previousFocus.current.focus(); }} onPointerDownOutside={event => event.preventDefault()}>
      <DialogHeader><DialogTitle>客户端请求连接 SerenaDesktop</DialogTitle><DialogDescription>请核对浏览器与本机的确认码一致。只有你在这台电脑上允许后，客户端才能访问 MCP。</DialogDescription></DialogHeader>
      <div className="remote-confirmation" aria-label="确认码">{pending?.confirmationCode}</div>
      <dl className="remote-approval-details"><dt>客户端声明名称</dt><dd>{pending?.clientName}</dd><dt>回调地址</dt><dd>{pending?.redirectUri}</dd><dt>权限</dt><dd>访问本机 MCP，包括当前公开的工具（{pending?.scope}）</dd><dt>授权持续性</dt><dd>{pending?.refreshAllowed ? "允许持续授权：访问令牌有效期最长一小时，客户端可自动刷新；授权有效期为 72 小时，每次成功刷新后续期 72 小时。自建固定地址模式可跨应用重启恢复授权，停止远程访问会撤销授权。" : "仅本次访问：客户端未注册刷新流程，访问令牌有效期最长一小时，不签发刷新令牌。"}</dd></dl>
      <p className="helper">{pending?.expiresInSeconds ?? 0} 秒后过期{(state?.pending.length ?? 0) > 1 ? ` · 还有 ${state!.pending.length - 1} 个请求等待处理` : ""}</p>
      {approvalError && <p role="alert" className="remote-error">{approvalError}</p>}
      <DialogFooter><Button ref={deny} variant="outline" disabled={approvalBusy} onClick={() => void decide(false)}>拒绝</Button><Button disabled={approvalBusy || !pending || pending.expiresInSeconds === 0} onClick={() => void decide(true)}>{approvalBusy ? "正在处理…" : "允许连接"}</Button></DialogFooter>
    </DialogContent>
  </Dialog>;
}
