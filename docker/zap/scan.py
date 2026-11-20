#!/usr/bin/env python3
"""Scan passif OWASP ZAP d'easybook, **connecté** en administrateur.

Lancé par le service `zap` du docker-compose.yml (voir le README). Il ouvre une
session avec `ZAP_EMAIL` / `ZAP_PASSWORD`, puis donne le cookie à ZAP : sans lui,
ZAP ne verrait que la page de connexion.

`zap-baseline.py` est passif (il observe le trafic de son robot d'exploration) :
il ne tente aucune attaque. Rapports : `zap-reports/zap-report.{html,json}`.
"""
import json
import os
import subprocess
import sys
import urllib.error
import urllib.request

target = os.environ.get("ZAP_TARGET", "http://easybook:8080").rstrip("/")
email = os.environ.get("ZAP_EMAIL", "")
password = os.environ.get("ZAP_PASSWORD", "")


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


def session_cookie() -> str:
    request = urllib.request.Request(
        f"{target}/api/auth/login",
        data=json.dumps({"email": email, "password": password}).encode(),
        headers={"content-type": "application/json"},
    )
    opener = urllib.request.build_opener(NoRedirect, urllib.request.ProxyHandler({}))
    try:
        with opener.open(request, timeout=20) as response:
            cookies = response.headers.get_all("set-cookie") or []
    except urllib.error.HTTPError as e:
        sys.exit(f"Connexion refusée ({e.code}) : vérifier ZAP_EMAIL / ZAP_PASSWORD.")
    for line in cookies:
        if line.startswith("easybook_session="):
            return line.split(";", 1)[0]
    sys.exit("Pas de cookie de session dans la réponse de connexion.")


command = [
    "zap-baseline.py",
    "-t", target,
    "-r", "zap-report.html",
    "-J", "zap-report.json",
    # N'échoue pas sur un avertissement : le rapport est fait pour être lu.
    "-I",
]
if email:
    cookie = session_cookie()
    print(f"Connecté en tant que {email} : scan authentifié.", flush=True)
    rule = "replacer.full_list(0)"
    command += [
        "-z",
        " ".join(
            f"-config {rule}.{key}={value}"
            for key, value in [
                ("description", "session"),
                ("enabled", "true"),
                ("matchtype", "REQ_HEADER"),
                ("matchstr", "Cookie"),
                ("regex", "false"),
                ("replacement", cookie),
            ]
        ),
    ]
else:
    print("Pas d'identifiants : scan anonyme (page de connexion seulement).", flush=True)

sys.exit(subprocess.call(command))
