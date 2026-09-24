#!/usr/bin/env python3
"""Exercise the real MCP process with a fake say; no audible playback.

Usage: python3 scripts/test_speech_queue.py [path/to/speak-mcp]
"""
import json
import os
from pathlib import Path
import select
import subprocess
import sys
import tempfile
import time


binary = str(Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/speak-mcp").resolve())
with tempfile.TemporaryDirectory() as directory:
    root = Path(directory)
    log = root / "events"
    fake = root / "say"
    fake.write_text(f"#!{sys.executable}\n" + '''
import json, os, sys, time
def record(event):
    with open(os.environ['SPEECH_TEST_LOG'], 'a') as f:
        f.write(json.dumps({'event': event, 'text': sys.argv[-1], 'pid': os.getpid(), 'args': sys.argv[1:]}) + '\\n')
record('start')
time.sleep(30 if sys.argv[-1] == 'hold' else 0.4)
record('end')
sys.exit(1 if sys.argv[-1] == 'fail' else 0)
''')
    fake.chmod(0o755)
    env = dict(os.environ, PATH=directory + os.pathsep + os.environ['PATH'], SPEECH_TEST_LOG=str(log))
    p = subprocess.Popen([binary], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         stderr=subprocess.PIPE, env=env, bufsize=0)
    sequence = 0

    def send(message):
        p.stdin.write((json.dumps(message) + '\n').encode())
        p.stdin.flush()

    def request(method, params=None):
        global sequence
        sequence += 1
        message = {'jsonrpc': '2.0', 'id': sequence, 'method': method}
        if params is not None:
            message['params'] = params
        send(message)
        assert select.select([p.stdout], [], [], 5)[0], 'MCP response timed out'
        response = json.loads(p.stdout.readline())
        assert response['id'] == sequence, response
        return response

    def speak(text, **args):
        return request('tools/call', {'name': 'speak', 'arguments': {'text': text, **args}})

    def events():
        return [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []

    def wait_for(predicate):
        deadline = time.monotonic() + 5
        while not predicate():
            assert time.monotonic() < deadline, events()
            time.sleep(0.02)

    try:
        assert 'result' in request('initialize', {
            'protocolVersion': '2024-11-05', 'capabilities': {},
            'clientInfo': {'name': 'queue-test', 'version': '1'},
        })
        send({'jsonrpc': '2.0', 'method': 'notifications/initialized'})
        assert 'result' in request('tools/list')  # Regression: omitted params.
        for params in ({}, {'cursor': 'test'}):
            assert 'result' in request('tools/list', params)

        ids = []
        for text in ('first', 'fail', 'last'):
            response = speak(text, voice='Karen', speed=180)
            accepted = json.loads(response['result']['content'][0]['text'])
            assert accepted['status'] == 'queued', response
            ids.append(accepted['job_id'])
        assert len(set(ids)) == 3
        assert not any(e['event'] == 'end' for e in events()), 'Response waited for playback'
        wait_for(lambda: len(events()) == 6)
        assert [(e['event'], e['text']) for e in events()] == [
            (event, text) for text in ('first', 'fail', 'last') for event in ('start', 'end')
        ], events()
        assert events()[0]['args'] == ['-v', 'Karen', '-r', '180', '--', 'first']
        assert 'error' in speak('   ')
        assert 'error' in speak('invalid speed', speed=0)

        assert 'result' in speak('hold')
        wait_for(lambda: any(e['text'] == 'hold' for e in events()))
        child_pid = events()[-1]['pid']
        for i in range(64):
            assert 'result' in speak(f'pending-{i}')
        response = speak('overflow')
        assert 'queue is full' in response['error']['message'], response
        # Closing stdin must stop the active child and discard pending jobs.
        p.stdin.close()
        assert p.wait(timeout=5) == 0
        try:
            os.kill(child_pid, 0)
        except ProcessLookupError:
            pass
        else:
            raise AssertionError('Playback child survived server EOF')
        assert not any(e['text'].startswith('pending-') for e in events())
        assert 'failed' in p.stderr.read().decode()
        print('PASS: immediate acceptance, FIFO, no overlap, failure recovery, validation, queue capacity, EOF cleanup')
    finally:
        if p.poll() is None:
            p.kill()
        p.wait()
