"""仅观察本次 probe 的进程树；不枚举输出用户进程或声称 Job containment。"""
import ctypes
from ctypes import wintypes
import json
from pathlib import Path
import subprocess
import time

ROOT = Path(__file__).resolve().parent
KERNEL = ctypes.WinDLL('kernel32', use_last_error=True)


class ProcessEntry(ctypes.Structure):
    """Windows PROCESSENTRY32W 对应布局。"""
    _fields_ = [('size', wintypes.DWORD), ('usage', wintypes.DWORD),
                ('pid', wintypes.DWORD), ('heap', ctypes.c_size_t),
                ('module', wintypes.DWORD), ('threads', wintypes.DWORD),
                ('parent', wintypes.DWORD), ('priority', wintypes.LONG),
                ('flags', wintypes.DWORD), ('exe', wintypes.WCHAR * 260)]


KERNEL.CreateToolhelp32Snapshot.argtypes = [wintypes.DWORD, wintypes.DWORD]
KERNEL.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
KERNEL.Process32FirstW.argtypes = [wintypes.HANDLE, ctypes.POINTER(ProcessEntry)]
KERNEL.Process32NextW.argtypes = [wintypes.HANDLE, ctypes.POINTER(ProcessEntry)]
KERNEL.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
KERNEL.OpenProcess.restype = wintypes.HANDLE
KERNEL.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
KERNEL.WaitForSingleObject.restype = wintypes.DWORD
KERNEL.TerminateProcess.argtypes = [wintypes.HANDLE, wintypes.UINT]
KERNEL.CloseHandle.argtypes = [wintypes.HANDLE]


def snapshot():
    """短时只读快照；调用方只保留自有 root 下的记录。"""
    handle = KERNEL.CreateToolhelp32Snapshot(2, 0)
    if handle == ctypes.c_void_p(-1).value:
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        entry = ProcessEntry()
        entry.size = ctypes.sizeof(entry)
        rows = []
        more = KERNEL.Process32FirstW(handle, ctypes.byref(entry))
        while more:
            rows.append((entry.pid, entry.parent, entry.exe))
            more = KERNEL.Process32NextW(handle, ctypes.byref(entry))
        error = ctypes.get_last_error()
        if error not in (0, 18):
            raise ctypes.WinError(error)
        return rows
    finally:
        KERNEL.CloseHandle(handle)


def main():
    """观察有界运行，finally 仅回收本次拥有或已确认的进程句柄。"""
    output = ROOT / 'evidence/real-cli.json'
    executable = ROOT / 'harness/target/debug/cb5-002-initialize-probe.exe'
    start = time.monotonic()
    child = subprocess.Popen([str(executable), 'real', str(output)], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    known = {child.pid: {'pid': child.pid, 'role': 'harness'}}
    handles = {}
    errors = []
    timed_out = False
    samples = 0
    exit_code = None
    try:
        finished_at = None
        while True:
            rows = snapshot()
            samples += 1
            # 同一快照可能无序，迭代闭包只扩展本次 root 的子树。
            changed = True
            while changed:
                changed = False
                for pid, parent, exe in rows:
                    if parent in known and pid not in known:
                        known[pid] = {'pid': pid, 'parentPid': parent, 'exe': exe, 'firstSeenMs': round((time.monotonic()-start)*1000)}
                        handle = KERNEL.OpenProcess(0x00100001, False, pid)
                        if handle:
                            handles[pid] = handle
                        else:
                            errors.append(f'OpenProcess {pid}: {ctypes.get_last_error()}')
                        changed = True
            if child.poll() is not None:
                finished_at = finished_at or time.monotonic()
                if time.monotonic() - finished_at >= 0.15:
                    break
            if time.monotonic()-start > 40:
                timed_out = True
                break
            time.sleep(0.01)
    except OSError as error:
        errors.append(str(error))
    finally:
        # 先确认正常退出；异常路径仅清理已持有的自有 descendant handles。
        for pid, handle in reversed(list(handles.items())):
            observed_exit = KERNEL.WaitForSingleObject(handle, 0) == 0
            known[pid]['exitedBeforeObserverCleanup'] = observed_exit
            if not observed_exit:
                errors.append(f'owned descendant still running: {pid}')
                if not KERNEL.TerminateProcess(handle, 1):
                    errors.append(f'TerminateProcess {pid}: {ctypes.get_last_error()}')
            known[pid]['exitObserved'] = KERNEL.WaitForSingleObject(handle, 3000) == 0
            KERNEL.CloseHandle(handle)
        try:
            if child.poll() is None:
                child.kill()
        except OSError as error:
            errors.append(f'harness kill: {error}')
        finally:
            try:
                exit_code = child.wait(timeout=5)
            except (OSError, subprocess.TimeoutExpired) as error:
                errors.append(f'harness wait: {error}')
        # 关闭 pipe，不依赖输出 EOF 证明进程退出。
        child.stdout.close()
        child.stderr.close()
    report = {'method': 'Toolhelp32Snapshot + held process handles, observed owned tree only', 'sampleIntervalMs': 10, 'samples': samples, 'timeoutSeconds': 40, 'timedOut': timed_out, 'harnessExitCode': exit_code, 'observedProcesses': list(known.values()), 'errors': errors, 'allObservedDescendantsExited': bool(handles) and all(known[pid].get('exitObserved') for pid in handles), 'windowsJobAtCreationProven': False, 'limitation': 'sampling is not atomic containment and cannot prove no unobserved short-lived descendant ever existed'}
    (ROOT/'evidence/real-cli-process-tree.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf8')
    print(json.dumps(report))
    if errors or timed_out or exit_code != 0:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
