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
    config_dir = root / '.config' / 'speak-mcp'
    config_dir.mkdir(parents=True)
    (config_dir / 'config.json').write_text(json.dumps({
        'voicevox_default_speaker': None,
        'aivis_default_speaker': None,
        'locale': {
            'en_US': 'Nathan (Enhanced)',
            'en_AU': 'Karen (Premium)',
            'en_UK': 'Jamie (Enhanced)',
            'ko_KR': 'Yuna (Premium)',
            'ja_JP': 'Custom Japanese Voice',
        },
    }))
    env = dict(os.environ, PATH=directory + os.pathsep + os.environ['PATH'], SPEECH_TEST_LOG=str(log))
    env['HOME'] = directory
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
        tools_response = request('tools/list')  # Regression: omitted params.
        assert 'result' in tools_response
        speak_tool = next(tool for tool in tools_response['result']['tools'] if tool['name'] == 'speak')
        schema = speak_tool['inputSchema']
        assert schema['required'] == ['text', 'locale']
        assert schema['properties']['locale'] == {'type': 'string'}
        assert 'voice' not in schema['properties']
        for params in ({}, {'cursor': 'test'}):
            assert 'result' in request('tools/list', params)

        ids = []
        for text in ('first', 'fail', 'last'):
            response = speak(text, locale='en_AU', speed=180)
            accepted = json.loads(response['result']['content'][0]['text'])
            assert accepted['status'] == 'queued', response
            ids.append(accepted['job_id'])
        assert len(set(ids)) == 3
        assert not any(e['event'] == 'end' for e in events()), 'Response waited for playback'
        wait_for(lambda: len(events()) == 6)
        assert [(e['event'], e['text']) for e in events()] == [
            (event, text) for text in ('first', 'fail', 'last') for event in ('start', 'end')
        ], events()
        assert events()[0]['args'] == ['-v', 'Karen (Premium)', '-r', '180', '--', 'first']
        for locale, voice in {
            'en_US': 'Nathan (Enhanced)',
            'en_AU': 'Karen (Premium)',
            'en_UK': 'Jamie (Enhanced)',
            'ko_KR': 'Yuna (Premium)',
            'ja_JP': 'Custom Japanese Voice',
        }.items():
            assert 'result' in speak(f'locale-{locale}', locale=locale)
            wait_for(lambda: any(e['event'] == 'start' and e['text'] == f'locale-{locale}' for e in events()))
            event = next(e for e in events() if e['event'] == 'start' and e['text'] == f'locale-{locale}')
            assert event['args'] == ['-v', voice, '-r', '200', '--', f'locale-{locale}']
        assert 'error' in speak('missing locale')
        assert 'result' in speak('unsupported locale', locale='fr_FR')
        assert 'error' in speak('   ', locale='en_US')
        assert 'error' in speak('invalid speed', locale='en_US', speed=0)

        assert 'result' in speak('batch-first', locale='en_US')
        wait_for(lambda: any(e['event'] == 'start' and e['text'] == 'batch-first' for e in events()))
        config_path = config_dir / 'config.json'
        config = json.loads(config_path.read_text())
        config['locale']['en_US'] = 'Updated Voice'
        config['rate'] = 240
        config_path.write_text(json.dumps(config))
        assert 'result' in speak('batch-second', locale='en_US')
        wait_for(lambda: any(e['event'] == 'end' and e['text'] == 'batch-second' for e in events()))
        second = next(e for e in events() if e['event'] == 'start' and e['text'] == 'batch-second')
        assert second['args'][1] == 'Nathan (Enhanced)', second
        assert second['args'][3] == '200', second
        time.sleep(0.1)
        assert 'result' in speak('hold', locale='en_US')
        wait_for(lambda: any(e['text'] == 'hold' for e in events()))
        assert events()[-1]['args'][1] == 'Updated Voice'
        assert events()[-1]['args'][3] == '240'
        child_pid = events()[-1]['pid']
        for i in range(64):
            assert 'result' in speak(f'pending-{i}', locale='en_US')
        response = speak('overflow', locale='en_US')
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
