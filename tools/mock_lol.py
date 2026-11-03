"""
Simulate a League of Legends match for end-to-end testing of Matt Game Recorder.

  py -I tools/mock_lol.py [--length 90]

* Starts a stand-in game: a copy of ffplay named "League of Legends.exe" showing moving
  test video with a tone, so window capture and audio loopback have real content.
* Serves the Live Client Data API on https://127.0.0.1:2999 (self-signed cert), playing a
  scripted match: kill, double kill, death, assist, enemy turret, ally dragon, victory.
* Exits the stand-in game a few seconds after GameEnd, like the real client does.
"""

import argparse
import json
import os
import shutil
import ssl
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

ROOT = Path(__file__).resolve().parent
FFPLAY = ROOT.parent / "src-tauri" / "binaries" / "ffplay.exe"
OPENSSL = Path(r"C:\Program Files\Git\usr\bin\openssl.exe")

ME = {"riotId": "MattTest#NA1", "riotIdGameName": "MattTest", "summonerName": "MattTest"}
ALLY = "AllyBuddy"
ENEMY = "EnemyGuy"
LOADING = 6.0  # seconds of "loading screen" before game time starts


def script(length: float):
    s = length / 90.0
    return [
        {"EventName": "GameStart", "EventTime": 0.05},
        {"EventName": "FirstBlood", "EventTime": 18 * s, "Recipient": ME["riotIdGameName"]},
        {"EventName": "ChampionKill", "EventTime": 18 * s, "KillerName": ME["riotIdGameName"], "VictimName": ENEMY, "Assisters": [ALLY]},
        {"EventName": "ChampionKill", "EventTime": 20 * s, "KillerName": ME["riotIdGameName"], "VictimName": "EnemyTwo", "Assisters": []},
        {"EventName": "Multikill", "EventTime": 20 * s, "KillerName": ME["riotIdGameName"], "KillStreak": 2},
        {"EventName": "ChampionKill", "EventTime": 34 * s, "KillerName": ENEMY, "VictimName": ME["riotIdGameName"], "Assisters": []},
        {"EventName": "ChampionKill", "EventTime": 46 * s, "KillerName": ALLY, "VictimName": ENEMY, "Assisters": [ME["riotIdGameName"]]},
        {"EventName": "TurretKilled", "EventTime": 55 * s, "KillerName": ALLY, "TurretKilled": "Turret_T2_C_05_A", "Assisters": []},
        {"EventName": "TurretKilled", "EventTime": 58 * s, "KillerName": ENEMY, "TurretKilled": "Turret_T1_L_03_A", "Assisters": []},
        {"EventName": "DragonKill", "EventTime": 64 * s, "KillerName": ALLY, "DragonType": "Infernal", "Stolen": "False", "Assisters": []},
        {"EventName": "GameEnd", "EventTime": 76 * s, "Result": "Win"},
    ]


class State:
    def __init__(self, length):
        self.start = time.time()
        self.events = script(length)
        for i, e in enumerate(self.events):
            e["EventID"] = i

    def game_time(self):
        return max(0.0, time.time() - self.start - LOADING)

    def visible_events(self):
        gt = self.game_time()
        return [e for e in self.events if e["EventTime"] <= gt]

    def kda(self):
        k = d = a = 0
        for e in self.visible_events():
            if e["EventName"] == "ChampionKill":
                if e["KillerName"] == ME["riotIdGameName"]:
                    k += 1
                elif e["VictimName"] == ME["riotIdGameName"]:
                    d += 1
                elif ME["riotIdGameName"] in e["Assisters"]:
                    a += 1
        return k, d, a


