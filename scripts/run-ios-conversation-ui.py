#!/usr/bin/env python3
"""Run offline native conversation fixtures in a dedicated disposable simulator."""
import ctypes
import json
from datetime import datetime, timezone
import os
from pathlib import Path
import platform
import subprocess
import sys
import threading
import time

ROOT = Path(__file__).resolve().parent.parent


def simctl(*arguments):
    return subprocess.check_output(['xcrun', 'simctl', *arguments], text=True).strip()


class FixtureSampler:
    """Bounded host stacks for a long-lived app in this disposable simulator only."""

    def __init__(self, device, output):
        self.device = device
        self.output = output
        self.stopping = threading.Event()
        self.thread = threading.Thread(target=self._run, name='ios-fixture-sampler', daemon=True)
        self.pidpath = None

    def start(self):
        try:
            self.thread.start()
        except RuntimeError:
            print('iOS fixture stack collector could not start.', file=sys.stderr)

    def close(self):
        self.stopping.set()
        if self.thread.ident is not None:
            self.thread.join(timeout=8)
            if self.thread.is_alive():
                print('iOS fixture stack collector reached its shutdown limit.', file=sys.stderr)

    def _record(self, event, **fields):
        with (self.output / 'collector.jsonl').open('a') as stream:
            stream.write(json.dumps({'time': datetime.now(timezone.utc).isoformat(),
                                     'event': event, **fields}) + '\n')

    def _read(self, command):
        if self.stopping.is_set():
            return ''
        result = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                text=True, timeout=3, check=False)
        return result.stdout.strip() if result.returncode == 0 else ''

    def _process_path(self, pid):
        if self.stopping.is_set():
            return None
        if self.pidpath is None:
            # Load only in the protected collector thread. An unavailable host
            # diagnostic API must never prevent xcodebuild from running.
            self.pidpath = ctypes.CDLL('/usr/lib/libproc.dylib').proc_pidpath
            self.pidpath.argtypes = [ctypes.c_int, ctypes.c_void_p, ctypes.c_uint32]
            self.pidpath.restype = ctypes.c_int
        path = ctypes.create_string_buffer(4096)  # PROC_PIDPATHINFO_MAXSIZE
        if self.pidpath(pid, path, ctypes.sizeof(path)) <= 0:
            return None
        return Path(os.fsdecode(path.value)).resolve()

    def _fixture_processes(self):
        container = self._read(['xcrun', 'simctl', 'get_app_container', self.device,
                                'com.yxf.aterminal', 'app'])
        if not container:
            return {}
        app = Path(container).resolve()
        bundles = (Path.home() / 'Library/Developer/CoreSimulator/Devices' / self.device
                   / 'data/Containers/Bundle/Application').resolve()
        if app.name != 'aTerminal.app' or not app.is_relative_to(bundles):
            self._record('container_rejected')
            return {}
        executable = (app / 'aTerminal').resolve()
        if not executable.is_relative_to(app):
            self._record('executable_rejected')
            return {}
        # Darwin ucomm is the kernel's short process name. comm instead uses
        # argv[0], so only proc_pidpath can establish the real executable here.
        # Neither argv nor environment is read or recorded.
        listing = self._read(['/bin/ps', '-axo', 'pid=,ucomm='])
        matches = {}
        for line in listing.splitlines():
            parts = line.strip().split(None, 1)
            if (len(parts) != 2 or not parts[0].isdigit() or int(parts[0]) <= 0
                    or parts[1] != 'aTerminal'):
                continue
            if self._process_path(int(parts[0])) == executable:
                matches[int(parts[0])] = str(executable)
        return matches

    def _sample(self, pid, executable, number):
        if self.stopping.is_set():
            return
        report = self.output / f'sample-{number:02d}-pid-{pid}.txt'
        log = self.output / f'sample-{number:02d}-pid-{pid}.log'
        self._record('sample_started', pid=pid, executable=executable, number=number,
                     duration_seconds=3, interval_milliseconds=10)
        with log.open('w') as stream:
            process = subprocess.Popen(['/usr/bin/sample', str(pid), '3', '10', '-file', str(report)],
                                       stdout=stream, stderr=subprocess.STDOUT)
            deadline = time.monotonic() + 15
            status = 'completed'
            try:
                while process.poll() is None:
                    if self.stopping.wait(0.2) or time.monotonic() >= deadline:
                        status = 'cancelled' if self.stopping.is_set() else 'timed_out'
                        # This handle belongs to sample, never to the app being sampled.
                        process.terminate()
                        try:
                            process.wait(timeout=1)
                        except subprocess.TimeoutExpired:
                            process.kill()
                            process.wait(timeout=1)
                        break
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=1)
        self._record('sample_finished', pid=pid, number=number, status=status,
                     returncode=process.returncode, report=report.name)

    def _run(self):
        count = 0
        try:
            self.output.mkdir(parents=True, exist_ok=True)
            self._record('collector_started', device=self.device, max_samples=12,
                         max_samples_per_pid=2, first_sample_after_seconds=30,
                         minimum_interval_seconds=15)
            first_seen, next_sample, attempts = {}, {}, {}
            while count < 12 and not self.stopping.wait(5):
                try:
                    processes = self._fixture_processes()
                    now = time.monotonic()
                    first_seen = {pid: seen for pid, seen in first_seen.items() if pid in processes}
                    for pid, executable in processes.items():
                        if pid not in first_seen:
                            first_seen[pid] = now
                            self._record('fixture_observed', pid=pid, executable=executable)
                        if (attempts.get(pid, 0) >= 2 or now - first_seen[pid] < 30
                                or now < next_sample.get(pid, 0)):
                            continue
                        # A fixture may exit between probes. Recheck PID + path
                        # immediately before attaching; never fall back to a name.
                        if self._fixture_processes().get(pid) != executable or self.stopping.is_set():
                            continue
                        attempts[pid] = attempts.get(pid, 0) + 1
                        count += 1
                        try:
                            self._sample(pid, executable, count)
                        finally:
                            next_sample[pid] = time.monotonic() + 15
                        if count >= 12 or self.stopping.is_set():
                            break
                except (OSError, subprocess.SubprocessError) as error:
                    # Diagnosis must not alter the native test result or retry it.
                    self._record('probe_failed', error=type(error).__name__)
            self._record('collector_stopped', samples=count, cancelled=self.stopping.is_set())
        except Exception as error:
            # Keep a diagnostic failure separate from xcodebuild's exit status.
            print('iOS fixture stack collector unavailable: ' + type(error).__name__, file=sys.stderr)


