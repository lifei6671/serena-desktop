//! 复用 CB5-002 的 external streams tee 和有界进程清理。
use super::*;
pub type Wire = Arc<Mutex<Vec<(String, Vec<u8>)>>>;
/// Tee 只复制真实读写成功的字节，SDK 负责 JSON-RPC 编解码。
pub struct Tee<T> {
    pub inner: T,
    pub direction: &'static str,
    pub wire: Wire,
    pub line_bytes: usize,
    pub frame_count: usize,
}
impl<T> Tee<T> {
    /// 接收边界即限制单帧与帧总量，避免 SDK 内部队列无限增长。
    fn bounded(&mut self, bytes: &[u8]) -> io::Result<()> {
        for b in bytes {
            self.line_bytes += 1;
            if self.line_bytes > 1_048_576 {
                return Err(io::Error::other("FRAME_LIMIT"));
            }
            if *b == b'\n' {
                self.line_bytes = 0;
                self.frame_count += 1;
            }
            if self.frame_count > 2048 {
                return Err(io::Error::other("QUEUE_LIMIT"));
            }
        }
        Ok(())
    }
}
impl<T: AsyncRead + Unpin> AsyncRead for Tee<T> {
    /// 捕获接收分片。
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        if self
            .wire
            .lock()
            .unwrap()
            .iter()
            .map(|(_, b)| b.len())
            .sum::<usize>()
            >= 4 * 1024 * 1024
        {
            return Poll::Ready(Err(io::Error::other("WIRE_LIMIT")));
        }
        let cap = buf.len().min(8192);
        match Pin::new(&mut self.inner).poll_read(cx, &mut buf[..cap]) {
            Poll::Ready(Ok(n)) => {
                if n > 0 {
                    if let Err(e) = self.bounded(&buf[..n]) {
                        return Poll::Ready(Err(e));
                    }
                    self.wire
                        .lock()
                        .unwrap()
                        .push((self.direction.into(), buf[..n].to_vec()));
                }
                Poll::Ready(Ok(n))
            }
            other => other,
        }
    }
}
impl<T: AsyncWrite + Unpin> AsyncWrite for Tee<T> {
    /// 仅记录成功写入部分，避免把失败发送当成 wire 证据。
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self
            .wire
            .lock()
            .unwrap()
            .iter()
            .map(|(_, b)| b.len())
            .sum::<usize>()
            + buf.len()
            > 4 * 1024 * 1024
        {
            return Poll::Ready(Err(io::Error::other("WIRE_LIMIT")));
        }
        match Pin::new(&mut self.inner).poll_write(cx, buf) {
            Poll::Ready(Ok(n)) => {
                if n > 0 {
                    if let Err(e) = self.bounded(&buf[..n]) {
                        return Poll::Ready(Err(e));
                    }
                    self.wire
                        .lock()
                        .unwrap()
                        .push((self.direction.into(), buf[..n].to_vec()));
                }
                Poll::Ready(Ok(n))
            }
            other => other,
        }
    }
    /// 刷新真实写入管道。
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    /// SDK 关闭时关闭外部写入流。
    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_close(cx)
    }
}
/// 清理失败也继续执行后续回收；每一步有界，失败事实写入 evidence。
pub async fn cleanup_child(
    child: &mut tokio::process::Child,
    stderr_task: &mut tokio::task::JoinHandle<Vec<u8>>,
    first_wait: Option<io::Result<std::process::ExitStatus>>,
) -> Value {
    cleanup_with_kill(
        child,
        stderr_task,
        first_wait,
        tokio::process::Child::start_kill,
    )
    .await
}
/// 注入 kill 边界仅供故障回归；真实路径使用 Child::start_kill。
pub async fn cleanup_with_kill(
    child: &mut tokio::process::Child,
    stderr_task: &mut tokio::task::JoinHandle<Vec<u8>>,
    first_wait: Option<io::Result<std::process::ExitStatus>>,
    kill: fn(&mut tokio::process::Child) -> io::Result<()>,
) -> Value {
    let mut errors: Vec<String> = Vec::new();
    let mut status = match first_wait {
        Some(Ok(status)) => Some(status),
        Some(Err(_)) => {
            errors.push("INITIAL_WAIT_ERROR".into());
            None
        }
        None => None,
    };
    let terminated = status.is_none();
    if terminated {
        if kill(child).is_err() {
            errors.push("KILL_ERROR".into());
        }
        // 即使 kill 返回错误，也必须尝试 wait；不伪造进程退出证据。
        match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
            Ok(Ok(exit)) => status = Some(exit),
            Ok(Err(_)) => errors.push("FINAL_WAIT_ERROR".into()),
            Err(_) => errors.push("final_wait: timeout".into()),
        }
    }
    let mut stderr_joined = false;
    let stderr_bytes = match tokio::time::timeout(Duration::from_secs(1), &mut *stderr_task).await {
        Ok(Ok(bytes)) => {
            stderr_joined = true;
            Some(bytes)
        }
        Ok(Err(_)) => {
            stderr_joined = true;
            errors.push("STDERR_JOIN_ERROR".into());
            None
        }
        Err(_) => {
            stderr_task.abort();
            match tokio::time::timeout(Duration::from_secs(1), &mut *stderr_task).await {
                Ok(_) => stderr_joined = true,
                Err(_) => errors.push("stderr_abort_join: timeout".into()),
            }
            None
        }
    };
    json!({"streamsClosed":true,"terminated":terminated,"waited":status.is_some(),"exitCode":status.and_then(|s|s.code()),"directChildReaped":status.is_some(),"stderrJoined":stderr_joined,"succeeded":errors.is_empty()&&status.is_some()&&stderr_joined,"errors":errors,"stderrBytesDrainedCapped8192":stderr_bytes.map(|b|b.len()),"windowsJobAtCreationProven":false})
}