STATE: State


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def send_json(self, obj, code=200):
        body = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        gt = STATE.game_time()
        if gt <= 0:
            # The real API refuses connections while loading.
            return self.send_json({"errorCode": "RESOURCE_NOT_FOUND"}, 404)
        path = self.path.split("?")[0]
        if path == "/liveclientdata/gamestats":
            return self.send_json({"gameMode": "CLASSIC", "gameTime": gt, "mapName": "Map11", "mapNumber": 11})
        if path == "/liveclientdata/eventdata":
            return self.send_json({"Events": STATE.visible_events()})
        if path == "/liveclientdata/activeplayer":
            return self.send_json({**ME, "level": 9, "abilities": {"R": {"abilityLevel": 1 if gt > 30 else 0}}})
        if path == "/liveclientdata/playerlist":
            k, d, a = STATE.kda()
            players = [
                {**ME, "team": "ORDER", "championName": "Yasuo", "rawChampionName": "game_character_displayname_Yasuo", "scores": {"kills": k, "deaths": d, "assists": a}},
                {"riotId": ALLY + "#NA1", "riotIdGameName": ALLY, "summonerName": ALLY, "team": "ORDER", "championName": "Leona", "rawChampionName": "game_character_displayname_Leona", "scores": {}},
                {"riotId": ENEMY + "#NA1", "riotIdGameName": ENEMY, "summonerName": ENEMY, "team": "CHAOS", "championName": "Zed", "rawChampionName": "game_character_displayname_Zed", "scores": {}},
                {"riotId": "EnemyTwo#NA1", "riotIdGameName": "EnemyTwo", "summonerName": "EnemyTwo", "team": "CHAOS", "championName": "Lux", "rawChampionName": "game_character_displayname_Lux", "scores": {}},
            ]
            return self.send_json(players)
        return self.send_json({"errorCode": "RESOURCE_NOT_FOUND"}, 404)


def ensure_cert(dirpath: Path):
    cert, key = dirpath / "mock.crt", dirpath / "mock.key"
    if not cert.exists():
        subprocess.run(
            [str(OPENSSL), "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "30", "-subj", "/CN=127.0.0.1", "-keyout", str(key), "-out", str(cert)],
            check=True,
            capture_output=True,
        )
    return cert, key


def main():
    global STATE
    ap = argparse.ArgumentParser()
    ap.add_argument("--length", type=float, default=90, help="seconds of game time until GameEnd is ~85%% of this")
    args = ap.parse_args()

    work = Path(tempfile.mkdtemp(prefix="mock-lol-"))
    cert, key = ensure_cert(work)
    exe = work / "League of Legends.exe"
    shutil.copy(FFPLAY, exe)

    # The overlay shows wall-clock time, so a frame at an event mark can be checked against
    # the printed game start: event wall time = game start + EventTime.
    graph = r"testsrc2=s=1600x900:r=60,drawtext=text='MOCK LEAGUE %{localtime\:%X}':fontsize=64:fontcolor=white:x=40:y=40[out0];sine=f=330:b=2,volume=0.15[out1]"
    game = subprocess.Popen([str(exe), "-hide_banner", "-loglevel", "error", "-window_title", "League of Legends (TM) Client", "-f", "lavfi", graph])
    print(f"stand-in game pid {game.pid}", flush=True)
    STATE = State(args.length)
    game_zero = time.localtime(STATE.start + LOADING)
    print(f"game time 0 = {time.strftime('%H:%M:%S', game_zero)} (+{(STATE.start + LOADING) % 1:.2f}s)", flush=True)
    for e in STATE.events:
        print(f"  {e['EventName']:<13} at {time.strftime('%H:%M:%S', time.localtime(STATE.start + LOADING + e['EventTime']))}", flush=True)

    httpd = ThreadingHTTPServer(("127.0.0.1", 2999), Handler)
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.load_cert_chain(cert, key)
    httpd.socket = ctx.wrap_socket(httpd.socket, server_side=True)
    threading.Thread(target=httpd.serve_forever, daemon=True).start()
    print("live client API on https://127.0.0.1:2999", flush=True)

    end_time = next(e["EventTime"] for e in STATE.events if e["EventName"] == "GameEnd")
    try:
        while True:
            gt = STATE.game_time()
            if gt > end_time + 5:
                break
            if game.poll() is not None:
                print("stand-in game closed early", flush=True)
                break
            time.sleep(0.5)
    finally:
        game.terminate()
        httpd.shutdown()
        shutil.rmtree(work, ignore_errors=True)
    print(f"match over (game time {STATE.game_time():.1f}s), KDA {STATE.kda()}", flush=True)


if __name__ == "__main__":
    sys.exit(main())