def main():
    if platform.system() != 'Darwin':
        raise SystemExit('Native iOS UI tests require macOS and Xcode.')
    runtimes = json.loads(simctl('list', 'runtimes', '--json'))['runtimes']
    available = [item for item in runtimes if item.get('isAvailable') and '.iOS-' in item['identifier']]
    if not available:
        raise SystemExit('No available iOS simulator runtime.')
    runtime = max(available, key=lambda item: tuple(map(int, item['version'].split('.'))))
    parts = list(map(int, runtime['version'].split('.'))) + [0, 0]
    version = (parts[0] << 16) + (parts[1] << 8) + parts[2]
    devices = json.loads(simctl('list', 'devicetypes', '--json'))['devicetypes']
    phones = [item for item in devices if item['name'].startswith('iPhone')
              and item.get('minRuntimeVersion', 0) <= version <= item.get('maxRuntimeVersion', 0xFFFFFFFF)]
    if not phones:
        raise SystemExit('No iPhone simulator supports the installed iOS runtime.')
    phone = next((item for item in phones if item['name'] == 'iPhone 16'), phones[-1])
    device = simctl('create', 'aTerminal conversation CI', phone['identifier'], runtime['identifier'])
    output = ROOT / 'build/reconnect-evidence/ios-conversation.xcresult'
    output.parent.mkdir(parents=True, exist_ok=True)
    sampler = FixtureSampler(device, output.parent / 'ios-hang-diagnostics')
    tests = [
        'ChatDeliveryUITests',
        'AuthorizationUITests',
        'WorkspaceUITests/testSessionAndGlobalDraftsStaySeparateAndReturnToList',
        'WorkspaceUITests/testClosedAndOfflineHistoryRemainReachableAndReadOnly',
        'WorkspaceUITests/testBackgroundClearsConnectionBusyAndIgnoresOldFailure',
    ]
    try:
        simctl('boot', device)
        subprocess.run(['xcrun', 'simctl', 'bootstatus', device, '-b'], check=True)
        # Select only offline fixtures; live-account/RPC suites require separate setup.
        command = ['xcodebuild', '-project', 'apps/ios/aTerminal.xcodeproj', '-scheme', 'aTerminal',
                   '-sdk', 'iphonesimulator', '-configuration', 'Debug',
                   '-destination', 'platform=iOS Simulator,id=' + device,
                   '-derivedDataPath', 'build/xcode-conversation', '-resultBundlePath', str(output),
                   '-jobs', '2', '-parallel-testing-enabled', 'NO', 'CODE_SIGN_IDENTITY=-',
                   *['-only-testing:aTerminalUITests/' + test for test in tests], 'test']
        sampler.start()
        return subprocess.run(command, cwd=ROOT, check=False).returncode
    finally:
        sampler.close()
        # Only the simulator created above is touched; preserve results on failure.
        subprocess.run(['xcrun', 'simctl', 'shutdown', device], check=False)
        subprocess.run(['xcrun', 'simctl', 'delete', device], check=False)


if __name__ == '__main__':
    raise SystemExit(main())
