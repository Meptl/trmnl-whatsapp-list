#!/usr/bin/env python3
"""Generate a Google Calendar OAuth refresh token for this service."""

from __future__ import annotations

import json
import os
import secrets
import sys
import urllib.parse
import urllib.request
import webbrowser
from http.server import BaseHTTPRequestHandler, HTTPServer

AUTH_URL = "https://accounts.google.com/o/oauth2/v2/auth"
TOKEN_URL = "https://oauth2.googleapis.com/token"
SCOPE = "https://www.googleapis.com/auth/calendar.readonly"


class OAuthCallbackHandler(BaseHTTPRequestHandler):
    server: "OAuthCallbackServer"

    def do_GET(self) -> None:
        parsed_url = urllib.parse.urlparse(self.path)
        query = urllib.parse.parse_qs(parsed_url.query)
        state = query.get("state", [""])[0]

        if parsed_url.path != "/callback" or state != self.server.expected_state:
            self.send_response(400)
            self.end_headers()
            self.wfile.write(b"Invalid OAuth callback. You can close this tab.")
            return

        self.server.authorization_code = query.get("code", [None])[0]
        self.server.authorization_error = query.get("error", [None])[0]

        self.send_response(200)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.end_headers()
        if self.server.authorization_code:
            self.wfile.write(b"Authorization received. You can close this tab and return to the terminal.\n")
        else:
            self.wfile.write(b"Authorization failed. You can close this tab and return to the terminal.\n")

    def log_message(self, format: str, *args: object) -> None:
        return


class OAuthCallbackServer(HTTPServer):
    def __init__(self, server_address: tuple[str, int], expected_state: str) -> None:
        super().__init__(server_address, OAuthCallbackHandler)
        self.expected_state = expected_state
        self.authorization_code: str | None = None
        self.authorization_error: str | None = None


def required_env(name: str) -> str:
    value = os.environ.get(name)
    if not value:
        print(f"{name} is required in the environment", file=sys.stderr)
        sys.exit(2)
    return value


def build_authorization_url(client_id: str, redirect_uri: str, state: str) -> str:
    query = urllib.parse.urlencode(
        {
            "client_id": client_id,
            "redirect_uri": redirect_uri,
            "response_type": "code",
            "scope": SCOPE,
            "access_type": "offline",
            "prompt": "consent",
            "state": state,
        }
    )
    return f"{AUTH_URL}?{query}"


def exchange_code_for_refresh_token(
    client_id: str,
    client_secret: str,
    redirect_uri: str,
    authorization_code: str,
) -> str:
    body = urllib.parse.urlencode(
        {
            "client_id": client_id,
            "client_secret": client_secret,
            "redirect_uri": redirect_uri,
            "code": authorization_code,
            "grant_type": "authorization_code",
        }
    ).encode("utf-8")
    request = urllib.request.Request(
        TOKEN_URL,
        data=body,
        headers={"Content-Type": "application/x-www-form-urlencoded"},
        method="POST",
    )

    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            payload = json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        detail = error.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"token exchange failed with HTTP {error.code}: {detail}") from error
    except urllib.error.URLError as error:
        raise RuntimeError(f"token exchange failed: {error.reason}") from error

    refresh_token = payload.get("refresh_token")
    if not refresh_token:
        raise RuntimeError(
            "Google did not return a refresh_token. Re-run this script and approve the consent "
            "screen, or revoke the app at https://myaccount.google.com/permissions and try again."
        )
    return refresh_token


def main() -> int:
    client_id = required_env("GOOGLE_CALENDAR_CLIENT_ID")
    client_secret = required_env("GOOGLE_CALENDAR_CLIENT_SECRET")
    state = secrets.token_urlsafe(24)

    server = OAuthCallbackServer(("127.0.0.1", 0), state)
    redirect_uri = f"http://127.0.0.1:{server.server_port}/callback"
    authorization_url = build_authorization_url(client_id, redirect_uri, state)

    print("Opening browser for Google Calendar authorization...", file=sys.stderr)
    print(f"If the browser does not open, visit:\n{authorization_url}\n", file=sys.stderr)
    webbrowser.open(authorization_url)

    while server.authorization_code is None and server.authorization_error is None:
        server.handle_request()

    if server.authorization_error:
        print(f"Authorization failed: {server.authorization_error}", file=sys.stderr)
        return 1

    assert server.authorization_code is not None
    try:
        refresh_token = exchange_code_for_refresh_token(
            client_id,
            client_secret,
            redirect_uri,
            server.authorization_code,
        )
    except RuntimeError as error:
        print(error, file=sys.stderr)
        return 1

    print(refresh_token)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
