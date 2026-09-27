#!/usr/bin/env python3
"""Automated Unix PTY test of the interactive CLI, including host mode restoration."""
import argparse
import fcntl
import os
from pathlib import Path
import pty
import select
import signal
import struct
import termios
import time
import tempfile
import subprocess

p=argparse.ArgumentParser()
p.add_argument('binary',type=Path)
p.add_argument('--shell', default='/bin/sh')
p.add_argument('--vim', action='store_true')
p.add_argument('--colors', choices=['256', 'rgb', 'ansi'], default='256')
p.add_argument('--startup-failure', action='store_true')
a=p.parse_args()
binary=a.binary.resolve()
state=tempfile.TemporaryDirectory(prefix="ai-terminal-host-")
pid,master=pty.fork()
if pid==0:
    fcntl.ioctl(1,termios.TIOCSWINSZ,struct.pack('HHHH',24,80,0,0))
    os.environ['TERM']='xterm' if a.colors == 'ansi' else 'xterm-256color'
    os.environ['TERM_PROGRAM']='iTerm.app' if a.colors == 'rgb' else 'Apple_Terminal'
    os.environ.pop('COLORTERM', None)
    if a.colors == 'rgb': os.environ['COLORTERM']='truecolor'
    # Exercise the shells without loading user rc files or writing user history.
    os.environ['HISTFILE']='/dev/null'
    os.environ['ENV']='/dev/null'
    shell_name=Path(a.shell).name
    flags=['-f','-i'] if shell_name == 'zsh' else ['--noprofile','--norc','-i'] if shell_name == 'bash' else ['-i']
    command=['vim','-Nu','NONE','-n','-i','NONE'] if a.vim else [a.shell,*flags]
    if a.startup_failure: command=['/aterminal-test-does-not-exist']
    os.execv(str(binary),[str(binary),'--state-dir',state.name,'--',*command])
output=bytearray();sent=False;quit_sent=False;status=None;deadline=time.monotonic()+15
try:
    while time.monotonic()<deadline:
        ready,_,_=select.select([master],[],[],0.05)
        if ready:
            try:
                block=os.read(master,65536)
                if not block:break
                output.extend(block)
            except OSError:break
        ready_to_type=b'~' in output if a.vim else b'\x1b[?2004h' in output
        if not a.startup_failure and not sent and ready_to_type:
            os.write(master,b":echo 'VIM_'.'OK'\r" if a.vim else b"printf '\\137\\137PTY_UI_OK\\137\\137\\n'; exit\r")
            sent=True
        if a.vim and not quit_sent and b'VIM_OK' in output:
            os.write(master,b':qa!\r');quit_sent=True
        if status is None:
            done,code=os.waitpid(pid,os.WNOHANG)
            if done:status=code
    while status is None and time.monotonic()<deadline:
        done,code=os.waitpid(pid,os.WNOHANG)
        if done:status=code
        else:time.sleep(0.01)
    if status is None:raise AssertionError('interactive CLI failed to exit within 15 seconds')
    assert (os.waitstatus_to_exitcode(status)!=0 if a.startup_failure else os.waitstatus_to_exitcode(status)==0),output[-2000:]
    # The command uses octal escapes, so this can only appear in child output.
    marker=b'VIM_OK' if a.vim else b'__PTY_UI_OK__'
    if not a.startup_failure:
        assert marker in output,output[-2000:]
        if a.colors == 'rgb': assert b';38;2;' in output, 'truecolor lost'
        elif a.colors == '256':
            assert b';38;5;' in output, 'indexed colors missing'
            assert b';38;2;' not in output, 'unsupported truecolor emitted'
        else: assert b';38;' not in output, 'basic ANSI mode emitted extended colors'
    assert b'\x1b[2J\x1b[1;1H' in output, 'entry did not clear/home'
    assert b'\x1b[3J' not in output, 'entry erased host scrollback'
    assert b'\x1b[?1049l' in output,'alternate screen was not restored'
    flags=termios.tcgetattr(master)[3]
    assert flags & termios.ICANON and flags & termios.ECHO,'raw mode was not restored'
    print('PASS:', 'startup failure' if a.startup_failure else 'Vim' if a.vim else a.shell, a.colors, 'PTY input/output, clear/home, alternate screen, canonical mode, echo restoration')
finally:
    if status is None:
        try:os.kill(pid,signal.SIGKILL);os.waitpid(pid,0)
        except ProcessLookupError:pass
    os.close(master)
    subprocess.run([str(binary),"--state-dir",state.name,"--agent-stop"],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=5)
    state.cleanup()
