"""Publish rattler-build to crates.io using GitHub Actions trusted publishing.

Exchanges a GitHub OIDC token for a short-lived crates.io publish token,
then runs `cargo publish`.

Usage:
    pixi run -e release publish-crates-io
"""

import os
import subprocess
import sys

import httpx

CRATES_IO_AUDIENCE = "crates.io"
CRATES_IO_TOKEN_URL = "https://crates.io/api/v1/trusted_publishing/tokens"
USER_AGENT = "prefix-dev/rattler-build CI"


def get_oidc_token() -> str:
    """Request an OIDC token from the GitHub Actions runtime."""
    request_url = os.environ.get("ACTIONS_ID_TOKEN_REQUEST_URL")
    request_token = os.environ.get("ACTIONS_ID_TOKEN_REQUEST_TOKEN")

    if not request_url or not request_token:
        print(
            "error: ACTIONS_ID_TOKEN_REQUEST_URL and ACTIONS_ID_TOKEN_REQUEST_TOKEN "
            "must be set (needs `permissions: id-token: write` in the workflow)",
            file=sys.stderr,
        )
        sys.exit(1)

    response = httpx.get(
        request_url,
        params={"audience": CRATES_IO_AUDIENCE},
        headers={
            "User-Agent": "actions/oidc-client",
            "Authorization": f"Bearer {request_token}",
        },
    )
    response.raise_for_status()
    return response.json()["value"]


def exchange_for_publish_token(oidc_token: str) -> str:
    """Exchange a GitHub OIDC token for a crates.io publish token."""
    response = httpx.post(
        CRATES_IO_TOKEN_URL,
        headers={"User-Agent": USER_AGENT},
        json={"jwt": oidc_token},
    )
    response.raise_for_status()
    return response.json()["token"]


def main() -> None:
    print("Requesting OIDC token from GitHub Actions...")
    oidc_token = get_oidc_token()

    print("Exchanging OIDC token for crates.io publish token...")
    publish_token = exchange_for_publish_token(oidc_token)

    print("Publishing to crates.io...")
    result = subprocess.run(
        ["cargo", "publish", "--package", "rattler-build"],
        env={**os.environ, "CARGO_REGISTRY_TOKEN": publish_token},
    )
    sys.exit(result.returncode)


if __name__ == "__main__":
    main()
